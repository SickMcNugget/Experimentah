use crate::parse::{Config, ExperimentConfig, RemoteExecutionConfig};

use reqwest::Client;
use serde_json::json;
use std::process::Command;
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug)]
pub struct Experiment<'a> {
    pub id: Option<String>,
    pub name: String,
    pub description: &'a str,
    pub kind: &'a str,
    pub setup: Vec<RemoteExecution<'a>>,
    pub teardown: Vec<RemoteExecution<'a>>,
    pub runners: Vec<Runner<'a>>,
    pub execute: &'a Path,
    pub arguments: &'a u8,
    pub exporters: Vec<Exporter<'a>>,
}

impl<'a> Experiment<'a> {
    fn new(
        name: String,
        description: &'a str,
        kind: &'a String,
        setup: Vec<RemoteExecution<'a>>,
        teardown: Vec<RemoteExecution<'a>>,
        runners: Vec<Runner<'a>>,
        execute: &'a PathBuf,
        arguments: &'a u8,
        exporters: Vec<Exporter<'a>>,
    ) -> Self {
        Self {
            id: None,
            name,
            description,
            kind,
            setup,
            teardown,
            runners,
            execute,
            arguments,
            exporters,
        }
    }
}

#[derive(Debug, Hash, Eq, PartialEq)]
pub struct Runner<'a> {
    pub name: &'a str,
    pub address: &'a str,
    pub port: &'a u16,
}

impl<'a> Runner<'a> {
    fn new(name: &'a str, address: &'a str, port: &'a u16) -> Self {
        Self {
            name,
            address,
            port,
        }
    }

    fn url(&self) -> String {
        format!("http://{}:{}", self.address, self.port)
    }
}

#[derive(Debug)]
pub struct Exporter<'a> {
    pub name: &'a str,
    pub address: &'a str,
    pub command: &'a str,
    pub setup: Vec<&'a str>,
    pub poll_interval: &'a u16,
}

impl<'a> Exporter<'a> {
    fn new(
        name: &'a str,
        address: &'a str,
        command: &'a str,
        setup: &'a Vec<String>,
        poll_interval: &'a u16,
    ) -> Self {
        let mapped_setup = setup
            .iter()
            .map(|command| command.as_str())
            .collect::<Vec<&'a str>>();

        Self {
            name,
            address,
            command,
            setup: mapped_setup,
            poll_interval,
        }
    }
}
#[derive(Debug)]
pub struct RemoteExecution<'a> {
    pub runners: Vec<Runner<'a>>,
    pub scripts: Vec<&'a Path>,
}

impl<'a> RemoteExecution<'a> {
    fn new(runners: Vec<Runner<'a>>, scripts: &'a Vec<PathBuf>) -> Self {
        let mapped_scripts =
            scripts.iter().map(|script| script.as_path()).collect();
        Self {
            runners,
            scripts: mapped_scripts,
        }
    }
}

// #[derive(Debug)]
// pub struct ExporterConfig {
//     name: String,
//     host: String,
//     // port: u16,
//     command: String,
//     setup: Vec<String>,
//     #[serde(default = "ExporterConfig::default_poll_interval")]
//     poll_interval: u16,
// }

// The ExperimentRunner should only be used AFTER parsing has been completed!
pub struct ExperimentRunner<'a> {
    // pub config: Arc<Config>,
    // pub experiment_config: Arc<ExperimentConfig>,
    pub experiments: Vec<Experiment<'a>>,

    client: Arc<Client>,
}

impl<'a> ExperimentRunner<'a> {
    pub fn new(
        config: &'a Config,
        experiment_config: &'a ExperimentConfig,
        client: Arc<Client>,
    ) -> Self {
        let experiments =
            ExperimentRunner::experiments(config, experiment_config);

        Self {
            experiments,
            client,
        }
    }

    fn map_runners(
        config: &'a Config,
        runners: &'a Vec<String>,
    ) -> Vec<Runner<'a>> {
        let mut mapped_runners = Vec::with_capacity(runners.len());
        for c_runner in config.runners.iter() {
            for runner in runners.iter() {
                if c_runner.name == *runner {
                    let host = config.hosts.iter().find(|host| host.name == c_runner.host).expect("We should not fail to cross-reference at this point.");
                    mapped_runners.push(Runner::new(
                        &c_runner.name,
                        &host.address,
                        &c_runner.port,
                    ));
                }
            }
        }
        mapped_runners
    }

    fn map_exporters(
        config: &'a Config,
        exporters: &'a [String],
    ) -> Vec<Exporter<'a>> {
        let mut mapped_exporters = Vec::with_capacity(exporters.len());
        for c_exporter in config.exporters.iter() {
            for exporter in exporters.iter() {
                if c_exporter.name == *exporter {
                    let host = config.hosts.iter().find(|host| host.name == c_exporter.host).expect("We should not fail to cross-reference at this point.");
                    mapped_exporters.push(Exporter::new(
                        &c_exporter.name,
                        &host.address,
                        &c_exporter.command,
                        &c_exporter.setup,
                        &c_exporter.poll_interval,
                    ));
                }
            }
        }
        mapped_exporters
    }

    fn map_remote_executions(
        config: &'a Config,
        remote_executions: &'a [RemoteExecutionConfig],
    ) -> Vec<RemoteExecution<'a>> {
        let mut mapped_remote_executions =
            Vec::with_capacity(remote_executions.len());
        for remote_execution in remote_executions.iter() {
            let mut mapped_runners =
                Vec::with_capacity(remote_execution.runners.len());
            for c_runner in config.runners.iter() {
                for runner in remote_execution.runners.iter() {
                    if c_runner.name == *runner {
                        let host = config.hosts.iter().find(|host| host.name == c_runner.host).expect("We should not fail to cross-reference at this point.");
                        mapped_runners.push(Runner::new(
                            &c_runner.name,
                            &host.address,
                            &c_runner.port,
                        ));
                    }
                }
            }
            mapped_remote_executions.push(RemoteExecution::new(
                mapped_runners,
                &remote_execution.scripts,
            ));
        }
        mapped_remote_executions
    }

    // pub name: String,
    // pub description: String,
    // pub kind: String,
    // pub execute: PathBuf,
    // pub arguments: Option<u8>,
    // #[serde(default = "ExperimentConfig::default_runs")]
    // pub runs: u16,
    // #[serde(default)]
    // pub setup: Vec<RemoteExecution>,
    // #[serde(default)]
    // pub teardown: Vec<RemoteExecution>,
    // #[serde(default)]
    // pub variations: Vec<ExperimentVariation>,
    // #[serde(default)]
    // pub exporters: Vec<String>,
    // #[serde(default)]
    // pub runners: Vec<String>,

    // An experiment is constructed from the experiment configuration.
    // It's easier to construct it with a function because there isn't a one-to-one
    // mapping from an experiment variation to an experiment. There's an override system
    // that makes creating experiments easier on the user end. We handle all the cases
    // starting from this function.
    pub fn experiments(
        config: &'a Config,
        experiment_config: &'a ExperimentConfig,
    ) -> Vec<Experiment<'a>> {
        let name = &experiment_config.name;
        let description = &experiment_config.description;
        let kind = &experiment_config.kind;
        let execute = &experiment_config.execute;
        let arguments = &experiment_config.arguments;
        let setup = &experiment_config.setup;
        let teardown = &experiment_config.teardown;
        let variations = &experiment_config.variations;
        let exporters = &experiment_config.exporters;
        let runners = &experiment_config.runners;

        let with_base: bool = !runners.is_empty() && arguments.is_some();
        let num_experiments = if with_base {
            variations.len() + 1
        } else {
            variations.len()
        };

        let mut experiments = Vec::with_capacity(num_experiments);
        if with_base {
            experiments.push(Experiment::new(
                name.clone(),
                description,
                kind,
                ExperimentRunner::map_remote_executions(config, setup),
                ExperimentRunner::map_remote_executions(config, teardown),
                ExperimentRunner::map_runners(config, runners),
                execute,
                arguments.as_ref().unwrap(),
                ExperimentRunner::map_exporters(config, exporters),
            ));
        }

        for variation in experiment_config.variations.iter() {
            let name = format!("{name}{}", variation.name);
            let arguments = variation
                .arguments
                .as_ref()
                .or(experiment_config.arguments.as_ref());
            let runners = if !variation.runners.is_empty() {
                &variation.runners
            } else {
                &experiment_config.runners
            };
            let exporters = if !variation.exporters.is_empty() {
                &variation.exporters
            } else {
                &experiment_config.exporters
            };
            experiments.push(Experiment::new(
                name,
                description,
                kind,
                ExperimentRunner::map_remote_executions(config, setup),
                ExperimentRunner::map_remote_executions(config, teardown),
                ExperimentRunner::map_runners(config, runners),
                execute,
                arguments.unwrap(),
                ExperimentRunner::map_exporters(config, exporters),
            ));
        }
        experiments
    }

    fn unique_runners(&self) -> HashSet<&Runner> {
        let mut unique_runners = HashSet::new();
        for experiment in self.experiments.iter() {
            for runner in experiment.runners.iter() {
                unique_runners.insert(runner);
            }
        }

        unique_runners
    }

    pub async fn prep_runners(&self) {
        let unique_runners = self.unique_runners();
        for runner in unique_runners.into_iter() {
            match self.client.get(runner.url() + "/status").send().await {
                // A bad status code means we have a problem in the runner/malformed request
                Ok(response) => match response.status() {
                    reqwest::StatusCode::OK => {
                        println!("Runner was available at {}", response.url());
                    }
                    _ => {
                        eprintln!(
                            "Reaching runner at {} failed with error: {}",
                            response.url(),
                            response.status()
                        );
                    }
                },
                // Failure to connect prompts us to send over and start the runner
                Err(e) => {
                    if e.is_connect() {
                        ExperimentRunner::distribute_runner(
                            runner.address,
                            runner.port,
                        );
                    } else {
                        eprintln!("Unexpected error during prep_runners: {e}");
                    }
                }
            }
        }

        // let mut promises = Vec::with_capacity(unique_runners.len());
        // for runner in self.unique_runners().iter() {
        //     self.client.get(runner.url() + "/status").send().await;
        // }

        // for experiment in self.experiments.iter() {
        //     for runner in experiment.unique.iter() {
        //         self.client.get(runner.url() + "/status").send().await;
        //     }
        // }
        // for experiment in self.experiments() {
        //     for runner in experiment.runners {
        //         runner.
        //     }
        // }
        // for variation in experiment_config.experiments()
        // let experiment
    }

    // We assume the runner is in the current directory and simply called 'runner'
    // A key aspect of experimentah is the use of advisory locks. These
    fn distribute_runner(address: &str, port: &u16) {
        let runner_binary = Path::new("./runner");

        assert!(
            runner_binary.exists(),
            "Runner binary does not exist in current directory!"
        );

        if address == "localhost" {
            match Command::new(runner_binary).output() {
                Ok(..) => {
                    println!("Successfully executed")
                }
                Err(e) => {
                    eprintln!("Unable to execute runner: {e}");
                }
            }
        }
        match Command::new("ssh").arg(address).output() {
            Ok(result) => {
                println!("{}", result.status)
            }
            Err(e) => eprintln!("Error executing command: {e}"),
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

fn get_time() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis()
        .to_string()
}
