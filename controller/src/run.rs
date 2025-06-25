use crate::parse::{
    to_experiments, Config, Experiment, ExperimentConfig, Runner,
};

use reqwest::Client;
use std::process::{Child, Command};
use std::{collections::HashSet, path::Path};

type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(Debug)]
pub enum RuntimeError {
    ReqwestError(String, reqwest::Error),
    // I know, it's hilarious
    BadStatusCode(String, reqwest::StatusCode),
    IOError(String, std::io::Error),
    Generic(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            RuntimeError::ReqwestError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            RuntimeError::BadStatusCode(ref message, ref code) => {
                write!(f, "{message}: Failed with status code: {code}")
            }
            RuntimeError::IOError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            RuntimeError::Generic(ref message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            RuntimeError::ReqwestError(.., ref source) => Some(source),
            RuntimeError::IOError(.., ref source) => Some(source),
            RuntimeError::BadStatusCode(..) => None,
            RuntimeError::Generic(..) => None,
        }
    }
}

impl From<(&str, reqwest::Error)> for RuntimeError {
    fn from(value: (&str, reqwest::Error)) -> Self {
        RuntimeError::ReqwestError(value.0.to_string(), value.1)
    }
}

impl From<(String, reqwest::Error)> for RuntimeError {
    fn from(value: (String, reqwest::Error)) -> Self {
        RuntimeError::ReqwestError(value.0, value.1)
    }
}

impl From<(&str, std::io::Error)> for RuntimeError {
    fn from(value: (&str, std::io::Error)) -> Self {
        RuntimeError::IOError(value.0.to_string(), value.1)
    }
}

impl From<(String, std::io::Error)> for RuntimeError {
    fn from(value: (String, std::io::Error)) -> Self {
        RuntimeError::IOError(value.0, value.1)
    }
}

impl From<(String, reqwest::StatusCode)> for RuntimeError {
    fn from(value: (String, reqwest::StatusCode)) -> Self {
        RuntimeError::BadStatusCode(value.0, value.1)
    }
}

impl From<String> for RuntimeError {
    fn from(value: String) -> Self {
        RuntimeError::Generic(value)
    }
}

impl From<&str> for RuntimeError {
    fn from(value: &str) -> Self {
        RuntimeError::Generic(value.to_string())
    }
}

#[allow(dead_code)]
struct ChildProcess {
    address: String,
    child: Child,
}

impl ChildProcess {
    fn new(address: String, child: Child) -> Self {
        Self { address, child }
    }
}

// The ExperimentRunner should only be used AFTER parsing has been completed!
pub struct ExperimentRunner<'a> {
    pub experiments: Vec<Experiment<'a>>,
    subprocesses: Vec<ChildProcess>,

    client: Client,
}

impl<'a> ExperimentRunner<'a> {
    pub fn new(
        config: &'a Config,
        experiment_config: &'a ExperimentConfig,
        client: Client,
    ) -> Self {
        Self {
            experiments: to_experiments(config, experiment_config),
            subprocesses: vec![],
            client,
        }
    }

    // An experiment is constructed from the experiment configuration.
    // It's easier to construct it with a function because there isn't a one-to-one
    // mapping from an experiment variation to an experiment. There's an override system
    // that makes creating experiments easier on the user end. We handle all the cases
    // starting from this function.

    fn unique_runners(
        experiments: &'a [Experiment],
    ) -> HashSet<&'a Runner<'a>> {
        let mut unique_runners = HashSet::new();
        for experiment in experiments.iter() {
            for runner in experiment.runners.iter() {
                unique_runners.insert(runner);
            }
        }
        unique_runners
    }

    // Checks if runners are reachable
    // If any runners fail, they are returned in a vector to be activated
    // later.
    pub async fn check_runners(&mut self) -> Result<()> {
        let unique_runners =
            ExperimentRunner::unique_runners(&self.experiments);

        for runner in unique_runners.into_iter() {
            let url = runner.url() + "/status";
            match self.client.get(&url).send().await {
                Ok(response) => {
                    let status = response.status();
                    match status {
                        reqwest::StatusCode::OK => {
                            println!("Runner {} is available", runner.name)
                        }
                        _ => Err(RuntimeError::BadStatusCode(
                            format!("Error reaching runner at {}", url),
                            status,
                        ))?,
                    };
                }
                Err(e) => {
                    // If we were unable to connect to the runner,
                    // then we can try and make sure that there is a
                    // runner running
                    if e.is_connect() {
                        self.subprocesses.push(ChildProcess::new(
                            runner.address.to_string(),
                            ExperimentRunner::distribute_runner(
                                runner.address,
                                runner.port,
                            )?,
                        ));
                    } else {
                        Err(RuntimeError::from((
                            format!(
                                "Error making request to runner {}",
                                runner.name
                            ),
                            e,
                        )))?
                    }
                }
            }
        }
        Ok(())
    }

    // We assume the runner is in the current directory and simply called 'runner'
    // A key aspect of experimentah is the use of advisory locks. These
    fn distribute_runner(address: &str, _port: &u16) -> Result<Child> {
        let runner_binary = Path::new("./runner");

        dbg!(runner_binary);
        assert!(
            runner_binary.exists(),
            "Runner binary does not exist in current directory!"
        );

        if address == "localhost" {
            match Command::new(runner_binary).spawn() {
                Ok(child) => Ok(child),
                Err(e) => {
                    Err(RuntimeError::from(("Unable to execute runner", e)))?
                }
            }
        } else {
            match Command::new("ssh").arg(address).spawn() {
                Ok(child) => {
                    Ok(child)
                    // println!("{}", result.status)
                }
                Err(e) => Err(RuntimeError::from(("Error executing SSH", e)))?,
            }
        }
    }

    /// An 'experiment' can consist of multiple 'variations'.
    /// If the top level of an experiment config contains all the variables required
    /// to run an experiment, then an experiment is run with these variables in addition
    /// to any variations that may also be present inside the same config.
    ///
    /// This means you can have a default script that is run with default arguments on
    /// a default set of runners with default exporters, which can be overwritten in
    /// each and every variation.
    /// Setup and teardown cannot be varied, and the current recommendation is that
    /// another experiment configuration be made if these need to be changed.
    pub async fn run_experiments() {}
    // pub async fn run_experiments(&self) -> Result<Vec<String>, Box<dyn Error>> {
    //     let mut successful_experiments = Vec::new();
    //
    //     for variation in self.experiment_config.experiments().iter() {
    //         self.run_experiment(experiment).await?;
    //         successful_experiments.push(experiment.id().clone());
    //     }
    //
    //     Ok(successful_experiments)
    // }
    //
    // async fn run_experiment(
    //     &self,
    //     experiment: &Experiment,
    // ) -> Result<(), Box<dyn Error>> {
    //     let experiment_id = self.create_experiment(experiment).await?;
    //     experiment.allocate_id(experiment_id)?;
    //
    //     let exporters_mapping = self.configure_prometheus(experiment).await?;
    //     // TODO(joren): add wait for prometheus ready
    //
    //     self.experiment_setup(experiment).await?;
    //
    //     let mut job_ids = Vec::new();
    //     for job in experiment.jobs().iter() {
    //         let job_id = self.run_job(job, experiment).await;
    //         job_ids.push(job_id);
    //     }
    //
    //     // exporters_mapping = self.configure_prometheus(experiment_id);
    //     Ok(())
    // }
    //
    // pub async fn check_metrics_api(&self) -> reqwest::Result<()> {
    //     let endpoint = format!("http://{}/status", self.config.metric_server());
    //     let response = self.client.get(endpoint).send().await?; // .await?;
    //     response.error_for_status()?;
    //     Ok(())
    // }
    //
    // pub async fn check_runners(&self) -> reqwest::Result<()> {
    //     for url in self.config.runners().iter() {
    //         let endpoint = format!("http://{}/job", url);
    //         let response = self.client.get(endpoint).send().await?;
    //         response.error_for_status()?;
    //     }
    //     Ok(())
    // }
    //
    // async fn wait_for_jobs() {}
    //
    // async fn create_job(
    //     &self,
    //     job: &Job,
    //     experiment: &Experiment,
    // ) -> reqwest::Result<String> {
    //     let response = {
    //         let endpoint = format!(
    //             "http://{}/experiment/{}/job",
    //             self.config.metric_server(),
    //             experiment.id()
    //         );
    //         let body = json!({
    //             "experimentId": *experiment.id(),
    //             "runnerName": job.runner(),
    //             "exporters": "yummers",
    //             "workload": job.name(),
    //             "supplementary": "false",
    //             "resultsType": experiment.kind(),
    //             "arguments": job.arguments(),
    //             "timestampMs": get_time(),
    //         });
    //         // let body = HashMap::from([
    //         //     ("experimentId", experiment.id().clone()),
    //         //     ("runnerName", job.runner().clone()),
    //         //     ("exporters", "yummers".into()),
    //         //     ("workload", job.name().clone()),
    //         //     ("supplementary", "false".into()),
    //         //     ("resultsType", experiment.kind().clone()),
    //         //     ("arguments", job.arguments().to_string()),
    //         //     ("timestampMs", get_time()),
    //         // ]);
    //         self.client.post(endpoint).json(&body).send().await?
    //     };
    //
    //     response.error_for_status_ref()?;
    //     let ret = response.json().await?;
    //     Ok(ret)
    // }
    //
    // async fn start_job(
    //     &self,
    //     job_id: &str,
    //     job: &Job,
    //     experiment: &Experiment,
    // ) -> reqwest::Result<()> {
    //     let response = {
    //         let endpoint = format!("http://{}/job", job.runner());
    //         // let args_str = job.arguments().to_string();
    //         let body = json!({
    //             "jobId": job_id,
    //             "experimentId": *experiment.id(),
    //             "workload": job.name(),
    //             "workloadType": experiment.kind(),
    //             "arguments": job.arguments()
    //         });
    //         // let body: HashMap<&str, &str> = HashMap::from([
    //         //     ("jobId", job_id),
    //         //     ("experimentId", &experiment.id()()),
    //         //     ("workload", job.name()),
    //         //     ("workloadType", experiment.kind()),
    //         //     ("arguments", &args_str),
    //         // ]);
    //         self.client.post(endpoint).json(&body).send().await?
    //     };
    //
    //     response.error_for_status_ref()?;
    //     // let ret = response.json().await?;
    //     Ok(())
    // }
    //
    // async fn run_job(
    //     &self,
    //     job: &Job,
    //     experiment: &Experiment,
    // ) -> reqwest::Result<()> {
    //     let job_id = self.create_job(job, experiment).await?;
    //     self.start_job(&job_id, job, experiment).await
    // }
    // // fn update_experiment(&self, experiment: &mut Experiment) {}
    //
    // async fn create_experiment(
    //     &self,
    //     experiment: &Experiment,
    // ) -> reqwest::Result<String> {
    //     let response = {
    //         let endpoint =
    //             format!("http://{}/experiment", self.config.metric_server());
    //         let body = json!({
    //             "name": experiment.name(),
    //             "timestampMs": get_time()
    //         });
    //         // let body = HashMap::from([("name", experiment.name()), ("timestamp_ms", &now)]);
    //         self.client.post(endpoint).json(&body).send().await?
    //     };
    //
    //     response.error_for_status_ref()?;
    //     let experiment_id = response.json().await?;
    //     // experiment
    //     //     .allocate_id(experiment_id)
    //     //     .expect("Somehow the experiment id was already allocated");
    //
    //     Ok(experiment_id)
    // }
    //
    // async fn configure_prometheus(
    //     &self,
    //     experiment: &Experiment,
    // ) -> reqwest::Result<HashMap<String, String>> {
    //     let response = {
    //         let endpoint =
    //             format!("http://{}/prometheus", self.config.metric_server());
    //         let body = json!({
    //             "experimentId": *experiment.id(),
    //             "exporters": self.config.exporters_map()
    //         });
    //         // let body = HashMap::from([
    //         //     (
    //         //         "experiment_id",
    //         //         PrometheusRequestBody::String(experiment.id().clone()),
    //         //     ),
    //         //     (
    //         //         "exporters",
    //         //         PrometheusRequestBody::HashMap(self.config.exporters_map()),
    //         //     ),
    //         // ]);
    //         self.client.post(endpoint).json(&body).send().await?
    //     };
    //
    //     response.error_for_status_ref()?;
    //     let ret = response.json().await?;
    //     Ok(ret)
    // }
    //
    // async fn experiment_setup(
    //     &self,
    //     experiment: &Experiment,
    // ) -> Result<(), Box<dyn Error>> {
    //     for setup in experiment.setup().iter() {
    //         for runner in self.config.runners().iter() {
    //             self.default_setup_tasks(runner).await?;
    //
    //             let commands = setup.commands()?;
    //             self.custom_setup_tasks(runner, &commands).await?;
    //
    //             let response = {
    //                 let endpoint = format!(
    //                     "http://{}/experiment/{}/setup",
    //                     self.config.metric_server(),
    //                     experiment.id()
    //                 );
    //                 let body = json!({
    //                     "experimentId": *experiment.id(),
    //                     "runner": runner,
    //                     "commands": commands
    //                 });
    //                 self.client.post(endpoint).json(&body).send().await?
    //             };
    //             response.error_for_status_ref()?;
    //             response.json().await?;
    //         }
    //     }
    //     Ok(())
    // }
    //
    // // async fn
    // async fn custom_setup_tasks(
    //     &self,
    //     runner: &String,
    //     commands: &Vec<String>,
    // ) -> reqwest::Result<()> {
    //     let response = {
    //         let endpoint = format!("http://{}/exec", runner);
    //         let body = json!({
    //             "commands": commands
    //         });
    //         self.client.post(endpoint).json(&body).send().await?
    //     };
    //
    //     response.error_for_status_ref()?;
    //     let ret = response.json().await?;
    //     Ok(ret)
    // }
    //
    // async fn default_setup_tasks(
    //     &self,
    //     runner: &String,
    // ) -> reqwest::Result<()> {
    //     let response = {
    //         let endpoint = format!("http://{}/default_setup", runner);
    //         self.client.post(endpoint).send().await?
    //     };
    //
    //     response.error_for_status_ref();
    //     // let _ = response.json().await?;
    //     Ok(())
    // }
    // // pub fn run_experiment() -> Result<String, String> {}
}

#[allow(dead_code)]
fn get_time() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr, time::Duration};

    use super::*;
    use crate::parse::{Config, ExperimentConfig};

    fn test_path() -> PathBuf {
        PathBuf::from_str("test").expect("Failed to parse test_path")
    }

    fn generate_configs() -> (Config, ExperimentConfig) {
        let test_path = test_path();
        let config = Config::from_file(&test_path.join("test_config.toml"))
        .expect("Config failed to parse. Refer to parse::parse_config test for what went wrong");
        let experiment_config = ExperimentConfig::from_file(&test_path.join("test_experiment_config.toml"), &config)
        .expect("ExperimentConfig failed to parse. Refer to parse::parse_experiment_config test for what went wrong");

        (config, experiment_config)
    }

    fn generate_client() -> Client {
        Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .expect("Somehow an invalid client was built")
    }

    // #[tokio::test]
    // async fn runner_availability() {
    //     let (config, experiment_config) = generate_configs();
    //     let client = generate_client();
    //     let mut er = ExperimentRunner::new(&config, &experiment_config, client);
    //
    //     er.check_runners().await.unwrap();
    // }
}
