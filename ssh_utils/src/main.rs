pub mod ssh_utils;
use openssh::{Error, KnownHosts, Session};
use std::collections::HashMap;
use std::str;
use std::sync::{Arc, Mutex};
use tokio::task;

pub async fn connect_to_group(hosts: &Vec<String>) -> HashMap<String, Session> {
    // Using a vec since I don't think we care about order - could convert to HashMap if necessary
    let mut group: HashMap<String, Session> = HashMap::new();
    for host in hosts {
        match Session::connect(host, KnownHosts::Strict).await {
            Ok(s) => {
                group.insert(host.to_owned(), s);
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
    group: &mut HashMap<String, Session>,
    args: &Option<Vec<String>>,
) {
    // TODO: The openssh crate doesn't seem to provide a way to run these
    // async directly, but could look for a better way regardless.
    let mut tasks = Vec::new();
    let cmd_c = cmd.clone();
    // let group_c = group.clone();
    let group_c: Arc<Mutex<HashMap<String, Session>>> =
        Arc::new(Mutex::new(*group));
    // let args_c = args.clone();
    for (host, s) in group {
        tasks.push(task::spawn(async move {
            // Creates an OwningCommand you can add args to.
            let mut own_cmd = s.command(cmd);
            // TODO: Add some arg validation
            if let Some(arg_vals) = args {
                own_cmd.args(arg_vals);
            }
            // Run and collect stdout/stderr
            match own_cmd.output().await {
                Ok(out) => {
                    let out_str = match String::from_utf8(out.stdout) {
                        Ok(x) => x,
                        Err(_) => String::from("Failed to convert to string."),
                    };
                    println!("Output is {} from host {}", out_str, host);
                }
                Err(e) => {
                    eprintln!(
                        "Output failed for host {} with error: {}",
                        host, e
                    );
                }
            };
        }))
    }
}

/// Consume the groups to ensure they aren't used after closure.
pub async fn close_group_sessions(group: HashMap<String, Session>) {
    for (host, s) in group {
        match s.close().await {
            Ok(r) => {}
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
