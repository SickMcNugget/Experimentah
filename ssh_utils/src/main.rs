pub mod ssh_utils;
use openssh::{Error, KnownHosts, Session};
use std::collections::HashMap;
use std::str;
use std::sync::{Arc, Mutex};
use tokio::task;

pub async fn connect_to_group(
    hosts: &Vec<String>,
) -> HashMap<String, Arc<Session>> {
    // Using a vec since I don't think we care about order - could convert to HashMap if necessary
    let mut group: HashMap<String, Arc<Session>> = HashMap::new();
    for host in hosts {
        match Session::connect(host, KnownHosts::Strict).await {
            Ok(s) => {
                group.insert(host.to_owned(), Arc::new(s));
            }
            Err(e) => {
                eprintln!(
                    "Failed to connect to host {} with error: {}.",
                    host, e
                );
            }
        };
    }
    group
}

pub async fn run_cmd_on_group(
    cmd: &String,
    group: &HashMap<String, Arc<Session>>,
    args: &Option<Vec<String>>,
) {
    // TODO: The openssh crate doesn't seem to provide a way to run these
    // async directly, but could look for a better way regardless.
    let mut tasks = Vec::new();

    // for (host, s) in group {
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
                    let out_str = match String::from_utf8(out.stdout) {
                        Ok(x) => x,
                        Err(_) => String::from("Failed to convert to string."),
                    };
                    println!("Output is {} from host {}", out_str, host_c);
                }
                Err(e) => {
                    eprintln!(
                        "Output failed for host {} with error: {}",
                        host_c, e
                    );
                }
            };
        }))
    }

    for task in tasks {
        match task.await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Thread failed to join with error: {}.", e)
            }
        }
    }
}

/// Consume the groups to ensure they aren't used after closure.
pub async fn close_group_sessions(group: HashMap<String, Arc<Session>>) {
    for (_, s) in group {
        match Arc::into_inner(s).unwrap().close().await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Failed to close session with error: {}", e)
            }
        }
    }
}

#[tokio::main]
async fn main() {
    println!("Hello, world!");
    let test_groups =
        vec![String::from("p1"), String::from("p2"), String::from("p3")];
    let mut group = connect_to_group(&test_groups).await;
    run_cmd_on_group(&String::from("hostname"), &mut group, &None).await;
    run_cmd_on_group(
        &String::from("sleep"),
        &mut group,
        &Some(vec![String::from("5")]),
    )
    .await;
    close_group_sessions(group);
}
