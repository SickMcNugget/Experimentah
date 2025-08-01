use crate::parse::{
    generate_experiments, Config, Experiment, ExperimentConfig, ExperimentRuns,
    Exporter, Host, RemoteExecution,
};
use crate::REMOTE_DIR;

const MAX_EXPERIMENTS: usize = 32;
const RUN_POLL_SLEEP: Duration = Duration::from_millis(250);

use std::collections::VecDeque;
use std::path::{self, PathBuf};
// use reqwest::Client;
use log::{error, info};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{collections::HashSet, path::Path};
use tokio::sync::Mutex;

use crate::ssh::{
    self, close_group_sessions, connect_to_group, run_remote_command_on_group,
    Sessions,
};

type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(Debug)]
pub enum RuntimeError {
    // ReqwestError(String, reqwest::Error),
    // I know, it's hilarious
    // BadStatusCode(String, reqwest::StatusCode),
    IOError(String, std::io::Error),
    Generic(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            // RuntimeError::ReqwestError(ref message, ref source) => {
            //     write!(f, "{message}: {source}")
            // }
            // RuntimeError::BadStatusCode(ref message, ref code) => {
            //     write!(f, "{message}: Failed with status code: {code}")
            // }
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
            // RuntimeError::ReqwestError(.., ref source) => Some(source),
            RuntimeError::IOError(.., ref source) => Some(source),
            // RuntimeError::BadStatusCode(..) => None,
            RuntimeError::Generic(..) => None,
        }
    }
}

// impl From<(&str, reqwest::Error)> for RuntimeError {
//     fn from(value: (&str, reqwest::Error)) -> Self {
//         RuntimeError::ReqwestError(value.0.to_string(), value.1)
//     }
// }
//
// impl From<(String, reqwest::Error)> for RuntimeError {
//     fn from(value: (String, reqwest::Error)) -> Self {
//         RuntimeError::ReqwestError(value.0, value.1)
//     }
// }

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

// impl From<(String, reqwest::StatusCode)> for RuntimeError {
//     fn from(value: (String, reqwest::StatusCode)) -> Self {
//         RuntimeError::BadStatusCode(value.0, value.1)
//     }
// }

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
pub struct ExperimentRunner {
    // The experiment runner should be fed by a channel of work.
    // It will keep track of what it's currently doing.
    /// If current_experiment is None, then no work is being done.
    // We want a reference to the currently running experiment
    pub current_experiment: Mutex<Option<String>>,
    /// Runs start from 1. 0 Means that nothing is being worked on.
    pub current_runs: AtomicU16,
    pub current_run: AtomicU16,
    pub experiments_queue: Mutex<VecDeque<ExperimentRuns>>,
}

impl Default for ExperimentRunner {
    fn default() -> Self {
        Self {
            current_experiment: Mutex::new(None),
            current_runs: 0.into(),
            current_run: 0.into(),
            experiments_queue: Mutex::new(VecDeque::with_capacity(
                MAX_EXPERIMENTS,
            )),
        }
    }
}

impl ExperimentRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new experiment to the runner
    pub async fn enqueue(&self, experiments: ExperimentRuns) {
        self.experiments_queue.lock().await.push_back(experiments);
    }

    /// Begin the runner main loop. When experiments are enqueued,
    /// and no work is currently being done, the runner begins work
    /// on the experiment.
    pub async fn start(&self) {
        info!("Experiment runner now running..");
        loop {
            let (runs, experiments) = {
                let mut queue = self.experiments_queue.lock().await;
                if !queue.is_empty() {
                    queue.pop_front().unwrap()
                } else {
                    tokio::time::sleep(RUN_POLL_SLEEP).await;
                    continue;
                }
            };

            // We create the timestamp once for each full experiment
            // TODO(joren): Think about what you want to do with errors.
            // It might be worth having an endpoint that can retrieve errors.
            let ts = match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(ts) => ts.as_millis(),
                Err(e) => {
                    error!("Error getting the time since epoch: {e}");
                    continue;
                }
            };

            info!(
                "Experiment with {} variations received ({} repeats).",
                experiments.len(),
                runs
            );

            self.current_runs.store(runs, Ordering::Relaxed);

            for i in 1..=runs {
                self.current_run.store(i, Ordering::Relaxed);
                for experiment in experiments.iter() {
                    {
                        info!(
                            "Running experiment \"{}\" (repeat {}/{})...",
                            &experiment.name, i, runs
                        );
                        let mut current_experiment =
                            self.current_experiment.lock().await;
                        *current_experiment = Some(experiment.name.clone());
                    }

                    // TODO(joren): handle the result from connecting
                    let sessions = ssh::connect_to_hosts(&experiment.hosts())
                        .await
                        .unwrap();

                    let variation_directory =
                        Self::make_variation_directory(&sessions, ts, i).await;
                    let experiment_directory =
                        variation_directory.parent().unwrap();

                    Self::start_exporters(
                        &sessions,
                        &experiment.exporters,
                        experiment_directory,
                    )
                    .await;

                    Self::run_remote_execution(
                        &sessions,
                        &experiment.setup,
                        experiment_directory,
                    )
                    .await;

                    // ssh::upload(&sessions, source_path, destination_path)

                    // run_variation(&sessions, &experiment);
                    Self::run_remote_execution(
                        &sessions,
                        &experiment.teardown,
                        experiment_directory,
                    )
                    .await;

                    // for setup in experiment.setup.iter() {
                    //     let setup_sessions: Sessions = sessions
                    //         .iter()
                    //         .filter(|(key, _)| {
                    //             setup
                    //                 .runners
                    //                 .iter()
                    //                 .any(|runner| &runner.address == *key)
                    //         })
                    //         .map(|(key, value)| (key.clone(), value.clone()))
                    //         .collect();
                    //
                    //     for script in setup.scripts.iter() {
                    //         // TODO(joren): handle the canonicalisation error
                    //         let script_path = script.canonicalize().unwrap();
                    //         //TODO(joren): Handle error case
                    //         ssh::upload(
                    //             &setup_sessions,
                    //             &script_path,
                    //             experiment_directory,
                    //         )
                    //         .await
                    //         .unwrap();
                    //         // TODO(joren): Handle error case
                    //         let remote_script = experiment_directory
                    //             .join(script.file_name().unwrap());
                    //         ssh::run_script(&setup_sessions, &remote_script)
                    //             .await
                    //             .unwrap();
                    //     }
                    // }
                    //
                    // for teardown in experiment.teardown.iter() {
                    //     let teardown_sessions: Sessions = sessions
                    //         .iter()
                    //         .filter(|(key, _)| {
                    //             teardown
                    //                 .runners
                    //                 .iter()
                    //                 .any(|runner| &runner.address == *key)
                    //         })
                    //         .map(|(key, value)| (key.clone(), value.clone()))
                    //         .collect();
                    //
                    //     println!(
                    //         "teardown sessions: {}",
                    //         teardown_sessions.len()
                    //     );
                    //
                    //     for script in teardown.scripts.iter() {
                    //         // TODO(joren): handle the canonicalisation error
                    //         let script_path = script.canonicalize().unwrap();
                    //         //TODO(joren): Handle error case
                    //         ssh::upload(
                    //             &teardown_sessions,
                    //             &script_path,
                    //             experiment_directory,
                    //         )
                    //         .await
                    //         .unwrap();
                    //         // TODO(joren): Handle error case
                    //         let remote_script = experiment_directory
                    //             .join(script.file_name().unwrap());
                    //         ssh::run_script(&teardown_sessions, &remote_script)
                    //             .await
                    //             .unwrap();
                    //     }
                    // }

                    // let command_args = vec!["cd".to_string(), ".ssh".into()];
                    // ssh::run_command_on_sessions(&sessions, &command_args)
                    //     .await
                    //     .unwrap();
                    //
                    // let command_args = vec!["ls".to_string(), "-al".into()];
                    // // TODO(joren): handle the result from running commands
                    // ssh::run_command_on_sessions(&sessions, &command_args)
                    //     .await
                    //     .unwrap();

                    // let connections =
                    //     Experiment::connect_to_hosts(experiment.hosts())
                    // self.run_experiment();

                    // self.run_experiment(experiment).await;
                }
            }
            info!("Finished experiments");

            self.current_runs.store(0, Ordering::Relaxed);
            self.current_run.store(0, Ordering::Relaxed);
            *self.current_experiment.lock().await = None;
        }
    }

    async fn make_variation_directory(
        sessions: &Sessions,
        timestamp: u128,
        run: u16,
    ) -> PathBuf {
        let variation_directory =
            PathBuf::from(format!("{REMOTE_DIR}/{timestamp}/{run}"));

        // Create the variation directory
        // TODO(joren): handle the fail case for run_command
        ssh::run_command(
            &sessions,
            &[
                "mkdir".into(),
                "-p".into(),
                variation_directory.to_string_lossy().to_string(),
            ],
        )
        .await
        .unwrap();

        variation_directory
    }

    fn filter_host_sessions(
        sessions: &Sessions,
        hosts: &Vec<Host>,
    ) -> Sessions {
        sessions
            .into_iter()
            .filter(|(key, _)| {
                hosts.into_iter().any(|host| &host.address == *key)
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    fn filter_exporter_sessions(
        sessions: &Sessions,
        exporter: &Exporter,
    ) -> Sessions {
        sessions
            .into_iter()
            .filter(|(key, _)| {
                exporter.hosts.iter().any(|host| &host.address == *key)
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    async fn start_exporters(
        sessions: &Sessions,
        exporters: &Vec<Exporter>,
        experiment_directory: &Path,
    ) {
        // TODO(joren): Handle exporter setup
        for exporter in exporters.iter() {
            let exporter_sessions =
                Self::filter_exporter_sessions(sessions, exporter);

            //TODO(joren): Error check split
            let comm = shlex::split(&exporter.command).unwrap();
            ssh::run_command(&exporter_sessions, &comm).await;
        }
        println!("Finished running exporters");
        // for exporter in exporters.iter() {
        //     let exporter_sessions =
        //         Self::filter_exporter_sessions(sessions, exporters.address);
        //     exporter.
        // }
    }

    async fn run_remote_execution(
        sessions: &Sessions,
        re: &Vec<RemoteExecution>,
        experiment_directory: &Path,
    ) {
        for stage in re.into_iter() {
            let setup_sessions =
                Self::filter_host_sessions(sessions, &stage.hosts);
            for script in stage.scripts.iter() {
                assert!(script.exists());

                //TODO(joren): Handle error case
                ssh::upload(&setup_sessions, &script, experiment_directory)
                    .await
                    .unwrap();

                // TODO(joren): Handle error case
                let remote_script =
                    experiment_directory.join(script.file_name().unwrap());
                ssh::run_script(&setup_sessions, &remote_script)
                    .await
                    .unwrap();
            }
        }
    }

    async fn run_experiment(&self, experiment: &Experiment) {
        // todo!("Running an experiment needs to perform setup, execution, and teardown steps.");
        match connect_to_group(&experiment.hosts()).await {
            Ok((group, sftp)) => {
                info!("Successfully connected to group, {group:?}, {sftp:?}");

                run_remote_command_on_group::<fn(&String) -> Vec<String>>(
                    &"echo".to_string(),
                    &group,
                    &Some(vec!["hello".to_string()]),
                    None,
                )
                .await;

                close_group_sessions(group).await;
            }
            Err(e) => error!("Failed to connect to group, {e}"),
        }

        todo!("Running an experiment needs to perform setup, execution, and teardown steps.");
        // tokio::time::sleep(Duration::from_millis(2000)).await;
    }

    // An experiment is constructed from the experiment configuration.
    // It's easier to construct it with a function because there isn't a one-to-one
    // mapping from an experiment variation to an experiment. There's an override system
    // that makes creating experiments easier on the user end. We handle all the cases
    // starting from this function.

    fn unique_hosts(experiments: &[Experiment]) -> HashSet<&Host> {
        let mut unique_hosts = HashSet::new();
        for experiment in experiments.iter() {
            for host in experiment.hosts.iter() {
                unique_hosts.insert(host);
            }
        }
        unique_hosts
    }
}

// Checks if runners are reachable
// If any runners fail, they are returned in a vector to be activated
// later.
// pub async fn check_runners(&mut self) -> Result<()> {
//     let unique_runners =
//         ExperimentRunner::unique_runners(&self.experiments);
//
//     for runner in unique_runners.into_iter() {
//         let url = runner.url() + "/status";
//         match self.client.get(&url).send().await {
//             Ok(response) => {
//                 let status = response.status();
//                 match status {
//                     // reqwest::StatusCode::OK => {
//                         println!("Runner {} is available", runner.name)
//                     }
//                     // _ => Err(RuntimeError::BadStatusCode(
//                         format!("Error reaching runner at {}", url),
//                         status,
//                     // ))?,
//                 };
//             }
//             Err(e) => {
//                 // If we were unable to connect to the runner,
//                 // then we can try and make sure that there is a
//                 // runner running
//                 if e.is_connect() {
//                     self.subprocesses.push(ChildProcess::new(
//                         runner.address.to_string(),
//                         ExperimentRunner::distribute_runner(
//                             runner.address,
//                             runner.port,
//                         )?,
//                     ));
//                 } else {
//                     Err(RuntimeError::from((
//                         format!(
//                             "Error making request to runner {}",
//                             runner.name
//                         ),
//                         e,
//                     )))?
//                 }
//             }
//         }
//     }
//     Ok(())
// }

// We assume the runner is in the current directory and simply called 'runner'
// A key aspect of experimentah is the use of advisory locks. These
// fn distribute_runner(address: &str, _port: &u16) -> Result<Child> {
//     let runner_binary = Path::new("./runner");
//
//     dbg!(runner_binary);
//     assert!(
//         runner_binary.exists(),
//         "Runner binary does not exist in current directory!"
//     );
//
//     if address == "localhost" {
//         match Command::new(runner_binary).spawn() {
//             Ok(child) => Ok(child),
//             Err(e) => {
//                 Err(RuntimeError::from(("Unable to execute runner", e)))?
//             }
//         }
//     } else {
//         match Command::new("ssh").arg(address).spawn() {
//             Ok(child) => {
//                 Ok(child)
//                 // println!("{}", result.status)
//             }
//             Err(e) => Err(RuntimeError::from(("Error executing SSH", e)))?,
//         }
//     }
// }

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
// }

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
    // use std::{path::PathBuf, str::FromStr, time::Duration};
    //
    use super::*;

    // fn

    #[test]
    fn enqueue_experiment() {}
    // use crate::parse::{Config, ExperimentConfig};
    //
    // fn test_path() -> PathBuf {
    //     PathBuf::from_str("test").expect("Failed to parse test_path")
    // }
    //
    // fn generate_configs() -> (Config, ExperimentConfig) {
    //     let test_path = test_path();
    //     let config = Config::from_file(&test_path.join("test_config.toml"))
    //     .expect("Config failed to parse. Refer to parse::parse_config test for what went wrong");
    //     let experiment_config = ExperimentConfig::from_file(&test_path.join("test_experiment_config.toml"))
    //     .expect("ExperimentConfig failed to parse. Refer to parse::parse_experiment_config test for what went wrong");
    //
    //     (config, experiment_config)
    // }

    // fn generate_client() -> Client {
    //     Client::builder()
    //         .timeout(Duration::from_secs(1))
    //         .build()
    //         .expect("Somehow an invalid client was built")
    // }

    // #[tokio::test]
    // async fn runner_availability() {
    //     let (config, experiment_config) = generate_configs();
    //     let client = generate_client();
    //     let mut er = ExperimentRunner::new(&config, &experiment_config, client);
    //
    //     er.check_runners().await.unwrap();
    // }
}
