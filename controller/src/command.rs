use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use openssh::Stdio;

use crate::session::{OpenSSHChild, Session, Sessions};
// use crate::session::{Session, Sessions};
use crate::DEFAULT_INTERPRETER;

#[derive(Debug)]
pub enum Error {
    Build(String),
    TokioCommand(io::Error),
    OpenSSHCommand(openssh::Error),
    TaskJoin(tokio::task::JoinError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Build(ref message) => write!(f, "{message}"),
            Self::TokioCommand(..) => {
                write!(f, "Error running a local command with tokio")
            }
            Self::OpenSSHCommand(..) => {
                write!(f, "Error a command with OpenSSH")
            }
            Self::TaskJoin(..) => write!(f, "Error joining task"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::TokioCommand(ref source) => Some(source),
            Self::OpenSSHCommand(ref source) => Some(source),
            Self::TaskJoin(ref source) => Some(source),
            _ => None,
        }
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(value: tokio::task::JoinError) -> Self {
        Error::TaskJoin(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::TokioCommand(value)
    }
}

impl From<openssh::Error> for Error {
    fn from(value: openssh::Error) -> Self {
        Error::OpenSSHCommand(value)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
type Tasks = Vec<tokio::task::JoinHandle<Result<ExecutionResult>>>;

/// A [`ShellCommand`] is a helper for constructing common commands that we use throughout Experimentah.
/// It is essentially a builder for a vector of string arguments that we can use in multiple SSH functions.
#[derive(Debug, Default, Clone)]
pub struct ShellCommand {
    interpreter: &'static str,
    command: String,
    args: Vec<String>,
    working_directory: Option<PathBuf>,
    stdout_file: Option<PathBuf>,
    stderr_file: Option<PathBuf>,
    /// Captures the PID of the running process into a file for future reference
    pid_file: Option<PathBuf>,
    /// Uses the 'flock' command available on most Linux distributions to prevent running
    /// commands until a file lock is no longer held.
    advisory_lock_file: Option<PathBuf>,
}

impl ShellCommand {
    const INTERPRETERS: [&str; 2] = ["sh", "bash"];
    /// A helper function to ensure that none of the arguments contain spaces.
    // Maybe this is stupid, but so far I haven't needed spaces anywhere in commands.
    fn format_shell_command<S: AsRef<str>>(shell_command: &[S]) -> Vec<String> {
        let mut new = Vec::with_capacity(shell_command.len());
        for part in shell_command.iter() {
            let new_part = part.as_ref().trim();
            new.push(new_part.to_string());
        }

        new
    }

    // Constructs a new ShellCommand builder with a command
    pub fn from_command<S: AsRef<str>>(command: S) -> Self {
        let command = Self::format_shell_command(&[command]).pop().unwrap();

        Self {
            interpreter: DEFAULT_INTERPRETER,
            command,
            ..Default::default()
        }
    }

    pub fn from_command_args<S: AsRef<str>>(command_args: &[S]) -> Self {
        assert!(!command_args.is_empty());
        let mut command_args = Self::format_shell_command(command_args);
        let command = command_args.remove(0);

        Self {
            interpreter: DEFAULT_INTERPRETER,
            command,
            args: command_args,
            ..Default::default()
        }
    }

    pub fn command<S: AsRef<str>>(&mut self, command: S) -> &mut Self {
        let command = Self::format_shell_command(&[command]).pop().unwrap();

        self.command = command;
        self
    }

    pub fn args<S: AsRef<str>>(&mut self, args: &[S]) -> &mut Self {
        let args = Self::format_shell_command(args);
        self.args = args;
        self
    }

    pub fn command_args<S: AsRef<str>>(
        &mut self,
        command_args: &[S],
    ) -> &mut Self {
        assert!(!command_args.is_empty());
        let mut command_args = Self::format_shell_command(command_args);
        let command = command_args.remove(0);

        self.command = command;
        self.args = command_args;
        self
    }

    pub fn interpreter(&mut self, interpreter: &'static str) -> &mut Self {
        self.interpreter = interpreter;
        self
    }

    pub fn working_directory<P: AsRef<Path>>(
        &mut self,
        working_directory: P,
    ) -> &mut Self {
        self.working_directory = Some(working_directory.as_ref().into());
        self
    }

    pub fn stdout_file<P: AsRef<Path>>(&mut self, stdout_file: P) -> &mut Self {
        self.stdout_file = Some(stdout_file.as_ref().into());
        self
    }

    pub fn stderr_file<P: AsRef<Path>>(&mut self, stderr_file: P) -> &mut Self {
        self.stderr_file = Some(stderr_file.as_ref().into());
        self
    }

    pub fn pid_file<P: AsRef<Path>>(&mut self, pid_file: P) -> &mut Self {
        self.pid_file = Some(pid_file.as_ref().into());
        self
    }

    pub fn advisory_lock_file<P: AsRef<Path>>(
        &mut self,
        advisory_lock_file: P,
    ) -> &mut Self {
        self.advisory_lock_file = Some(advisory_lock_file.as_ref().into());
        self
    }

    // pub fn

    pub fn build(&self) -> Result<Vec<String>> {
        if self.command.contains(char::is_whitespace) {
            return Err(Error::Build(format!(
                "Command '{}' contains whitespace",
                &self.command
            )));
        }

        if !Self::INTERPRETERS.contains(&self.interpreter) {
            return Err(Error::Build(format!(
                "Interpreter '{}' is not allowed. Expected one of {:?}",
                &self.interpreter,
                Self::INTERPRETERS
            )));
        }

        let mut shell_command = vec![];

        if let Some(advisory_lock_file) = &self.advisory_lock_file {
            shell_command.push("flock".into());
            shell_command.push("-n".into());
            shell_command
                .push(advisory_lock_file.to_string_lossy().to_string());
        }

        shell_command.push(self.interpreter.to_string());
        shell_command.push("-c".into());

        let mut args: Vec<String> = vec![];

        if let Some(working_directory) = &self.working_directory {
            args.push("cd".into());
            args.push(working_directory.to_string_lossy().into());
            args.push("&&".into());
        }

        // We capture our shell's PID and then replace the shell with our process.
        if let Some(pid_file) = &self.pid_file {
            args.push("echo".into());
            //For some reason this always double-backslashes. We need a single backslash to get the
            //subprocess PID out.
            args.push("$$".into());
            args.push(format!(">{};", pid_file.to_string_lossy()));
            args.push("exec".into());
        }

        args.push(self.command.clone());
        for arg in self.args.iter() {
            args.push(format!("\"{arg}\""));
        }

        if let Some(stdout_file) = &self.stdout_file {
            args.push(format!(">{}", stdout_file.to_string_lossy()));
        }

        if let Some(stderr_file) = &self.stderr_file {
            args.push(format!("2>{}", stderr_file.to_string_lossy()));
        }

        shell_command.push(args.join(" "));

        Ok(shell_command)
    }

    pub fn build_remote_arc_command(
        &self,
        session: &Arc<openssh::Session>,
    ) -> Result<openssh::OwningCommand<Arc<openssh::Session>>> {
        let comm_args = self.build()?;
        let (comm, args) = comm_args.split_at(1);
        let comm = comm.first().unwrap();
        let mut owned_comm = session.clone().arc_command(comm);
        owned_comm
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(owned_comm)
    }

    pub fn build_remote_command<'a>(
        &self,
        session: &'a Arc<openssh::Session>,
    ) -> Result<openssh::OwningCommand<&'a openssh::Session>> {
        let comm_args = self.build()?;
        let (comm, args) = comm_args.split_at(1);
        let comm = comm.first().unwrap();
        let mut owned_comm = session.command(comm);
        owned_comm
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Ok(owned_comm)
    }

    pub fn build_tokio_command(&self) -> Result<tokio::process::Command> {
        let comm_args = self.build()?;
        let (comm, args) = comm_args.split_first().unwrap();
        let mut tokio_comm = tokio::process::Command::new(comm);
        tokio_comm.args(args);

        Ok(tokio_comm)
    }
}

/// A [`CommandExecutor`] handles running shell commands in the correct context, whether it needs
/// to be done locally or remotely across multiple hosts.
pub trait CommandExecutor {
    fn execute(
        &self,
        shell_command: Arc<ShellCommand>,
        execution_type: Arc<ExecutionType>,
    ) -> impl std::future::Future<Output = Result<Vec<ExecutionResult>>>;
}

/// An [`ExecutionType`] determines how we want a [`ShellCommand`] to be run. This allows us to
/// choose between running to completion and spawning a background task.
pub enum ExecutionType {
    /// Run a process to completion and collect it's output
    Output,
    /// Run a process in the background, to be handled in another (green) thread.
    Spawn,
    /// Run a process to completion and collect it's exit code
    Status,
}

/// An [`ExecutionResult`] wraps the return type for common [`ShellCommand`] spawning operations.
/// This is meant to serve as an adaptor between different implementations of Output, ExitStatus
/// and Child.
#[derive(Debug)]
pub enum ExecutionResult {
    Status(std::process::ExitStatus),
    Output(std::process::Output),
    Spawn(SpawnType),
}

#[derive(Debug)]
pub enum SpawnType {
    SSH(OpenSSHChild),
    Tokio(tokio::process::Child),
}

/// A [`LocalCommandExecutor`] runs [`ShellCommand`]s on the local host.
#[derive(Debug)]
pub struct LocalCommandExecutor;

impl LocalCommandExecutor {
    fn new() -> Self {
        Self {}
    }
}

impl CommandExecutor for LocalCommandExecutor {
    async fn execute(
        &self,
        shell_command: Arc<ShellCommand>,
        execution_type: Arc<ExecutionType>,
    ) -> Result<Vec<ExecutionResult>> {
        let mut command = shell_command.build_tokio_command()?;

        let out = match *execution_type {
            ExecutionType::Output => {
                command.stdin(std::process::Stdio::null());
                command.stdout(std::process::Stdio::piped());
                command.stderr(std::process::Stdio::piped());
                ExecutionResult::Output(command.output().await?)
            }
            ExecutionType::Spawn => {
                command.stdin(std::process::Stdio::null());
                command.stdout(std::process::Stdio::piped());
                command.stderr(std::process::Stdio::piped());
                ExecutionResult::Spawn(SpawnType::Tokio(command.spawn()?))
            }
            ExecutionType::Status => {
                command.stdin(std::process::Stdio::null());
                command.stdout(std::process::Stdio::null());
                command.stderr(std::process::Stdio::null());
                ExecutionResult::Status(command.status().await?)
            }
        };

        Ok(vec![out])
    }
}

/// A [`RemoteCommandExecutor`] runs [`ShellCommand`]s on multiple SSH sessions simultaneously.
#[derive(Debug)]
pub struct RemoteCommandExecutor {
    sessions: Vec<Arc<openssh::Session>>,
}

impl RemoteCommandExecutor {
    pub fn new(sessions: Vec<Arc<openssh::Session>>) -> Self {
        Self { sessions }
    }
}

impl CommandExecutor for RemoteCommandExecutor {
    async fn execute(
        &self,
        // Probably need to Arc<> our ShellCommand somewhere
        // Perhaps just consume a ShellCommand within this function and make it an Arc
        shell_command: Arc<ShellCommand>,
        execute_type: Arc<ExecutionType>,
    ) -> Result<Vec<ExecutionResult>> {
        let mut futures: Tasks = vec![];

        for session in self.sessions.iter() {
            let shell_command_clone = shell_command.clone();
            let session_clone = session.clone();
            let execute_type_clone = execute_type.clone();
            futures.push(tokio::spawn(async move {
                let mut command = shell_command_clone
                    .build_remote_arc_command(&session_clone)?;

                let out: ExecutionResult = match *execute_type_clone {
                    ExecutionType::Output => {
                        ExecutionResult::Output(command.output().await?)
                    }
                    ExecutionType::Spawn => ExecutionResult::Spawn(
                        SpawnType::SSH(command.spawn().await?),
                    ),
                    ExecutionType::Status => {
                        ExecutionResult::Status(command.status().await?)
                    }
                };

                Ok(out)
            }));
        }

        let mut results: Vec<ExecutionResult> =
            Vec::with_capacity(futures.len());
        for future in futures {
            results.push(future.await??);
        }

        Ok(results)
    }
}

pub struct UnifiedExecutor {
    local_executor: Arc<LocalCommandExecutor>,
    remote_executor: Arc<RemoteCommandExecutor>,
}

impl UnifiedExecutor {
    pub fn new(
        local_executor: LocalCommandExecutor,
        remote_executor: RemoteCommandExecutor,
    ) -> Self {
        Self {
            local_executor: Arc::new(local_executor),
            remote_executor: Arc::new(remote_executor),
        }
    }
}

impl CommandExecutor for Arc<UnifiedExecutor> {
    async fn execute(
        &self,
        shell_command: Arc<ShellCommand>,
        execution_type: Arc<ExecutionType>,
    ) -> Result<Vec<ExecutionResult>> {
        let local_command = shell_command.clone();
        let local_execution_type = execution_type.clone();
        let local_self = self.clone();

        let local_future = tokio::spawn(async move {
            local_self
                .local_executor
                .execute(local_command, local_execution_type)
                .await
        });

        let remote_command = shell_command.clone();
        let remote_execution_type = execution_type.clone();
        let remote_self = self.clone();
        // let remote_executor = Arc::new(self.remote_executor);
        let remote_future = tokio::spawn(async move {
            remote_self
                .remote_executor
                .execute(remote_command, remote_execution_type)
                .await
        });

        let results: Vec<ExecutionResult> = local_future
            .await??
            .into_iter()
            .chain(remote_future.await??)
            .collect();

        Ok(results)
    }
}

pub enum Executor {
    Unified(Arc<UnifiedExecutor>),
    Local(Arc<LocalCommandExecutor>),
    Remote(Arc<RemoteCommandExecutor>),
}

impl Executor {
    pub fn from_sessions(sessions: &Sessions) -> Executor {
        let mut has_local: bool = false;
        let mut remote_sessions: Vec<Arc<openssh::Session>> = Vec::new();

        for (_host, session) in sessions.iter() {
            match session {
                Session::Local => {
                    has_local = true;
                }
                Session::Remote(ssh_session) => {
                    remote_sessions.push(ssh_session.clone());
                }
            }
        }

        if has_local && !remote_sessions.is_empty() {
            let local_executor = LocalCommandExecutor::new();
            let remote_executor = RemoteCommandExecutor::new(remote_sessions);
            Executor::Unified(Arc::new(UnifiedExecutor::new(
                local_executor,
                remote_executor,
            )))
        } else if !remote_sessions.is_empty() {
            Executor::Remote(Arc::new(RemoteCommandExecutor::new(
                remote_sessions,
            )))
        } else {
            Executor::Local(Arc::new(LocalCommandExecutor::new()))
        }
    }

    /// Sometimes we need to execute a command on a single session.
    pub fn from_session(session: Arc<openssh::Session>) -> Executor {
        Executor::Remote(Arc::new(RemoteCommandExecutor::new(vec![session])))
        // match session {
        //     Session::Local => Executor::Local(Arc::new(LocalCommandExecutor)),
        //     Session::Remote(ssh_session) => {
        //         Executor::Remote(Arc::new(RemoteCommandExecutor::new(vec![
        //             ssh_session,
        //         ])))
        //     }
        // }
    }
}

impl CommandExecutor for Executor {
    async fn execute(
        &self,
        shell_command: Arc<ShellCommand>,
        execution_type: Arc<ExecutionType>,
    ) -> Result<Vec<ExecutionResult>> {
        match *self {
            Self::Unified(ref executor) => {
                executor.execute(shell_command, execution_type).await
            }
            Self::Local(ref executor) => {
                executor.execute(shell_command, execution_type).await
            }
            Self::Remote(ref executor) => {
                executor.execute(shell_command, execution_type).await
            }
        }
    }
}

/// Runs a command across sessions which may or may not need to be executed locally and/or
/// remotely.
/// This is the general case within Experimentah, but there are some times where only local/remote
/// commands are expected.
pub async fn run_command(
    sessions: &Sessions,
    shell_command: ShellCommand,
    execution_type: ExecutionType,
) -> Result<Vec<ExecutionResult>> {
    let executor = Executor::from_sessions(sessions);
    executor
        .execute(shell_command.into(), execution_type.into())
        .await
}

pub async fn run_command_openssh_session(
    session: Arc<openssh::Session>,
    shell_command: ShellCommand,
    execution_type: ExecutionType,
) -> Result<ExecutionResult> {
    let executor = Executor::from_session(session);
    Ok(executor
        .execute(shell_command.into(), execution_type.into())
        .await?
    .pop().expect("Somehow there wasn't any execution results in a single session command"))
}

pub async fn run_local_command(
    shell_command: ShellCommand,
    execution_type: ExecutionType,
) -> Result<Vec<ExecutionResult>> {
    let executor = LocalCommandExecutor::new();
    executor
        .execute(shell_command.into(), execution_type.into())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// We use a ShellCommand struct to create all the shell commands that we require. It's
    /// important that the commands created by this struct work as intended.

    #[test]
    fn shcomm_bad_commands() {
        assert!(ShellCommand::from_command("echo hi").build().is_err());
        assert!(ShellCommand::from_command_args(&["ec ho", "hi"])
            .build()
            .is_err());

        let mut shell_command = ShellCommand::from_command("echo");
        shell_command.command("ec ho");
        assert!(shell_command.build().is_err());

        let mut shell_command = ShellCommand::from_command("echo");
        shell_command.command_args(&["ec ho", "hi"]);
        assert!(shell_command.build().is_err());

        let mut shell_command = ShellCommand::from_command("echo");
        shell_command.interpreter("zsh");
        assert!(shell_command.build().is_err());
    }

    #[test]
    fn shcomm_valid_constructions() {
        // Command constructions
        let mut shell_command = ShellCommand::from_command("echo");
        assert!(shell_command.build().is_ok());
        assert!(shell_command.command(" echo").build().is_ok());
        assert!(shell_command.command("echo ").build().is_ok());
        assert!(shell_command.command(" echo ").build().is_ok());

        // Command_args constructions
        let mut shell_command =
            ShellCommand::from_command_args(&["echo", "Hello", "World!"]);
        assert!(shell_command.build().is_ok());
        assert!(shell_command
            .command_args(&["echo", "Hel lo", " Worl d! "])
            .build()
            .is_ok());
        assert!(shell_command
            .command_args(&[" echo ", " Hello", "World! "])
            .build()
            .is_ok());

        // Args constructions
        let mut shell_command = ShellCommand::from_command("echo");
        assert!(shell_command
            .args(&[" He llo ", " World !"])
            .build()
            .is_ok());

        // Interpreter constructions
        let mut shell_command = ShellCommand::from_command("echo");
        assert!(shell_command.interpreter("bash").build().is_ok());
        assert!(shell_command.interpreter("sh").build().is_ok());

        // Directory/File constructions
        let mut shell_command = ShellCommand::from_command("echo");
        assert!(shell_command
            .working_directory(Path::new("/srv/experimentah"))
            .build()
            .is_ok());
        assert!(shell_command
            .stdout_file(Path::new("/srv/experimentah/testfile.stdout"))
            .build()
            .is_ok());
        assert!(shell_command
            .stderr_file(Path::new("/srv/experimentah/testfile.stderr"))
            .build()
            .is_ok());

        assert!(shell_command
            .pid_file("/srv/experimentah/testfile.pid")
            .build()
            .is_ok());

        assert!(shell_command
            .advisory_lock_file("/srv/experimentah/testfile.lock")
            .build()
            .is_ok());
    }

    #[test]
    fn shcomm_build_returns() {
        let mut shell_command = ShellCommand::from_command("echo");
        compare_commands(
            shell_command.build().unwrap(),
            &["bash", "-c", "echo"],
        );
        shell_command.command("ln");
        compare_commands(shell_command.build().unwrap(), &["bash", "-c", "ln"]);
        shell_command.args(&["-s", "-t", "my-favorite-directory"]);
        compare_commands(
            shell_command.build().unwrap(),
            &["bash", "-c", "ln \"-s\" \"-t\" \"my-favorite-directory\""],
        );

        shell_command.command_args(&["echo", "Hello", "World!"]);
        compare_commands(
            shell_command.build().unwrap(),
            &["bash", "-c", "echo \"Hello\" \"World!\""],
        );

        shell_command.interpreter("sh");
        compare_commands(
            shell_command.build().unwrap(),
            &["sh", "-c", "echo \"Hello\" \"World!\""],
        );

        shell_command.working_directory(Path::new("/home/myuser/mydirectory"));
        compare_commands(
            shell_command.build().unwrap(),
            &[
                "sh",
                "-c",
                concat!(
                    "cd ",
                    "/home/myuser/mydirectory ",
                    "&& ",
                    "echo ",
                    "\"Hello\" ",
                    "\"World!\"",
                ),
            ],
        );

        shell_command.stdout_file("/home/myuser/mydirectory/echofile.stdout");
        compare_commands(
            shell_command.build().unwrap(),
            &[
                "sh",
                "-c",
                concat!(
                    "cd ",
                    "/home/myuser/mydirectory ",
                    "&& ",
                    "echo ",
                    "\"Hello\" ",
                    "\"World!\" ",
                    ">/home/myuser/mydirectory/echofile.stdout"
                ),
            ],
        );

        shell_command.stderr_file("/home/myuser/mydirectory/echofile.stderr");
        compare_commands(
            shell_command.build().unwrap(),
            &[
                "sh",
                "-c",
                concat!(
                    "cd ",
                    "/home/myuser/mydirectory ",
                    "&& ",
                    "echo ",
                    "\"Hello\" ",
                    "\"World!\" ",
                    ">/home/myuser/mydirectory/echofile.stdout ",
                    "2>/home/myuser/mydirectory/echofile.stderr"
                ),
            ],
        );

        shell_command.pid_file("/home/myuser/mydirectory/echofile.pid");
        compare_commands(
            shell_command.build().unwrap(),
            &[
                "sh",
                "-c",
                concat!(
                    "cd ",
                    "/home/myuser/mydirectory ",
                    "&& ",
                    "echo ",
                    "$$ ",
                    ">/home/myuser/mydirectory/echofile.pid; ",
                    "exec ",
                    "echo ",
                    "\"Hello\" ",
                    "\"World!\" ",
                    ">/home/myuser/mydirectory/echofile.stdout ",
                    "2>/home/myuser/mydirectory/echofile.stderr"
                ),
            ],
        );

        // How should we perform the flock. Unfortunately, at the moment using flock doesn't allow
        // us to get our PID very easily. :/
        // Probably, we'll need to have a separate case for PID capture when flock is in use
        shell_command
            .advisory_lock_file("/home/myuser/mydirectory/echofile.lock");
        compare_commands(
            shell_command.build().unwrap(),
            &[
                "flock",
                "-n",
                "/home/myuser/mydirectory/echofile.lock",
                "sh",
                "-c",
                concat!(
                    "cd ",
                    "/home/myuser/mydirectory ",
                    "&& ",
                    "echo ",
                    "$$ ",
                    ">/home/myuser/mydirectory/echofile.pid; ",
                    "exec ",
                    "echo ",
                    "\"Hello\" ",
                    "\"World!\" ",
                    ">/home/myuser/mydirectory/echofile.stdout ",
                    "2>/home/myuser/mydirectory/echofile.stderr"
                ),
            ],
        );
    }

    fn compare_commands(command_1: Vec<String>, command_2: &[&str]) {
        assert_eq!(
            command_1,
            command_2
                .iter()
                .map(|part| part.to_string())
                .collect::<Vec<String>>(),
        );
    }
}
