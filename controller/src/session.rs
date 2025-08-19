//! Functionality for SSH operations, such as running commands, creating background processes,
//! making directories, downloading files, etc. is provided in this module.

use openssh::{Child, KnownHosts};
use std::collections::HashMap;
use std::path::Path;
use std::string::FromUtf8Error;
use std::sync::Arc;
use std::{fmt, io};
use tokio::task::{self};

// TODO(joren): This will likely disappear later
use crate::command::{self, ExecutionType, ShellCommand};

// pub type BackgroundProcess = Child<Arc<openssh::Session>>;
// pub type BackgroundProcesses = Vec<BackgroundProcess>;
pub type OpenSSHChild = Child<Arc<openssh::Session>>;
pub type Sessions = HashMap<String, Session>;

/// A session can represent a remote host or a local connection
#[derive(Debug, Clone)]
pub enum Session {
    Local,
    Remote(Arc<openssh::Session>),
}

// pub struct SshSession {
//     pub session: Arc<openssh::Session>,
// }

pub struct LocalSession {}

/// A specialised [`Result`] type for SSH problems that happen during many stages of the Experiment
/// lifecycle.
///
/// This type is broadly used across [`crate::session`] for any operation which may produce an
/// error.
///
/// This typedef is generally used to avoid writing out [`crate::session::Error`] directly and is
/// otherwise a direct mapping to [`Result`].
pub type Result<T> = std::result::Result<T, Error>;

/// A wrapper around an [`openssh::Session`]. Originally this struct was also supposed to contain
/// an [`openssh_sftp_client::Sftp`] session as well, but this struct is so poorly suited to our
/// use case that we opt to simply make calls into the `scp` command instead, since it allows us to
/// transfer either files or directories at will. Whilst currently there is no point of having a
/// struct with only a session inside of it, we may have more fields required here in the future.
#[derive(Debug)]
pub struct SessionWrapper {
    pub session: Arc<openssh::Session>,
}

// impl SessionWrapper {
//     fn new(session: openssh::Session) -> Self {
//         Self {
//             session: Arc::new(session),
//         }
//     }
// }

/// Runs a ShellCommand in parallel on all SSH sessions provided.
// pub async fn run_shell_command(
//     sessions: &Sessions,
//     shell_command: &ShellCommand,
// ) -> Result<()> {
//     // TODO(joren): Do something with the return Output type
//     common_async(sessions, remote_shell_command, shell_command.clone()).await?;
//     Ok(())
// }

/// Runs a ShellCommand in parallel in the background on all SSH sessions provided,
/// returning a handle to each of the background processes.
// pub async fn run_background_shell_command(
//     sessions: &Sessions,
//     shell_command: &ShellCommand,
// ) -> Result<BackgroundProcesses> {
//     common_async(
//         sessions,
//         remote_background_shell_command,
//         shell_command.clone(),
//     )
//     .await
// }

// impl<'a> Default for ShellCommand<'a> {
//     fn default() -> Self {
//         Self {
//             interpreter: INTERPRETER,
//             working_directory: None,
//             stdout_file: None,
//             stderr_file: None,
//             command: "".to_string(),
//             args:
//
//         }
//     }
// }

// TODO(joren): SSH also handles local commands,
// it might be a good idea to change the name of this enum to reflect that
/// The error type for SSH errors.
///
/// Errors are produced from nearly every function in this module, due to the nature of calling
/// subprocesses or handling SSH connections. These errors usually wrap an [`openssh::Error`], but
/// sometimes also handle tokio, I/O and subprocess errors.
///
/// All errors include additional context that better explains *what* happened to cause the error,
/// which isn't done too well by the [`openssh`] crate.
#[derive(Debug)]
pub enum Error {
    OpenSSHError(String, openssh::Error),
    OutputError(String, FromUtf8Error),
    JoinError(String, task::JoinError),
    IOError(String, io::Error),
    StateError(String),
    ProcessError(String, String, Option<i32>),
    CommandError(String, command::Error),
}

impl Error {
    fn display_openssh_error(
        f: &mut fmt::Formatter<'_>,
        message: &str,
        source: &openssh::Error,
    ) -> fmt::Result {
        write!(f, "{message}: ")?;
        match source {
            openssh::Error::Ssh(ssh_e) => {
                write!(f, "{ssh_e}")?;
            }
            openssh::Error::Remote(remote_e) => match remote_e.kind() {
                io::ErrorKind::NotFound => {
                    write!(f, "command not found")?;
                }
                _ => {
                    write!(f, "{remote_e}")?;
                }
            },
            _ => write!(f, "{source}")?,
        }
        Ok(())
    }

    fn display_process_error(
        f: &mut fmt::Formatter<'_>,
        message: &str,
        stderr: &str,
        code: Option<i32>,
    ) -> fmt::Result {
        let mut msg: String = format!("{message}: ");

        match code {
            Some(code) => {
                msg += &format!("process exited with status code '{}': ", code)
            }
            None => msg += "process terminated by signal: ",
        }

        if !stderr.is_empty() {
            msg += &format!("\nstderr: {stderr}")
        }

        let msg = msg.trim();
        write!(f, "{}", msg)?;

        Ok(())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::OpenSSHError(ref message, ref source) => {
                Self::display_openssh_error(f, message, source)?;
            }
            Self::OutputError(ref message, ref source) => {
                write!(f, "{message}: {source}")?;
            }
            Self::IOError(ref message, ref source) => {
                write!(f, "{message}: {source}")?;
            }
            Self::JoinError(ref message, ref source) => {
                write!(f, "{message}: {source}")?;
            }
            Self::StateError(ref message) => {
                write!(f, "{message}")?;
            }
            Self::ProcessError(ref message, ref stderr, code) => {
                Self::display_process_error(f, message, stderr, code)?;
            }
            Self::CommandError(ref message, ref source) => {
                write!(f, "{message}: {source}")?;
            }
        }

        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::OpenSSHError(.., ref source) => Some(source),
            Self::OutputError(.., ref source) => Some(source),
            Self::IOError(.., ref source) => Some(source),
            Self::JoinError(.., ref source) => Some(source),
            Self::CommandError(.., ref source) => Some(source),
            _ => None,
        }
    }
}

impl From<openssh::Error> for Error {
    fn from(value: openssh::Error) -> Self {
        Self::OpenSSHError("openssh library error".to_string(), value)
    }
}

impl From<(String, openssh::Error)> for Error {
    fn from(value: (String, openssh::Error)) -> Self {
        Self::OpenSSHError(value.0, value.1)
    }
}

impl From<task::JoinError> for Error {
    fn from(value: task::JoinError) -> Self {
        Self::JoinError("tokio task join error".to_string(), value)
    }
}

impl From<(String, FromUtf8Error)> for Error {
    fn from(value: (String, FromUtf8Error)) -> Self {
        Self::OutputError(value.0, value.1)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::IOError("IO Error".to_string(), value)
    }
}

impl From<(String, String, Option<i32>)> for Error {
    fn from(value: (String, String, Option<i32>)) -> Self {
        Self::ProcessError(value.0, value.1, value.2)
    }
}

// impl From<shlex::QuoteError> for Error {
//     fn from(value: shlex::QuoteError) -> Self {
//         Self::ShellError(format!("Failed to quote argument: {value}"))
//     }
// }

impl From<command::Error> for Error {
    fn from(value: command::Error) -> Self {
        Self::CommandError("Command error".to_string(), value)
    }
}

/// Establishes a persistent SSH connection to the hosts passed in.
/// Currently this function makes use only of key-based authentication, but mechanisms to take
/// advantage of passwords and custom SSH keys will come in the future.
pub async fn connect_to_hosts<S: AsRef<str>>(hosts: &[S]) -> Result<Sessions> {
    if hosts.is_empty() {
        Err(Error::StateError(
            "No hosts were present for connection".to_string(),
        ))?;
    }

    let mut sessions: Sessions = HashMap::new();
    for host in hosts.iter() {
        let session = match host.as_ref() {
            crate::LOCALHOST => Session::Local,
            _ => Session::Remote(Arc::new(
                openssh::Session::connect(host, KnownHosts::Strict).await?,
            )),
        };
        sessions.insert(host.as_ref().to_string(), session);
    }

    Ok(sessions)
}

/// Uploads a single file/directory to the destination directory for each of the sessions provided.
// pub async fn upload_one<P: AsRef<Path>>(
//     sessions: &Sessions,
//     source_path: P,
//     destination_path: P,
// ) -> Result<()> {
//     upload(sessions, &[source_path], destination_path).await
// }

/// Uploads a list of files/directories to the destination directory for each of the sessions
/// provided.
pub async fn upload<P: AsRef<Path>>(
    sessions: &Sessions,
    source_paths: &[P],
    destination_path: P,
) -> Result<()> {
    let mut has_dir = false;
    for source_path in source_paths.iter() {
        let source_path_ref = source_path.as_ref();
        if !source_path_ref.exists() {
            Err(Error::StateError(format!(
                "Error during SSH upload: '{}' does not exist",
                source_path_ref.display()
            )))?;
        }

        if source_path_ref.is_dir() {
            has_dir = true;
        }
    }

    let mut command_args = vec!["scp".to_string()];
    if has_dir {
        command_args.push("-r".to_string());
    }

    for source_path in source_paths {
        command_args.push(source_path.as_ref().to_string_lossy().to_string());
    }

    for (host, _) in sessions.iter() {
        let command_args_w_host: Vec<String> = command_args
            .iter()
            .cloned()
            .chain([format!(
                "{host}:{}",
                destination_path.as_ref().to_string_lossy()
            )])
            .collect();

        let shell_command =
            ShellCommand::from_command_args(&command_args_w_host);

        command::run_local_command(shell_command, ExecutionType::Status)
            .await?;
    }

    Ok(())
}

// async fn do_scp(
//     host: String,
//     _: Session,
//     params: (Vec<String>, PathBuf),
// ) -> Result<std::process::Output> {
//     let (mut command, destination_path) = params;
//     command.push(format!("{host}:{}", destination_path.to_string_lossy()));
//
//     let output = tokio::process::Command::new(command.first().unwrap())
//         .args(&command[1..])
//         .output()
//         .await
//         .map_err(Error::from)?;
//
//     if !output.status.success() {
//         let status_code = output.status.code();
//         let stderr = String::from_utf8_lossy(&output.stderr).to_string();
//         let msg = format!("Failed to scp upload to host {host}");
//         Err((msg, stderr, status_code))?;
//     }
//
//     Ok(output)
// }
//
/// Used to download a directory from each remote host into a local experiment results directory.
pub async fn download<P: AsRef<Path>>(
    sessions: &Sessions,
    remote_paths: &[P],
    local_destination: P,
) -> Result<()> {
    let command_args = ["scp".into(), "-r".into()];

    for (host, _) in sessions.iter() {
        let mut command_args = command_args.to_vec();
        let local_destination_dir = local_destination.as_ref().join(host);
        std::fs::create_dir_all(&local_destination_dir).map_err(Error::from)?;

        for rp in remote_paths.iter() {
            command_args
                .push(format!("{host}:{}/*", rp.as_ref().to_string_lossy()));
        }
        command_args.push(local_destination_dir.to_string_lossy().to_string());

        let shell_command = ShellCommand::from_command_args(&command_args);
        command::run_local_command(shell_command, ExecutionType::Status)
            .await?;
    }

    Ok(())
}

// TODO(jackson): maybe just make the do_scp function work for upload and download?
// async fn do_scp_download(
//     host: String,
//     _: Session,
//     params: (Vec<String>, PathBuf),
// ) -> Result<std::process::Output> {
//     let (remote_paths, local_destination) = params;
//
//     // Ensure the local destination directory exists for this host
//     let local_destination_dir = local_destination.join(&host);
//     std::fs::create_dir_all(&local_destination_dir).map_err(Error::from)?;
//
//     let mut command: Vec<String> = vec!["scp".into(), "-r".into()];
//     for rp in remote_paths.iter() {
//         // Add /* to copy contents of directory, not the directory itself
//         command.push(format!("{host}:{}/*", rp));
//     }
//     command.push(local_destination_dir.to_string_lossy().to_string());
//
//     let output = tokio::process::Command::new(command.first().unwrap())
//         .args(&command[1..])
//         .output()
//         .await
//         .map_err(Error::from)?;
//
//     if !output.status.success() {
//         let status_code = output.status.code();
//         let stderr = String::from_utf8_lossy(&output.stderr).to_string();
//         let msg = format!(
//             "Failed to scp download from host {host} -> {}",
//             local_destination_dir.to_string_lossy()
//         );
//         Err((msg, stderr, status_code))?;
//     }
//
//     Ok(output)
// }

// TODO(joren): Change result so that it links to the started process
/// Runs a command in parallel in the background on all SSH sessions provided, returning a handle
/// to each of the processes run in this way.
// pub async fn run_background_command<S: AsRef<str>>(
//     sessions: &Sessions,
//     command_args: &[S],
// ) -> Result<BackgroundProcesses> {
//     command_check(command_args)?;
//
//     let shell_command = ShellCommand::from_command_args(command_args);
//     common_async(sessions, remote_background_shell_command, shell_command).await
// }

/// Runs a command in parallel in the background on all SSH sessions provided, with the current
/// working directory set to a given path, returning a handle to each of the processes run in this
/// way.
// pub async fn run_background_command_at<S: AsRef<str>, P: AsRef<Path>>(
//     sessions: &Sessions,
//     command_args: &[S],
//     directory: P,
// ) -> Result<BackgroundProcesses> {
//     let mut shell_command = ShellCommand::from_command_args(command_args);
//     shell_command.working_directory(directory);
//     common_async(sessions, remote_background_shell_command, shell_command).await
// }

// Previously used to emulate working directories before `ShellCommand` supported `working_directory`.
// Kept for clarity; replace call sites with `ShellCommand::working_directory`.
#[allow(dead_code)]
fn cd_command<P: AsRef<Path>>(path: P) -> [String; 3] {
    [
        "cd".into(),
        path.as_ref().to_string_lossy().into(),
        "&&".into(),
    ]
}

/// Runs a command in parallel on all SSH sessions provided.

/// Runs a command in parallel on all SSH sessions provided, with the current working directory
/// set to a given path.
// pub async fn run_command_at<S: AsRef<str>, P: AsRef<Path>>(
//     sessions: &Sessions,
//     command_args: &[S],
//     directory: P,
// ) -> Result<()> {
//     let mut shell_command = ShellCommand::from_command_args(command_args);
//     shell_command.working_directory(directory);
//     common_async(sessions, remote_shell_command, shell_command).await?;
//     Ok(())
// }

// fn command_check<S: AsRef<str>>(command: &[S]) -> Result<()> {
//     if command.is_empty() {
//         Err(Error::StateError(
//             "No command was supplied when trying to run a command remotely"
//                 .to_string(),
//         ))?;
//     }
//     Ok(())
// }

// async fn remote_shell_command(
//     host: String,
//     session: Session,
//     shell_command: ShellCommand,
// ) -> Result<()> {
//     let err_str = format!(
//         "Failed to run program '{}' on host {host}",
//         shell_command.command
//     );
//
//     let mut owned_command = shell_command.build_remote_command(&session)?;
//     match owned_command.output().await {
//         Ok(output) => {
//             if !output.status.success() {
//                 let stderr = String::from_utf8(output.stderr).map_err(|e| {
//                     Error::OutputError(
//                         format!(
//                             "Failed to convert stderr bytes to UTF-8 for program '{}' on host '{host}'",
//                             shell_command.command
//                         ),
//                         e,
//                     )
//                 })?;
//                 Err((err_str, stderr, output.status.code()))?;
//             }
//         }
//         Err(e) => {
//             Err((err_str, e))?;
//         }
//     }
//     Ok(())
// }

// async fn remote_background_shell_command(
//     host: String,
//     session: Session,
//     shell_command: ShellCommand,
// ) -> Result<BackgroundProcess> {
//     let err_str = format!(
//         "Failed to run background program '{}' on host '{host}'",
//         shell_command.command
//     );
//
//     shell_command
//         .build_remote_arc_command(&session)?
//         .spawn()
//         .await
//         .map_err(|e| Error::from((err_str, e)))
// }

/// Makes multiple directories at once in parallel across the sessions passed in.
/// This means if 8 directories need to be made on 3 hosts,
/// all 3 hosts will run the 'mkdir -p {DIRECTORY1..DIRECTORY8}' command in parallel.
pub async fn make_directories<P: AsRef<Path>>(
    sessions: &Sessions,
    paths: &[P],
) -> Result<()> {
    let command_args: Vec<String> = ["mkdir".to_string(), "-p".into()]
        .into_iter()
        .chain(
            paths
                .iter()
                .map(|path| path.as_ref().to_string_lossy().to_string()),
        )
        .collect();

    let shell_command = ShellCommand::from_command_args(&command_args);

    command::run_command(sessions, shell_command, ExecutionType::Status)
        .await?;

    Ok(())
}

/// Creates a directory on multiple remote hosts in parallel using the mkdir -p command.
/// It's best to ensure that absolute paths are passed into this function.
pub async fn make_directory<P: AsRef<Path>>(
    sessions: &Sessions,
    path: P,
) -> Result<()> {
    let command_args = [
        "mkdir".to_string(),
        "-p".into(),
        path.as_ref().to_string_lossy().to_string(),
    ];

    let shell_command = ShellCommand::from_command_args(&command_args);
    command::run_command(sessions, shell_command, ExecutionType::Status)
        .await?;

    Ok(())
}

/// This function does a lot of heavy lifting when we make remote requests.
/// Since all of our SSH functionality requires reaching out to multiple hosts
/// simultaneously, we need to generate asynchronous tasks that can run in parallel.
/// We also need flexibility, hence the use of a generic function.
/// Generally we want to return something from our function, but it can be (), too.
///
/// Some important notes about using this function.
/// Make sure that owned types are sent in; Don't use a String slice, use a Vector of Strings.
// async fn common_async<T, P, Fut>(
//     sessions: &Sessions,
//     task_fn: impl Fn(String, Arc<SessionWrapper>, P) -> Fut + Send + 'static + Copy,
//     params: P,
// ) -> Result<Vec<T>>
// where
//     T: Send + 'static,
//     Fut: Future<Output = Result<T>> + Send + 'static,
//     P: Clone + Send + 'static,
// {
//     if sessions.is_empty() {
//         Err(Error::StateError(
//             "No sessions were present when trying to run a command".to_string(),
//         ))?;
//     }
//
//     // let mut tasks: Vec<JoinHandle<BoxFuture<'static, Result<T>>>> =
//     //     Vec::with_capacity(sessions.len());
//     let mut tasks = Vec::with_capacity(sessions.len());
//     for (host, session) in sessions.iter() {
//         // These two variables are always available for the async block
//         let host_c = host.clone();
//         let session_c = session.clone();
//         let params_c = params.clone();
//
//         tasks.push(task::spawn(async move {
//             Box::pin(task_fn(host_c, session_c, params_c)).await
//         }));
//     }
//
//     let mut results = vec![];
//     for task in tasks.into_iter() {
//         results.push(task.await??);
//     }
//
//     Ok(results)
// }

// Note that the script in question is run remotely,
// so it must already exist on the target sessions
/// Executes a script in a bash shell in parallel on all the SSH sessions provided.
// pub async fn run_script(sessions: &Sessions, script: &Path) -> Result<()> {
//     let shell_command = ShellCommand::from_command_args(&[script
//         .to_string_lossy()
//         .to_string()]);
//     common_async(sessions, remote_shell_command, shell_command).await?;
//     Ok(())
// }

/// Executes a script in a bash shell in parallel on all the SSH sessions provided, with the
/// current working directory set to the given path.
pub async fn run_script_at<P: AsRef<Path>, R: AsRef<Path>>(
    sessions: &Sessions,
    script: P,
    directory: R,
) -> Result<()> {
    let mut shell_command = ShellCommand::from_command_args(&[script
        .as_ref()
        .to_string_lossy()
        .to_string()]);
    shell_command.working_directory(directory);
    command::run_command(sessions, shell_command, ExecutionType::Output)
        .await?;
    Ok(())
}

// ----- DANIEL'S IMPLEMENTATION OF SSH FUNCTIONALITY. KEEP THIS AROUND FOR REFERENCE AND -----
// ----- INSPIRATION. BE SURE TO ADD BACK IN THE TESTS WHEN EVERYTHING IS HUNKY DORY. -----
// Connects to a list of target hosts.
// pub async fn connect_to_group(
//     hosts: &Vec<String>,
// ) -> std::result::Result<(SessionMap, SFTPMap), AnyError> {
//     if hosts.is_empty() {
//         Err(NoTargetError)?
//     }
//     let mut group: SessionMap = HashMap::new();
//     let mut sftp_group: SFTPMap = HashMap::new();
//     // Make both the normal sessions and SFTP ones.
//     for host in hosts {
//         let s = Arc::new(Session::connect(host, KnownHosts::Strict).await?);
//         // TODO: Can't find a way to just use the same session for both or clone it, but would be
//         // better
//         let sftp_s = Arc::new(
//             Sftp::from_session(
//                 Session::connect(host, KnownHosts::Strict).await?,
//                 SftpOptions::new(),
//             )
//             .await?,
//         );
//         group.insert(host.to_owned(), s);
//         sftp_group.insert(host.to_owned(), sftp_s);
//     }
//     // Should never fail since we don't accept an empty host list and failures to connect should
//     // exit early. Validate to make sure this holds.
//     assert!(!group.is_empty());
//     assert!(!sftp_group.is_empty());
//     Ok((group, sftp_group))
// }
//
// // pub async fn basic_group_command(
// //
// // )
//
// // Runs command simultaneously on each specified host.
// /// Note: this currently doesn't crash if the command fails, and prints the stderr instead.
// /// It could be changed to panic on command failure, but some programs print to stderr deliberately
// /// (e.g. time).
// pub async fn run_remote_command_on_group<F>(
//     cmd: &String,
//     group: &SessionMap,
//     args: &Option<Vec<String>>,
//     custom_args_function: Option<F>,
// ) -> std::result::Result<(), AnyError>
// where
//     F: Fn(&String) -> Vec<String>,
// {
//     if group.is_empty() {
//         Err(NoTargetError)?
//     }
//     // TODO: The openssh crate doesn't seem to provide a way to run these
//     // async directly, but could look for a better way regardless.
//     let mut tasks = Vec::new();
//     for (host, s) in group.iter() {
//         // Each thread needs its own instance of the input data.
//         let host_c = host.clone();
//         let s_c = s.clone();
//         let cmd_c = cmd.clone();
//         let args_c = args.clone();
//
//         // Run the closure here so we don't need to move the closure into the thread, just the
//         // results. This allows us to dynamically generate args based off hostname where required.
//         let fargs_c = match custom_args_function {
//             Some(ref f) => Some(f(&host_c)),
//             None => None,
//         };
//         tasks.push(task::spawn(async move {
//             // Creates an OwningCommand you can add args to.
//             let mut own_cmd = s_c.command(&cmd_c);
//
//             // TODO: Add some arg validation
//             if let Some(arg_vals) = args_c {
//                 println!("Added args {:?}", arg_vals);
//                 own_cmd.args(arg_vals);
//             }
//
//             if let Some(fargs) = fargs_c {
//                 println!("Added args {:?}", fargs);
//                 own_cmd.args(fargs);
//             }
//             // Run and collect stdout/stderr
//             match own_cmd.output().await {
//                 Ok(out) => {
//                     let out_str = match String::from_utf8(out.stderr) {
//                         Ok(x) => x,
//                         Err(_) => panic!(
//                             "Failed to convert output for command {} on host {} to string.",
//                             cmd_c,
//                             host_c
//                         ),
//                     };
//                     println!("Output is {} from host {}", out_str, host_c);
//                 }
//                 Err(e) => {
//                     panic!(
//                         "Output failed for host {} with error: {}",
//                         host_c, e
//                     );
//                 }
//             };
//         }))
//     }
//
//     for task in tasks {
//         task.await?
//     }
//     Ok(())
// }
//
// // Runs command simultaneously on each specified host.
// pub async fn run_local_command_on_group<F>(
//     cmd: &String,
//     group: &SessionMap,
//     args: &Option<Vec<String>>,
//     custom_args_function: Option<F>,
// ) -> std::result::Result<(), AnyError>
// where
//     F: Fn(&String) -> Vec<String>,
// {
//     if group.is_empty() {
//         Err(NoTargetError)?
//     }
//     let mut tasks = Vec::new();
//     for (host, _) in group.iter() {
//         // TODO: Probably don't need to clone these
//         let host_c = host.clone();
//         let cmd_c = cmd.clone();
//         let args_c = args.clone();
//         // This allows us to dynamically generate args based off hostname where required.
//         let fargs_c = match custom_args_function {
//             Some(ref f) => Some(f(&host_c)),
//             None => None,
//         };
//         let merged_args = match(args_c, fargs_c) {
//             (Some(mut a1), Some(a2)) => {
//                 a1.extend(a2);
//                 Some(a1)
//             },
//             (Some(a1), None) | (None, Some(a1)) => Some(a1),
//             (None, None) => None
//         };
//         tasks.push(run_local_command_async(&cmd_c, &merged_args));
//     }
//
//     for task in tasks {
//         task.await?
//     }
//     Ok(())
// }
//
// /// Starts an async command on the local host and returns the task handle.
// fn run_local_command_async(cmd: &String, args: &Option<Vec<String>>) -> JoinHandle<()> {
//     let cmd_c = cmd.clone();
//     let args_c = args.clone();
//     task::spawn(async move {
//         let mut cmd_build = Command::new(&cmd_c);
//
//         // TODO: Add some arg validation
//         if let Some(arg_vals) = &args_c {
//             println!("Run added args {:?}", arg_vals);
//             cmd_build.args(arg_vals);
//         }
//
//         // Run and collect stdout/stderr
//         match cmd_build.output() {
//             Ok(out) => {
//                 if out.status.success() {
//                     return;
//                 }
//                 let out_str = match String::from_utf8(out.stderr) {
//                     Ok(x) => x,
//                     Err(_) => panic!(
//                         "Failed to convert output for command {} on host {:?} to string.",
//                         &cmd_c,
//                        &args_c
//                     ),
//                 };
//                 let status_msg = if let Some(status) = out.status.code() {
//                     status.to_string()
//                 } else {
//                     "(signal terminated)".to_string()
//             };
//             panic!("Command {} failed with status {:?}; stderr is {}",&cmd_c, status_msg, out_str);
//             }
//             Err(e) => {
//                 panic!(
//                     "Output failed for local command {} with error: {}",&cmd_c, e
//                 );
//             }
//         };
//     })
// }
// /// Consume the groups to ensure they aren't used after closure.
// pub async fn close_group_sessions(group: SessionMap) -> std::result::Result<(), Error> {
//     for (_, s) in group {
//         Arc::into_inner(s).unwrap().close().await?
//     }
//     Ok(())
// }
//
// /// Returns a Dir object from a directory string, or an error if it fails.
// pub async fn get_remote_dir_obj(s: &Sftp, dpath: &str) -> std::result::Result<Dir, AnyError> {
//     Ok(s.fs().open_dir(dpath).await?)
// }
//
// /// Prints all files in a directory.
// pub async fn read_remote_dir(dir: &Dir) {
//     let read_dir = dir.clone().read_dir();
//     // TODO: Consumes, but would rather it not. Can't find a way to do that though.
//     let dir_map = read_dir.map(|item| -> String {
//         item.unwrap()
//             .filename()
//             .to_str()
//             .expect("Should be value")
//             .to_string()
//             .clone()
//     });
//     tokio::pin!(dir_map);
//     while let Some(entry) = dir_map.next().await {
//         println!("Entry found in dir {:?}", entry);
//     }
// }
//
// /// For some reason, the sftp library doesn't support
// /// copying directories remote->local. Just execute SCP
// /// Format scp -r
// pub async fn retrieve_dir_all_remotes(
//     s: &SessionMap,
//     remote_path: &String, // e.g. (Remote) /srv/data
//     local_path: &String, // e.g. (Local) ~/results
// ) -> std::result::Result<(), AnyError> {
//     let ex = fs::exists(local_path)?;
//     if !ex {
//         match fs::create_dir_all(local_path) {
//             Ok(_) => {},
//             Err(e) => eprintln!("Failed to create dir {} (may already exist): {e}", local_path),
//         }
//     }
//     local_scp_over_group(s,
//         move |host: &String| -> Vec<String> {
//             let full_remote = format!("{}:{}", host.clone(), remote_path.clone());
//             let full_local = format!("{}/{}", local_path.clone(),host.clone(),);
//             vec![full_remote.clone(), full_local.clone()]
//         },
//     )
//     .await?;
//     Ok(())
// }
//
// /// Runs scp -r on the local host, across each target in the group.
// /// This function takes a closure 'f' returning a Vec; from this vec 'closure_vec', the command will run:
// ///     scp -r closure_vec[0] closure_vec[1]
// pub async fn local_scp_over_group<F>(
//     s: &SessionMap,
//     f: F
// ) -> std::result::Result<(), AnyError>
// where
//     F: Fn(&String) -> Vec<String>,
// {
//     let scp_cmd = String::from("scp");
//     let args = vec!["-r".to_string()];
//     run_local_command_on_group(
//         &scp_cmd,
//         &s,
//         &Some(args),
//         Some(f),
//     )
//     .await?;
//     Ok(())
// }
//
//
// pub async fn upload_dir_all_remotes(
//     s: &SessionMap,
//     local_path: &String, // e.g. (Local) ~/results
//     remote_path: &String, // e.g. (Remote) /srv/data
// ) -> std::result::Result<(), AnyError> {
//     // Check the directory to upload exists
//     fs::exists(local_path)?;
//     local_scp_over_group(s,
//         move |host: &String| -> Vec<String> {
//             let full_remote = format!("{}:{}", host.clone(), remote_path.clone());
//             vec![local_path.clone(), full_remote.clone()]
//         },
//     )
//     .await?;
//     Ok(())
// }
//
// pub async fn delete_dir_all_remotes(s: &SessionMap, remote_path: &String) -> std::result::Result<(),AnyError> {
//     run_remote_command_on_group::<fn(&String) -> Vec<String>>(&String::from("rm"), &s, &Some(vec!["-r".to_string(), remote_path.clone()]), None).await
// }
//
// pub async fn dir_exists_all_remotes(s: &SFTPMap, remote_path: &String) -> bool {
//     for (host, ss) in s.iter() {
//         let  cur_fs = ss.fs();
//         let pwd = cur_fs.cwd();
//         println!("Host {} currently at {:?}", host, pwd);
//         if get_remote_dir_obj(&ss, &remote_path).await.is_err() {
//             return false;
//         }
//     }
//     true
// }
