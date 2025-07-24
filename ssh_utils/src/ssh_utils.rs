// use futures_core::stream::Stream;
use futures_util::StreamExt;
use openssh::{Error, KnownHosts, Session};
use openssh_sftp_client::{
    fs::Dir,
    Sftp, SftpOptions,
};
use std::{collections::HashMap, fs};
use std::fmt;
use std::process::Command;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};

// Bit ugly, but we need separate maps for the normal Sessions and SFTP ones.
type SessionMap = HashMap<String, Arc<Session>>;
type SFTPMap = HashMap<String, Arc<Sftp>>;
// type AnyError = Box<dyn std::error::Error>;
type AnyError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, Clone)]
struct NoTargetError;
impl std::error::Error for NoTargetError {}
impl fmt::Display for NoTargetError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Empty Targets List is disallowed (must be at least one valid host)")
    }
}
/// Connects to a list of target hosts.
pub async fn connect_to_group(
    hosts: &Vec<String>,
) -> Result<(SessionMap, SFTPMap), AnyError> {
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

// Runs command simultaneously on each specified host.
/// Note: this currently doesn't crash if the command fails, and prints the stderr instead.
/// It could be changed to panic on command failure, but some programs print to stderr deliberately
/// (e.g. time).
pub async fn run_remote_command_on_group<F>(
    cmd: &String,
    group: &SessionMap,
    args: &Option<Vec<String>>,
    custom_args_function: Option<F>,
) -> Result<(), AnyError>
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
) -> Result<(), AnyError>
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
pub async fn close_group_sessions(group: SessionMap) -> Result<(), Error> {
    for (_, s) in group {
        Arc::into_inner(s).unwrap().close().await?
    }
    Ok(())
}

/// Returns a Dir object from a directory string, or an error if it fails. 
pub async fn get_remote_dir_obj(s: &Sftp, dpath: &str) -> Result<Dir, AnyError> {
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
) -> Result<(), AnyError> {
    // let scp_cmd = String::from("scp");
    // let args = vec!["-r".to_string()];
    let ex = fs::exists(local_path)?;
    if !ex { 
        match fs::create_dir_all(local_path) { 
            Ok(_) => {}, 
            Err(e) => eprintln!("Failed to create dir {} (may already exist).", local_path),
        } 
    } 
    // run_local_command_on_group(
    //     &scp_cmd,
    //     &s,
    //     &Some(args),
    //     Some(move |host: &String| -> Vec<String> {
    //         let full_src = format!("{}:{}", host.clone(), remote_path.clone());
    //         let full_dst = format!("{}/{}", local_path.clone(),host.clone(),);
    //         vec![full_src.clone(), full_dst.clone()]
    //     }),
    // )
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
) -> Result<(), AnyError>
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
) -> Result<(), AnyError> {
    // Check the directory to upload exists
    let ex = fs::exists(local_path)?;
    local_scp_over_group(s,
        move |host: &String| -> Vec<String> {
            let full_remote = format!("{}:{}", host.clone(), remote_path.clone());
            vec![local_path.clone(), full_remote.clone()]
        },
    )
    .await?;
    Ok(())
}

pub async fn delete_dir_all_remotes(s: &SessionMap, remote_path: &String) -> Result<(),AnyError> {
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
    // async fn connect_to_known() {
    //     // These should be known in your SSH config.
    //     let test_groups = vec![String::from("p1"), String::from("p2")];
    //     let group = connect_to_group(&test_groups).await;
    //     assert!(group.is_ok());
    // }

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
        let test_groups = vec![String::from("p1"), String::from("p2")];
        let group_res = connect_to_group(&test_groups).await;
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
        let test_groups = vec![String::from("p1"), String::from("p2")];
        let group_res = connect_to_group(&test_groups).await;
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
        let test_groups = vec![String::from("p1"), String::from("p2")];
        let group_res = connect_to_group(&test_groups).await;
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
        assert!(fs::exists(format!("{}/p1", test_dir)).expect("Path should exist.")); 
        assert!(fs::exists(format!("{}/p2", test_dir)).expect("Path should exist.")); 
    }

    #[tokio::test]
    async fn dir_upload() {
        let test_groups = vec![String::from("p1"), String::from("p2")];
        let group_res = connect_to_group(&test_groups).await;
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
        let test_groups = vec![String::from("p1"), String::from("p2")];
        let group_res = connect_to_group(&test_groups).await;
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
