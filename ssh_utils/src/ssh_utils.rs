use openssh::{Error, KnownHosts, Session};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tokio::task::{self, JoinError};

type SessionMap = HashMap<String, Arc<Session>>;
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
) -> Result<SessionMap, AnyError> {
    if hosts.is_empty() {
        Err(NoTargetError)?
    }
    let mut group: SessionMap = HashMap::new();
    for host in hosts {
        let s = Session::connect(host, KnownHosts::Strict).await?;
        group.insert(host.to_owned(), Arc::new(s));
    }
    // Should never fail since we don't accept an empty host list and failures to connect should
    // exit early. Validate to make sure this holds.
    assert!(!group.is_empty());
    Ok(group)
}

pub async fn run_cmd_on_group(
    cmd: &String,
    group: &HashMap<String, Arc<Session>>,
    args: &Option<Vec<String>>,
) -> Result<(), AnyError> {
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

        tasks.push(task::spawn(async move {
            // Creates an OwningCommand you can add args to.
            let mut own_cmd = s_c.command(&cmd_c);

            // TODO: Add some arg validation
            if let Some(arg_vals) = args_c {
                own_cmd.args(arg_vals);
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

/// Consume the groups to ensure they aren't used after closure.
pub async fn close_group_sessions(
    group: HashMap<String, Arc<Session>>,
) -> Result<(), Error> {
    for (_, s) in group {
        Arc::into_inner(s).unwrap().close().await?
    }
    Ok(())
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
        let group = group_res.unwrap();

        let cmd_res =
            run_cmd_on_group(&String::from("hostname"), &group, &None).await;
        assert!(cmd_res.is_ok());
    }

    #[tokio::test]
    async fn run_unknown_command() {
        let test_groups = vec![String::from("p1"), String::from("p2")];
        let group_res = connect_to_group(&test_groups).await;
        assert!(group_res.is_ok());
        let group = group_res.unwrap();

        let cmd_res = run_cmd_on_group(
            &String::from("surelythisdoesntexist"),
            &group,
            &None,
        )
        .await;
        assert!(cmd_res.is_err());
    }
}
