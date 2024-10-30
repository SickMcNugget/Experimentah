use std::collections::HashMap;
use std::path::PathBuf;
use std::{env, fs};
use toml;

use clap::Parser;

use controller::parse::{Config, Experiment, ExperimentConfig};
use controller::run::ExperimentRunner;

use reqwest::Client;

use std::time::Duration;

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let timeout = Duration::from_secs(args.timeout);

    let config: Config = {
        let config_str = fs::read_to_string(args.config).expect("Unable to read config file");
        toml::from_str(&config_str).expect("Unable to parse config toml")
    };
    let experiment_config: ExperimentConfig = {
        let experiment_config_str = fs::read_to_string(args.experiment_config)
            .expect("Unable to read experiment config file");
        toml::from_str(&experiment_config_str).expect("Unable to parse experiment config toml")
    };

    if let Err(e) = ExperimentConfig::validate(&experiment_config, &config) {
        panic!("{e}");
    }

    println!("Logging to file: {:?}", args.log_file);

    println!(
        "--- Config ---\n{:#?}\n--- Experiment Config ---\n{:#?}",
        config, experiment_config
    );

    let client = Client::builder()
        .timeout(timeout)
        .build()
        .expect("Somehow an invalid client was built");

    let er = ExperimentRunner::build(config, experiment_config, client)
        .expect("Error creating ExperimentRunner");

    if let Err(e) = er.check_metrics_api().await {
        panic!("{e}")
    }

    if let Err(e) = er.check_runners().await {
        panic!("{e}")
    }

    let runs: u16 = er.experiment_config().runs();
    println!("= Running Experiment {}=", runs);

    for i in 0..runs {
        er.run_experiments();
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
async fn check_metrics_api(client: &Client, url: &String) -> reqwest::Result<()> {
    let endpoint = format!("http://{}/status", url);
    let response = client.get(endpoint).send().await?; // .await?;
    response.error_for_status()?;
    Ok(())
}

async fn check_runners(client: &Client, urls: &Vec<String>) -> reqwest::Result<()> {
    for url in urls.iter() {
        let endpoint = format!("http://{}/status", url);
        let response = client.get(endpoint).send().await?;
        response.error_for_status()?;
    }
    Ok(())
}

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
#[command(about = "Allows a user to run experiments using defined configs/experiment configs")]
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

        assert!(
            check_metrics_api(&client, &"http://localhost:50000".to_string())
                .await
                .is_err()
        );

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
