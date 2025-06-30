pub mod ssh_utils;

use ssh_utils::*;

#[tokio::main]
async fn main() {
    println!("Hello, world!");
    let test_groups =
        vec![String::from("p1"), String::from("p2"), String::from("p3")];
    let (group, _) = connect_to_group(&test_groups).await.unwrap();
    // run_cmd_on_group(&String::from("hostname"), &group, &None).await;
    // run_cmd_on_group(
    //     &String::from("sleep"),
    //     &group,
    //     &Some(vec![String::from("5")]),
    // )
    // .await;
    close_group_sessions(group).await;
}
