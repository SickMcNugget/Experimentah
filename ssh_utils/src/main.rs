pub mod ssh_utils;
use openssh::{Error, KnownHosts, Session};
pub async fn run_cmd() -> Result<(), Error> {
    let session = Session::connect("p1", KnownHosts::Strict).await?;

    let ls = session.command("ls").output().await?;
    eprintln!(
        "{}",
        String::from_utf8(ls.stdout)
            .expect("server output was not valid UTF-8")
    );

    let whoami = session.command("whoami").output().await?;

    session.close().await
}
#[tokio::main]
async fn main() {
    println!("Hello, world!");
    run_cmd().await.unwrap();
}
