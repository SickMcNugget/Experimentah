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
