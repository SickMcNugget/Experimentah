// use futures_core::stream::Stream;
use futures_util::StreamExt;
use log::info;
use openssh::{Error, KnownHosts, Session};
use openssh_sftp_client::{
    fs::Dir,
    Sftp, SftpOptions,
};
use std::any::Any;
use std::path::{Path, PathBuf};
use std::string::FromUtf8Error;
use std::{collections::HashMap, fs};
use std::{fmt, io};
use std::process::Command;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};

// Bit ugly, but we need separate maps for the normal Sessions and SFTP ones.
type SessionMap = HashMap<String, Arc<Session>>;
type SFTPMap = HashMap<String, Arc<Sftp>>;
// type AnyError = Box<dyn std::error::Error>;
pub type Sessions = HashMap<String, Arc<SFTPSession>>;
type AnyError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub type Result<T> = std::result::Result<T, SSHError>;

#[derive(Debug)]
pub struct SFTPSession {
    session: Session,
    sftp: Sftp
}

impl SFTPSession {
    fn new(session: Session, sftp: Sftp) -> Self {
        Self {
            session,
            sftp
        }
    }
}

// TODO(joren): SSH also handles local commands,
// it might be a good idea to change the name of this enum to reflect that
#[derive(Debug)]
pub enum SSHError {
    OpenSSHError(String, openssh::Error),
    OpenSSHSFTPError(String, openssh_sftp_client::Error),
    OutputError(String, FromUtf8Error),
    JoinError(String, task::JoinError),
    IOError(String, io::Error),
    StateError(String)
}

impl SSHError {
    fn display_openssh_error(f: &mut fmt::Formatter<'_>, message: &str, source: &openssh::Error) -> fmt::Result {
        write!(f, "{message}: ")?;
        match source {
            openssh::Error::Ssh(ssh_e) => {
                write!(f, "{ssh_e}")?;
            },
            openssh::Error::Remote(remote_e) => {
                match remote_e.kind() {
                    io::ErrorKind::NotFound => {
                        write!(f, "command not found")?;
                    },
                    _ => {
                        write!(f, "{remote_e}")?;
                    }
                }
            }
            _ => write!(f, "{source}")?
        }
        Ok(())
    }
}

impl fmt::Display for SSHError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::OpenSSHError(ref message, ref source) => {
                Self::display_openssh_error(f, message, source)?;
            },
            Self::OpenSSHSFTPError(ref message, ref source) => {
                write!(f, "{message}: {source}")?;
            },
            Self::OutputError(ref message, ref source) => {
                write!(f, "{message}: {source}")?;
            },
            Self::IOError(ref message, ref source) => {
                write!(f, "{message}: {source}")?;
            },
            Self::JoinError(ref message, ref source) => {
                write!(f, "{message}: {source}")?;
            }
            Self::StateError(ref message) => {
                write!(f, "{message}")?;
            }
        }

        Ok(())
    }
}

impl std::error::Error for SSHError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::OpenSSHError(.., ref source) => {
                Some(source)
            },
            Self::OpenSSHSFTPError(.., ref source) => {
                Some(source)
            },
            Self::OutputError(.., ref source) => {
                Some(source)
            },
            Self::IOError(.., ref source) => {
                Some(source)
            },
            Self::JoinError(.., ref source) => {
                Some(source)
            },
            _ => {
                None
            }
        }
    }

}

impl From<openssh::Error> for SSHError {
    fn from(value: openssh::Error) -> Self {
        Self::OpenSSHError("openssh library error".to_string(), value)
    }
}

impl From<(String, openssh::Error)> for SSHError {
    fn from(value: (String, openssh::Error)) -> Self {
        Self::OpenSSHError(value.0, value.1)
    }
}

impl From<openssh_sftp_client::Error> for SSHError {
    fn from(value: openssh_sftp_client::Error) -> Self {
        Self::OpenSSHSFTPError("openssh sftp library error".to_string(),value)
    }
}

impl From<task::JoinError> for SSHError {
    fn from(value: task::JoinError) -> Self {
        Self::JoinError("tokio task join error".to_string(),value)
    }
}

impl From<(String, FromUtf8Error)> for SSHError {
    fn from(value: (String, FromUtf8Error)) -> Self {
        Self::OutputError(value.0, value.1)
    }
}

impl From<io::Error> for SSHError {
    fn from(value: io::Error) -> Self {
        Self::IOError("IO Error".to_string(), value)
    }
}

#[derive(Debug, Clone)]
struct NoTargetError;
impl std::error::Error for NoTargetError {}
impl fmt::Display for NoTargetError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Empty Targets List is disallowed (must be at least one valid host)")
    }
}

pub async fn connect_to_hosts(
    hosts: &[String]
) -> Result<Sessions> {
    if hosts.is_empty() {
        Err(SSHError::StateError("No hosts were present for connection".to_string()))?;
    }

    let mut sessions: Sessions = HashMap::new();
    for host in hosts.iter() {
        let session = Session::connect(host, KnownHosts::Strict).await?;
        let sftp = Sftp::from_session(Session::connect(host, KnownHosts::Strict).await?, SftpOptions::new()).await?;
        sessions.insert(host.to_string(), Arc::new(SFTPSession::new(session, sftp)));
    }

    Ok(sessions)
}

// Since the Sftp library we use just sucks,
// we use simple subprocess commands instead.
pub async fn upload(
    sessions: &Sessions,
    source_path: &Path,
    destination_path:&Path 
) -> Result<()>
{
    if sessions.is_empty() {
        Err(SSHError::StateError("No sessions were present when trying to run a command".to_string()))?;
    } else if !source_path.exists() {
        Err(SSHError::StateError("The source path to upload does not exist".to_string()))?;
    }

    let mut command = vec!["scp".to_string()];
    if source_path.is_dir() {
        command.push("-r".to_string());
    }
    command.push(source_path.to_string_lossy().to_string());

    let mut tasks = Vec::with_capacity(sessions.len());
    for (host, _) in sessions.iter() {
        let mut command_c = command.to_vec();
        command_c.push(format!("{host}:{}", destination_path.to_string_lossy()));

        tasks.push(tokio::process::Command::new(command_c.first().unwrap()).args(&command_c[1..]).output());
    }

    for task in tasks {
        let output = task.await?;
        // println!("{}", String::from_utf8(output.stdout).unwrap());
    }

    Ok(())
}

pub async fn run_command_at(
    sessions: &Sessions,
    command_args: &[String],
    directory: &Path,
) -> Result<()> {
    let cd_command = vec!["cd".to_string(), directory.to_string_lossy().to_string(), "&&".to_string()];
    let command_args: Vec<String> = cd_command.iter().chain(command_args).cloned().collect();
    run_command(sessions, &command_args).await
}

pub async fn run_command(
    sessions: &Sessions,
    command_args: &[String]
) -> Result<()> {
    if sessions.is_empty() {
        Err(SSHError::StateError("No sessions were present when trying to run a command".to_string()))?;
    } else if command_args.is_empty() {
        Err(SSHError::StateError("No command was supplied when trying to run a command remotely".to_string()))?;
    }

    let mut tasks: Vec<JoinHandle<Result<()>>> = Vec::with_capacity(sessions.len());
    for (host, session) in sessions.iter() {

        let host_c = host.clone();
        let session_c = session.clone();

        // We assume the use of bash
        // TODO(joren): Add "sh" downgrade functionality
        let interpreter = "bash";
        // let command_c = command_args.first().unwrap().clone();
        // let args_c = command_args[1..].to_vec();
        // Since we run our command through bash, we should treat it as
        // one single argument so that bash can do the splitting
        let command_c = command_args.first().unwrap().clone();
        let command_args_c = command_args.join(" ");

        tasks.push(task::spawn(async move {
            let mut s_command = session_c.session.command(interpreter);
            s_command.arg("-c");

            // let mut s_command = session_c.session.command(&command_c);
            s_command.arg(&command_args_c);

            match s_command.output().await {
                Ok(output) => {
                    // format!("Failed to convert output for command '{command_c}' on host '{host_c}' to string.").as_str()
                    let stdout = String::from_utf8(output.stdout).map_err(|e| {
                        SSHError::OutputError(format!("Failed to convert stdout bytes to UTF-8 for command '{command_c}' on host '{host_c}'"), e)
                    })?;
                    let stderr = String::from_utf8(output.stderr).map_err(|e| {
                        SSHError::OutputError(format!("Failed to convert stderr bytes to UTF-8 for command '{command_c}' on host '{host_c}'"), e)
                    })?;

                    println!("'{host_c}'\nstdout\n{stdout}\nstderr\n{stderr}");
                }
                Err(e) => {
                    Err((format!("Failed to run command '{command_args_c}' on host {host_c}"),e))?;
                    // handle_openssh_error(&e);
                    // panic!("Failed to run command {:?} on host {host_c}", std::iter::once(&command_c).chain(&args_c).collect::<Vec<&String>>());
                }
            };
            Ok(())
        }));
    }

    // We have our JoinError and also an internal SSHError
    for task in tasks.into_iter() {
        task.await??;
    }
    Ok(())
}

pub async fn make_directory(sessions: &Sessions, path: &Path) -> Result<()> {
    let path = path.to_string_lossy();
        run_command(
            sessions,
            &["mkdir".into(), "-p".into(), path.to_string()],
        )
        .await
}

fn handle_openssh_error(e: &openssh::Error) {
    match e {
        openssh::Error::Remote(remote_e) => {
            eprintln!("Error on remote occurred during SSH - {}: {e}", remote_e.kind());
        },
        _ => eprintln!("{e}")
    }

}

/// Note that the script in question is run remotely,
/// so it must already exist on the target sessions
pub async fn run_script(
    sessions: &Sessions,
    script: &PathBuf
) -> Result<()> {
    if sessions.is_empty() {
        Err(SSHError::StateError("No sessions were present when trying to run a command".to_string()))?;
    }

    let mut tasks: Vec<JoinHandle<Result<()>>> = Vec::with_capacity(sessions.len());
    for (host, session) in sessions.iter() {

        let host_c = host.clone();
        let session_c = session.clone();
        let script_c = script.clone();

        tasks.push(task::spawn(async move {
            let mut s_command = session_c.session.command("bash");
            let script_c_str = script_c.to_string_lossy();
            s_command.arg(&script_c_str);

            match s_command.output().await {
                Ok(output) => {
                    // format!("Failed to convert output for command '{command_c}' on host '{host_c}' to string.").as_str()
                    let stdout = String::from_utf8(output.stdout).map_err(|e| {
                        SSHError::OutputError(format!("Failed to convert stdout bytes to UTF-8 for script '{script_c_str}' on host '{host_c}'"), e)
                    })?;
                    let stderr = String::from_utf8(output.stderr).map_err(|e| {
                        SSHError::OutputError(format!("Failed to convert stderr bytes to UTF-8 for script '{script_c_str}' on host '{host_c}'"), e)
                    })?;

                    info!("Successfully ran script '{}' on '{host_c}'", script_c.file_name().unwrap().to_string_lossy());
                    // info!("Command success on '{host_c}'");
                    // println!("'{host_c}'\nstdout\n{stdout}\nstderr\n{stderr}");
                }
                Err(e) => {
                    panic!( "Output failed for host {host_c} with error: {e}");
                }
            };
            Ok(())
        }));
    }

    // We have our JoinError and also an internal SSHError
    for task in tasks.into_iter() {
        task.await??;
    }
    Ok(())
}


/// Connects to a list of target hosts.
pub async fn connect_to_group(
    hosts: &Vec<String>,
) -> std::result::Result<(SessionMap, SFTPMap), AnyError> {
    if hosts.is_empty() {
        Err(NoTargetError)?
    }
    let mut group: SessionMap = HashMap::new();
    let mut sftp_group: SFTPMap = HashMap::new();
    // Make both the normal sessions and SFTP ones.
    for host in hosts {
        let s = Arc::new(Session::connect(host, KnownHosts::Strict).await?);
        // TODO: Can't find a way to just use the same session for both or clone it, but would be
        // better
        let sftp_s = Arc::new(
            Sftp::from_session(
                Session::connect(host, KnownHosts::Strict).await?,
                SftpOptions::new(),
            )
            .await?,
        );
        group.insert(host.to_owned(), s);
        sftp_group.insert(host.to_owned(), sftp_s);
    }
    // Should never fail since we don't accept an empty host list and failures to connect should
    // exit early. Validate to make sure this holds.
    assert!(!group.is_empty());
    assert!(!sftp_group.is_empty());
    Ok((group, sftp_group))
}

// pub async fn basic_group_command(
//
// )

// Runs command simultaneously on each specified host.
/// Note: this currently doesn't crash if the command fails, and prints the stderr instead.
/// It could be changed to panic on command failure, but some programs print to stderr deliberately
/// (e.g. time).
pub async fn run_remote_command_on_group<F>(
    cmd: &String,
    group: &SessionMap,
    args: &Option<Vec<String>>,
    custom_args_function: Option<F>,
) -> std::result::Result<(), AnyError>
where
    F: Fn(&String) -> Vec<String>,
{
    if group.is_empty() {
        Err(NoTargetError)?
    }
    // TODO: The openssh crate doesn't seem to provide a way to run these
    // async directly, but could look for a better way regardless.
    let mut tasks = Vec::new();
    for (host, s) in group.iter() {
        // Each thread needs its own instance of the input data.
        let host_c = host.clone();
        let s_c = s.clone();
        let cmd_c = cmd.clone();
        let args_c = args.clone();

        // Run the closure here so we don't need to move the closure into the thread, just the
        // results. This allows us to dynamically generate args based off hostname where required.
        let fargs_c = match custom_args_function {
            Some(ref f) => Some(f(&host_c)),
            None => None,
        };
        tasks.push(task::spawn(async move {
            // Creates an OwningCommand you can add args to.
            let mut own_cmd = s_c.command(&cmd_c);

            // TODO: Add some arg validation
            if let Some(arg_vals) = args_c {
                println!("Added args {:?}", arg_vals);
                own_cmd.args(arg_vals);
            }

            if let Some(fargs) = fargs_c {
                println!("Added args {:?}", fargs);
                own_cmd.args(fargs);
            }
            // Run and collect stdout/stderr
            match own_cmd.output().await {
                Ok(out) => {
                    let out_str = match String::from_utf8(out.stderr) {
                        Ok(x) => x,
                        Err(_) => panic!(
                            "Failed to convert output for command {} on host {} to string.",
                            cmd_c,
                            host_c
                        ),
                    };
                    println!("Output is {} from host {}", out_str, host_c);
                }
                Err(e) => {
                    panic!(
                        "Output failed for host {} with error: {}",
                        host_c, e
                    );
                }
            };
        }))
    }

    for task in tasks {
        task.await?
    }
    Ok(())
}

// Runs command simultaneously on each specified host.
pub async fn run_local_command_on_group<F>(
    cmd: &String,
    group: &SessionMap,
    args: &Option<Vec<String>>,
    custom_args_function: Option<F>,
) -> std::result::Result<(), AnyError>
where
    F: Fn(&String) -> Vec<String>,
{
    if group.is_empty() {
        Err(NoTargetError)?
    }
    let mut tasks = Vec::new();
    for (host, _) in group.iter() {
        // TODO: Probably don't need to clone these
        let host_c = host.clone();
        let cmd_c = cmd.clone();
        let args_c = args.clone();
        // This allows us to dynamically generate args based off hostname where required.
        let fargs_c = match custom_args_function {
            Some(ref f) => Some(f(&host_c)),
            None => None,
        };
        let merged_args = match(args_c, fargs_c) { 
            (Some(mut a1), Some(a2)) => { 
                a1.extend(a2); 
                Some(a1)
            },
            (Some(a1), None) | (None, Some(a1)) => Some(a1), 
            (None, None) => None
        };
        tasks.push(run_local_command_async(&cmd_c, &merged_args));
    }

    for task in tasks {
        task.await?
    }
    Ok(())
}

/// Starts an async command on the local host and returns the task handle.
fn run_local_command_async(cmd: &String, args: &Option<Vec<String>>) -> JoinHandle<()> {
    let cmd_c = cmd.clone();
    let args_c = args.clone();
    task::spawn(async move {
        let mut cmd_build = Command::new(&cmd_c); 

        // TODO: Add some arg validation
        if let Some(arg_vals) = &args_c {
            println!("Run added args {:?}", arg_vals);
            cmd_build.args(arg_vals);
        }

        // Run and collect stdout/stderr
        match cmd_build.output() {
            Ok(out) => {
                if out.status.success() {
                    return; 
                }
                let out_str = match String::from_utf8(out.stderr) {
                    Ok(x) => x,
                    Err(_) => panic!(
                        "Failed to convert output for command {} on host {:?} to string.",
                        &cmd_c,
                       &args_c 
                    ),
                };
                let status_msg = if let Some(status) = out.status.code() { 
                    status.to_string()
                } else { 
                    "(signal terminated)".to_string()
            };
            panic!("Command {} failed with status {:?}; stderr is {}",&cmd_c, status_msg, out_str);
            }
            Err(e) => {
                panic!(
                    "Output failed for local command {} with error: {}",&cmd_c, e
                );
            }
        };
    })
} 
/// Consume the groups to ensure they aren't used after closure.
pub async fn close_group_sessions(group: SessionMap) -> std::result::Result<(), Error> {
    for (_, s) in group {
        Arc::into_inner(s).unwrap().close().await?
    }
    Ok(())
}

/// Returns a Dir object from a directory string, or an error if it fails. 
pub async fn get_remote_dir_obj(s: &Sftp, dpath: &str) -> std::result::Result<Dir, AnyError> {
    Ok(s.fs().open_dir(dpath).await?)
}

/// Prints all files in a directory.
pub async fn read_remote_dir(dir: &Dir) {
    let read_dir = dir.clone().read_dir();
    // TODO: Consumes, but would rather it not. Can't find a way to do that though.
    let dir_map = read_dir.map(|item| -> String {
        item.unwrap()
            .filename()
            .to_str()
            .expect("Should be value")
            .to_string()
            .clone()
    });
    tokio::pin!(dir_map);
    while let Some(entry) = dir_map.next().await {
        println!("Entry found in dir {:?}", entry);
    }
}

/// For some reason, the sftp library doesn't support
/// copying directories remote->local. Just execute SCP
/// Format scp -r  
pub async fn retrieve_dir_all_remotes(
    s: &SessionMap,
    remote_path: &String, // e.g. (Remote) /srv/data
    local_path: &String, // e.g. (Local) ~/results 
) -> std::result::Result<(), AnyError> {
    let ex = fs::exists(local_path)?;
    if !ex { 
        match fs::create_dir_all(local_path) { 
            Ok(_) => {}, 
            Err(e) => eprintln!("Failed to create dir {} (may already exist).", local_path),
        } 
    } 
    local_scp_over_group(s,
        move |host: &String| -> Vec<String> {
            let full_remote = format!("{}:{}", host.clone(), remote_path.clone());
            let full_local = format!("{}/{}", local_path.clone(),host.clone(),);
            vec![full_remote.clone(), full_local.clone()]
        },
    )
    .await?;
    Ok(())
}

/// Runs scp -r on the local host, across each target in the group. 
/// This function takes a closure 'f' returning a Vec; from this vec 'closure_vec', the command will run: 
///     scp -r closure_vec[0] closure_vec[1]
pub async fn local_scp_over_group<F>(
    s: &SessionMap,
    f: F
) -> std::result::Result<(), AnyError>
where
    F: Fn(&String) -> Vec<String>,
{
    let scp_cmd = String::from("scp");
    let args = vec!["-r".to_string()];
    run_local_command_on_group(
        &scp_cmd,
        &s,
        &Some(args),
        Some(f),
    )
    .await?;
    Ok(())
} 


pub async fn upload_dir_all_remotes(
    s: &SessionMap,
    local_path: &String, // e.g. (Local) ~/results 
    remote_path: &String, // e.g. (Remote) /srv/data
) -> std::result::Result<(), AnyError> {
    // Check the directory to upload exists
    fs::exists(local_path)?;
    local_scp_over_group(s,
        move |host: &String| -> Vec<String> {
            let full_remote = format!("{}:{}", host.clone(), remote_path.clone());
            vec![local_path.clone(), full_remote.clone()]
        },
    )
    .await?;
    Ok(())
}

pub async fn delete_dir_all_remotes(s: &SessionMap, remote_path: &String) -> std::result::Result<(),AnyError> {
    run_remote_command_on_group::<fn(&String) -> Vec<String>>(&String::from("rm"), &s, &Some(vec!["-r".to_string(), remote_path.clone()]), None).await
}

pub async fn dir_exists_all_remotes(s: &SFTPMap, remote_path: &String) -> bool { 
    for (host, ss) in s.iter() {
        let  cur_fs = ss.fs();
        let pwd = cur_fs.cwd();
        println!("Host {} currently at {:?}", host, pwd);
        if get_remote_dir_obj(&ss, &remote_path).await.is_err() { 
            return false;
        } 
    }
    true
} 
#[cfg(test)]
mod tests {
    use super::*;

    // Implicitly tested in the run_* tests
    // #[tokio::test]

    // Change the vector to contain your SSH targets for testing.
    async fn create_test_group() -> std::result::Result<(SessionMap,SFTPMap),AnyError> { 
        let test_groups = vec![String::from("prod-agent1.recsa.prod"), String::from("prod-agent2.recsa.prod")];
        connect_to_group(&test_groups).await
    } 

    #[tokio::test]
    async fn connect_to_none() {
        let test_groups = vec![];
        let group = connect_to_group(&test_groups).await;
        assert!(group.is_err());
        let e = group.unwrap_err();
        assert_eq!(
            format!(
                "{:?}",
                e.downcast::<NoTargetError>()
                    .expect("Connect to none should be NoTargetError")
            ),
            "NoTargetError"
        );
    }

    #[tokio::test]
    async fn connect_to_unknown() {
        let test_groups =
            vec![String::from("doesntexist1"), String::from("doesntexist2")];
        let group = connect_to_group(&test_groups).await;
        assert!(group.is_err());

        // Check the error isn't the NoTargets one (we're passing targets, they just shouldn't be
        // recognised).
        let e = group.unwrap_err();
        match e.downcast::<NoTargetError>() {
            Ok(_) => panic!("Downcasting to NoTargetError should fail"),
            Err(_) => assert!(true),
        }
    }

    #[tokio::test]
    async fn run_known_command() {
        let group_res = create_test_group().await;
        assert!(group_res.is_ok());
        let (group, sftp_group) = group_res.unwrap();

        let cmd_res =
            run_remote_command_on_group::<fn(&String) -> Vec<String>>(
                &String::from("whoami"),
                &group,
                &None,
                None,
            )
            .await;
        assert!(cmd_res.is_ok());
    }

    #[tokio::test]
    async fn run_unknown_command() {
        let group_res = create_test_group().await;
        assert!(group_res.is_ok());
        let (group, sftp_group) = group_res.unwrap();

        let cmd_res =
            run_remote_command_on_group::<fn(&String) -> Vec<String>>(
                &String::from("surelythisdoesntexist"),
                &group,
                &None,
                None,
            )
            .await;
        assert!(cmd_res.is_err());
    }

    #[tokio::test]
    async fn dir_retrieval() {
        let group_res = create_test_group().await;
        assert!(group_res.is_ok());
        let (s_group, sftp_group) = group_res.unwrap();

        for (host, ss) in sftp_group.into_iter() {
            let  cur_fs = ss.fs();
            let pwd = cur_fs.cwd();
            println!("Host {} currently at {:?}", host, pwd);
            assert_eq!(pwd.to_str().unwrap(), "");

            assert!(get_remote_dir_obj(&ss, "/tmp").await.is_ok());
            assert!(get_remote_dir_obj(&ss, "/root").await.is_ok());
            assert!(get_remote_dir_obj(&ss, "/home").await.is_ok());

            assert!(get_remote_dir_obj(&ss, "/abcdef_ghijk").await.is_err());
            assert!(get_remote_dir_obj(&ss, "hello123").await.is_err());
            assert!(get_remote_dir_obj(&ss, "/proc\0bablyshouldntexist\0")
                .await
                .is_err());
            assert!(get_remote_dir_obj(&ss, "/proc\0/proc/stat\0")
                .await
                .is_err());
        }
        let dir_result = retrieve_dir_all_remotes(
            &s_group,
            &"/tmp/abcde".to_string(),
            &"test/".to_string(),
        )
        .await;
        assert!(dir_result.is_err());

        let test_dir = "./test";
        // Delete the testing folder if it exists 
        if fs::exists(test_dir).expect("Path should exist or not.") {
            fs::remove_dir_all(test_dir).expect("Deletion of test folder (./test) should be fine.");
        }
        let dir_result = retrieve_dir_all_remotes(
            &s_group,
            &"/etc/hostname".to_string(),
            &test_dir.to_string(),
        )
        .await;
        assert!(dir_result.is_ok());
        assert!(fs::exists(test_dir).expect("Path should exist.")); 
        assert!(fs::exists(format!("{}/prod-agent1.recsa.prod", test_dir)).expect("Path should exist.")); 
        assert!(fs::exists(format!("{}/prod-agent2.recsa.prod", test_dir)).expect("Path should exist.")); 
    }

    #[tokio::test]
    async fn dir_upload() {
        let group_res = create_test_group().await;
        assert!(group_res.is_ok());
        let (s_group, sftp_group) = group_res.unwrap();

        // If this doesn't exist, create some recognisable files in ssh_utils/upload_test for testing.
        let to_upload = "./upload_test";

        let remote_loc = "/tmp/experimentah-upload-test";

        // for (host, ss) in sftp_group.iter() {
        //     let  cur_fs = ss.fs();
        //     let pwd = cur_fs.cwd();
        //     println!("Host {} currently at {:?}", host, pwd);
        //     assert_eq!(pwd.to_str().unwrap(), "");
        //     assert!(get_remote_dir_obj(&ss, &remote_loc).await.is_err());
        // }

        assert!(!dir_exists_all_remotes(&sftp_group, &remote_loc.to_string()).await);
        assert!(upload_dir_all_remotes(&s_group, &to_upload.to_string(), &remote_loc.to_string()).await.is_ok());
        assert!(dir_exists_all_remotes(&sftp_group, &remote_loc.to_string()).await);

        for (host, ss) in sftp_group.iter() {
            let  cur_fs = ss.fs();
            let pwd = cur_fs.cwd();
            println!("Host {} currently at {:?}", host, pwd);
            assert_eq!(pwd.to_str().unwrap(), "");
            let d = get_remote_dir_obj(&ss, &remote_loc).await;
            assert!(d.is_ok());
            println!("In upload dir:");
            read_remote_dir(&d.unwrap()).await;
        }

        // Cleanup 
        assert!(delete_dir_all_remotes(&s_group, &remote_loc.to_string()).await.is_ok());

        assert!(!dir_exists_all_remotes(&sftp_group, &remote_loc.to_string()).await);
        // for (host, ss) in sftp_group.iter() {
        //     let  cur_fs = ss.fs();
        //     let pwd = cur_fs.cwd();
        //     println!("Host {} currently at {:?}", host, pwd);
        //     assert_eq!(pwd.to_str().unwrap(), "");
        //     assert!(get_remote_dir_obj(&ss, &remote_loc).await.is_err());
        // }
    }
    // Example use case - start a group of hosts, run the sample_script.sh and collect the
    // directory it outputs.
    #[tokio::test]
    async fn full_run_sample_script(){ 
        let group_res = create_test_group().await;
        assert!(group_res.is_ok());
        let (s_group, sftp_group) = group_res.unwrap();

        let to_upload = "./test_scripts";
        // This is a temp path for output files, specified in the sample_script.sh
        let remote_loc = "/tmp/full-exp-test";
        // Make sure this is executable.
        let target_script_loc = "/tmp/full-exp-test/sample_script.sh";
        
        // For testing, remove the remote files if they already exist so we get updated ones
        if (dir_exists_all_remotes(&sftp_group, &remote_loc.to_string()).await) { 
            assert!(delete_dir_all_remotes(&s_group, &remote_loc.to_string()).await.is_ok())
        } 

        assert!(upload_dir_all_remotes(&s_group, &to_upload.to_string(), &remote_loc.to_string()).await.is_ok());
        assert!(dir_exists_all_remotes(&sftp_group, &remote_loc.to_string()).await);

        let cmd_res =
            run_remote_command_on_group::<fn(&String) -> Vec<String>>(
                &target_script_loc.to_string(), 
                &s_group,
                &None,
                None,
            )
            .await;
        assert!(cmd_res.is_ok());

        assert!(retrieve_dir_all_remotes(&s_group, &remote_loc.to_string(), &"./test/sample_dir".to_string()).await.is_ok());
        assert!(run_local_command_async(&"ls".to_string(), &Some(vec!["test/sample_dir".to_string()])).await.is_ok());
    }
}
