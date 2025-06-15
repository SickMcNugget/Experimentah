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

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let timeout = Duration::from_secs(args.timeout);

    let config: Arc<Config> = match Config::from_toml(&args.config) {
        Ok(config) => Arc::new(config),
        Err(e) => panic!("{e}"),
    };

    println!(
        "Successfully parsed and validated config: {:?}",
        &args.config
    );

    let experiment_config: Arc<ExperimentConfig> =
        match ExperimentConfig::from_toml(
            &args.experiment_config,
            config.as_ref(),
        ) {
            Ok(experiment_config) => Arc::new(experiment_config),
            Err(e) => panic!("{e}"),
        };

    println!(
        "Successfully parsed and validated experiment config: {:?}",
        &args.experiment_config
    );

    println!("Logging to file: {:?}", args.log_file);

    let client = Arc::new(
        Client::builder()
            .timeout(timeout)
            .build()
            .expect("Somehow an invalid client was built"),
    );

    let er = ExperimentRunner::new(
        config.as_ref(),
        experiment_config.as_ref(),
        client.clone(),
    );

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
    println!("Ensuring all runners are up and reachable");
    er.prep_runners().await;

    tokio::spawn(async move { heartbeat_thread(client, config) });

    let runs = &experiment_config.runs;
    println!("= Running Experiment {}=", runs);

    for _ in 0..*runs {
        // er.
        // let successful_experiments = er.run_experiments().await;
        // dbg!("{:?}", successful_experiments.unwrap());
    }

    println!("\n= Successful Experiments =");
    // println!("{}",)

    // Need to validate host endpoints in the jobs for each experiment
    //  This requires comparing against config
    //
    //
    // Contact all servers: metrics-api, workload repository, postgres, prometheus, runner(s)
    //  Also check periodically during running!
    //
    // Create experiment
    //
    // Create job under experiment
    //
    // Run job (by contacting the runner)
    //  Post results of job and run next one until finished
}

// Runs periodically and determines service connectivity
async fn heartbeat_thread(client: Arc<Client>, config: Arc<Config>) {
    loop {
        let brain_promise = client
            .get(format!("{}/status", config.brain_endpoint()))
            .send();

        let mut runner_promises = Vec::with_capacity(config.runners.len());
        for runner in config.runner_endpoints() {
            runner_promises
                .push(client.get(format!("{}/status", runner)).send());
        }

        match brain_promise.await {
            Ok(response) => match response.status() {
                StatusCode::OK => {}
                _ => eprintln!(
                    "Bad status code from brain: {}",
                    response.status()
                ),
            },
            Err(..) => {
                eprintln!("Error awaiting brain");
            }
        }
        for runner_promise in runner_promises {
            match runner_promise.await {
                Ok(response) => match response.status() {
                    StatusCode::OK => {}
                    _ => eprintln!(
                        "Bad status code from runner: {}",
                        response.status()
                    ),
                },
                Err(..) => {
                    eprintln!("Error awaiting runner");
                }
            }
        }
        tokio::time::sleep(HEARTBEAT_INTERVAL).await;
    }
}

// fn run_all(client: &Client, experiment_config: &ExperimentConfig, config: &Config) {
//     for experiment in experiment_config.experiments().iter() {
//         run_experiment(experiment, &client, experiment_config, config);
//     }
// }

// fn run_experiment(
//     experiment: &Experiment,
//     client: &Client,
//     experiment_config: &ExperimentConfig,
//     config: &Config,
// ) -> Result<Vec<String>, String> {
//     let experiment_id = create_experiment(experiment, client, &config.metric_server());
//     exporters_mapping = configure_prometheus(experiment_id, config);
//
//     Ok(vec!["ligma".to_string()])
// }
//
// async fn create_experiment(
//     experiment: &Experiment,
//     client: &Client,
//     url: &String,
// ) -> reqwest::Result<String> {
//     let endpoint = format!("http://{}/experiment", url);
//
//     let response = {
//         let now = get_time();
//         let body = HashMap::from([("name", experiment.name()), ("timestamp_ms", &now)]);
//         client.post(endpoint).json(&body).send().await?
//     };
//
//     response.error_for_status_ref()?;
//     let ret = response.json().await?;
//     Ok(ret)
// }
//
// fn configure_prometheus(experiment_id: String, config: &Config, url: &String) -> HashMap<&str, String> {
//     let endpoint = format!("http://{}/prometheus", url);
//
//
//
//
//     let response = {
//         let body = config
//
//     }
// }
//
// fn get_time() -> String {
//     std::time::SystemTime::now()
//         .duration_since(std::time::UNIX_EPOCH)
//         .expect("Time went backwards")
//         .as_millis()
//         .to_string()
// }
//
fn default_log_file() -> PathBuf {
    let mut path = env::current_exe().unwrap();
    path.pop();
    path.push("log/experimentah.log");
    path
}

// fn parse_duration(arg: &str) -> Result<Duration, std::num::ParseIntError> {
//     let seconds = arg.parse()?;
//     Ok(Duration::from_secs(seconds))
// }

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
    config: PathBuf,

    /// The experiment-specific configuration file
    #[arg(value_name = "EXPERIMENT_CONFIG_FILE")]
    experiment_config: PathBuf,

    /// Perform all initial checks without running the experiment
    #[arg(short, long)]
    dry_run: bool,

    /// The log file storing controller outputs
    #[arg(short, long, default_value=default_log_file().into_os_string())]
    log_file: PathBuf,

    #[arg(short, long, default_value_t = 5)]
    timeout: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }

    #[tokio::test]
    async fn server_checks() {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        assert!(check_metrics_api(
            &client,
            &"http://localhost:50000".to_string()
        )
        .await
        .is_err());

        assert!(check_runners(
            &client,
            &vec![
                "http://localhost:9010".into(),
                "http://localhost:9011".into()
            ]
        )
        .await
        .is_err())
    }
}
