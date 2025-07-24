// Written by Joren Regan
// Date: 2025-06-13
//
// This is the client-side portion of the Experimentah system.
// Two threads are at play (once validation has succeeded and the system is ready):
// 1. The experiment running thread. Tells the runner what to do and waits until it's done.
// 2. The heartbeat thread. Ensures that endpoints continue to be reachable. If they aren't,
// after a time delay, notifies the main thread which then:
//  a. Tries to shut down the runner, collecting whatever results may exist.
//  b. Tries to upload collected results (if the error was with repo node)
//  c. Dies

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;

use controller::parse::{Config, ExperimentConfig};
use controller::run::ExperimentRunner;

use reqwest::{Client, StatusCode};

use std::time::Duration;

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(5000);

// #[tokio::main]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let address = args.address;
    let timeout = Duration::from_secs(args.timeout);

    std::env::set_current_dir(args.working_directory)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()?;

    if args.status {
        let res = client.post(format!("http://{address}/status")).send()?;
        dbg!(res);
    } else {
        let config = &args.config.expect("Config should have been set on CLI");
        let experiment_config = &args
            .experiment_config
            .expect("Experiment config should have been set on CLI");

        let config: Config = Config::from_file(config)?;
        println!("Successfully parsed config: {:?}", config);
        config.validate()?;
        println!("Successfully validated config: {:?}", config);

        let experiment_config: ExperimentConfig =
            ExperimentConfig::from_file(experiment_config)?;
        println!(
            "Successfully parsed experiment config: {:?}",
            experiment_config
        );
        experiment_config.validate(&config)?;
        println!(
            "Successfully validated experiment_config: {:?}",
            experiment_config
        );

        let res = client
            .post(format!("http://{address}/run"))
            .json(&(config, experiment_config))
            .send()?;
        dbg!(res);
    }

    Ok(())

    // println!("Logging to file: {:?}", args.log_file);

    // let client = Client::builder()
    //     .timeout(timeout)
    //     .build()
    //     .expect("Somehow an invalid client was built");

    // let mut er = ExperimentRunner::new(
    //     config.as_ref(),
    //     experiment_config.as_ref(),
    //     client.clone(),
    // );

    // I think that I want the controller to ensure that runners are available on the hosts they're
    // meant to be on.
    // I want the runner to ensure that exporters are working as expected, though, meaning that
    // an endpoint needs to be added to the runner which allows an exporter to be registered (and
    // subsequently monitored) by that runner. Exporters will be programs that are run in the
    // background during the execution of a run. We'll use and advisory lock to make sure that
    // multiple instances of the exporter program are not created, and that they're properly killed
    // at the end of a run. I think the expectation at this point is that an exporter should be a
    // long-running process which outputs it's data to a file. It's the responsibility of the
    // exporter itself to include timestamp information and do all the heavy lifting regarding
    // metrics collection.
    // Think tools like sar, and raritan (my bespoke power usage script).

    // TODO(joren): In the future, it may be better to only prepare runners for the experiment
    // that's about to be run. For now, I'd like to do everything at the beginning, though.
    // println!("Ensuring all runners are up and reachable");
    // if let Err(e) = er.check_runners().await {
    //     panic!("{e}");
    // }
    //
    // tokio::spawn(async move { heartbeat_thread(client, config).await });
    //
    // let runs = &experiment_config.runs;
    // println!("= Running Experiment {}=", runs);
    //
    // for _ in 0..*runs {
    //     todo!();
    // }
    //
    // println!("\n= Successful Experiments =");
}

// Runs periodically and determines service connectivity
// async fn heartbeat_thread(client: Client, config: Arc<Config>) {
//     loop {
//         let brain_promise = client
//             .get(format!("{}/status", config.brain_endpoint()))
//             .send();
//
//         let mut runner_promises = Vec::with_capacity(config.runners.len());
//         for runner in config.runner_endpoints() {
//             runner_promises
//                 .push(client.get(format!("{}/status", runner)).send());
//         }
//
//         match brain_promise.await {
//             Ok(response) => match response.status() {
//                 StatusCode::OK => {}
//                 _ => eprintln!(
//                     "Bad status code from brain: {}",
//                     response.status()
//                 ),
//             },
//             Err(..) => {
//                 eprintln!("Error awaiting brain");
//             }
//         }
//         for runner_promise in runner_promises {
//             match runner_promise.await {
//                 Ok(response) => match response.status() {
//                     StatusCode::OK => {}
//                     _ => eprintln!(
//                         "Bad status code from runner: {}",
//                         response.status()
//                     ),
//                 },
//                 Err(..) => {
//                     eprintln!("Error awaiting runner");
//                 }
//             }
//         }
//         tokio::time::sleep(HEARTBEAT_INTERVAL).await;
//     }
// }

fn default_log_file() -> PathBuf {
    let mut path = env::current_exe().unwrap();
    path.pop();
    path.push("log/experimentah.log");
    path
}

fn default_working_directory() -> PathBuf {
    let mut path = env::current_dir().unwrap();
    path
}

#[derive(Parser, Debug)]
#[command(name = "Controller")]
#[command(version = "0.0.1")]
#[command(
    about = "Allows a user to run experiments using defined configs/experiment configs"
)]
#[command(long_about = None)]
struct Args {
    /// The config file storing global configuration
    #[arg(value_name = "CONFIG_FILE")]
    config: Option<PathBuf>,

    /// The experiment-specific configuration file
    #[arg(value_name = "EXPERIMENT_CONFIG_FILE")]
    experiment_config: Option<PathBuf>,

    #[arg(short, long, default_value_t = String::from("localhost:50000"))]
    address: String,
    /// Perform all initial checks without running the experiment
    #[arg(short, long)]
    dry_run: bool,

    /// The log file storing controller outputs
    #[arg(short, long, default_value=default_log_file().into_os_string())]
    log_file: PathBuf,

    #[arg(short, long)]
    status: bool,

    #[arg(short, long, default_value=default_working_directory().into_os_string())]
    working_directory: PathBuf,

    #[arg(short, long, default_value_t = 5)]
    timeout: u64,
}
