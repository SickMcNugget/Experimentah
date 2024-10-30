pub mod parse {
    use serde::ser::Error;
    use serde::{Deserialize, Serialize};
    use std::cell::{Ref, RefCell};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::{fs, io};

    #[derive(Deserialize, Debug)]
    pub struct Config {
        metrics_api: MetricsApi,
        hosts: Vec<Host>,
        runners: Vec<Runner>,
        exporters: Vec<Exporter>,
    }

    #[derive(Serialize)]
    pub enum PrometheusRequestBody<'a> {
        String(String),
        HashMap(Vec<HashMap<&'a str, String>>),
    }

    #[derive(Serialize)]
    pub enum DbSaveRequestBody {
        String(String),
        Commands(Vec<String>),
    }

    #[derive(Serialize)]
    pub enum CreateJobRequest {
        String(String),
        Exporters(Vec<String>),
        Bool(bool),
        U8(u8),
        Timestamp(u128),
    }

    #[derive(Serialize)]
    pub enum CreateExperimentRequest {
        String(String),
        Time(u128),
    }

    impl Config {
        pub fn metric_server(&self) -> String {
            let host_name = &self.metrics_api.host;
            let pos = self
                .hosts
                .iter()
                .position(|host| host.name == *host_name)
                .expect("Unable to find metric server host in config");

            let url = &self
                .hosts
                .get(pos)
                .expect("Somehow, the host was found but couldn't be retrieved from parsed Config")
                .address;
            let port = &self.metrics_api.port;

            format!("{}:{}", url, port)
        }

        pub fn runners(&self) -> Vec<String> {
            let mut urls: Vec<String> = Vec::new();
            for runner in self.runners.iter() {
                let runner_name = &runner.name;
                let pos = self
                    .runners
                    .iter()
                    .position(|runner| runner.name == *runner_name)
                    .expect("Unable to find runner host in config");
                let found_runner = &self.runners.get(pos).expect("Somehow, the runner was found but couldn't be retrieved from the parsed config.");

                urls.push(format!("{}:{}", found_runner.host, found_runner.port));
            }
            urls
        }

        pub fn exporters_map(&self) -> Vec<HashMap<&str, String>> {
            let mut exporters: Vec<HashMap<&str, String>> = Vec::new();
            for exporter in self.exporters.iter() {
                exporters.push(HashMap::from([
                    ("name", exporter.name.clone()),
                    ("host", exporter.host.clone()),
                    ("port", exporter.port.to_string()),
                    ("kind", exporter.kind.clone()),
                    ("poll_interval", 1.to_string()), // Seconds
                ]))
            }
            exporters
        }
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct MetricsApi {
        host: String,
        port: u16,
    }

    #[derive(Deserialize, PartialEq, Debug)]
    struct Host {
        name: String,
        address: String,
        infrastructure: Option<String>,
    }

    #[derive(Deserialize, PartialEq, Debug)]
    struct Runner {
        name: String,
        host: String,
        port: u16,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Exporter {
        name: String,
        host: String,
        port: u16,
        kind: String,
    }

    #[derive(Deserialize, Debug)]
    pub struct ExperimentConfig {
        #[serde(default = "ExperimentConfig::default_runs")]
        runs: u16,
        experiments: Vec<Experiment>,
    }

    impl ExperimentConfig {
        fn default_runs() -> u16 {
            1
        }

        pub fn validate(
            experiment_config: &ExperimentConfig,
            config: &Config,
        ) -> Result<(), String> {
            for experiment in experiment_config.experiments.iter() {
                Experiment::validate(experiment, config)?;
            }
            Ok(())
        }

        pub fn runs(&self) -> u16 {
            self.runs
        }

        pub fn experiments(&self) -> &Vec<Experiment> {
            &self.experiments
        }
    }

    fn default_id() -> RefCell<String> {
        RefCell::new(String::from("N/A"))
    }

    #[derive(Deserialize, Debug)]
    pub struct Experiment {
        name: String,
        #[serde(default = "default_id")]
        id: RefCell<String>,
        description: String,
        kind: String,
        #[serde(default)]
        setup: Vec<RemoteExecution>,
        #[serde(default)]
        teardown: Vec<RemoteExecution>,
        jobs: Vec<Job>,
    }

    impl Experiment {
        pub fn name(&self) -> &String {
            &self.name
        }

        pub fn kind(&self) -> &String {
            &self.kind
        }

        pub fn setup(&self) -> &Vec<RemoteExecution> {
            &self.setup
        }

        pub fn teardown(&self) -> &Vec<RemoteExecution> {
            &self.teardown
        }

        pub fn jobs(&self) -> &Vec<Job> {
            &self.jobs
        }

        pub fn id(&self) -> Ref<'_, String> {
            self.id.borrow()
        }

        pub fn allocate_id(&self, id: String) -> Result<(), &str> {
            if *self.id.borrow() != "N/A" {
                Err("Experiment ID already allocated!")
            } else {
                *self.id.borrow_mut() = id;
                Ok(())
            }
        }

        fn validate(experiment: &Experiment, config: &Config) -> Result<(), String> {
            for setup in experiment.setup.iter() {
                for runner in setup.runners.iter() {
                    if !config
                        .runners
                        .iter()
                        .any(|c_runner| c_runner.name == *runner)
                    {
                        return Err(format!("Setup runner {:?} was not found in config", runner));
                    }
                }
                check_files_exist(&setup.scripts)?;
            }

            for teardown in experiment.teardown.iter() {
                for runner in teardown.runners.iter() {
                    if !config
                        .runners
                        .iter()
                        .any(|c_runner| c_runner.name == *runner)
                    {
                        return Err(format!(
                            "Teardown runner {:?} was not found in config",
                            runner
                        ));
                    }
                }
                check_files_exist(&teardown.scripts)?;
            }

            for job in experiment.jobs.iter() {
                Job::validate(job, config)?;
            }

            Ok(())
        }
    }

    #[derive(Deserialize, Debug, PartialEq)]
    pub struct RemoteExecution {
        pub runners: Vec<String>,
        pub scripts: Vec<PathBuf>,
    }

    impl RemoteExecution {
        pub fn commands(&self) -> io::Result<Vec<String>> {
            let mut commands = Vec::new();

            for script in self.scripts.iter() {
                let script_str = fs::read_to_string(script)?;
                for mut line in script_str.lines() {
                    line = line.trim();
                    if line != "" {
                        commands.push(line.into());
                    }
                }
            }

            Ok(commands)
        }
    }

    #[derive(Deserialize, PartialEq, Debug)]
    pub struct Job {
        name: String,
        runner: String,
        execute: PathBuf,
        arguments: u8,
        #[serde(default)]
        exporters: Vec<String>,
    }

    impl Job {
        pub fn name(&self) -> &String {
            &self.name
        }

        pub fn runner(&self) -> &String {
            &self.runner
        }

        pub fn execute(&self) -> &PathBuf {
            &self.execute
        }

        pub fn arguments(&self) -> &u8 {
            &self.arguments
        }

        pub fn exporters(&self) -> &Vec<String> {
            &self.exporters
        }

        fn validate(job: &Job, config: &Config) -> Result<(), String> {
            if !config
                .runners
                .iter()
                .any(|c_runner| c_runner.name == job.runner)
            {
                return Err(format!(
                    "Job runner {:?} was not found in config",
                    job.runner
                ));
            }

            if let Err(e) = check_file_exists(&job.execute) {
                return Err(format!("Error in job execute: {}", e));
            }

            for j_exporter in job.exporters.iter() {
                if !config
                    .exporters
                    .iter()
                    .any(|c_exporter| c_exporter.name == *j_exporter)
                {
                    return Err(format!(
                        "Job exporter {:?} was not found in config",
                        j_exporter
                    ));
                }
            }

            Ok(())
        }
    }
    fn check_file_exists(file: &PathBuf) -> Result<(), String> {
        if !file.exists() {
            return Err(format!("File {:?} does not exist!", file));
        }

        Ok(())
    }

    fn check_files_exist(files: &Vec<PathBuf>) -> Result<(), String> {
        for file in files.iter() {
            check_file_exists(file)?;
        }

        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parsing() {
            let config_file = Path::new("./resources/test_config.toml");
            assert!(config_file.exists());
            let config_str = fs::read_to_string(config_file).expect("Failed to read config file");
            let config: Config = toml::from_str(&config_str).expect("Failed to parse test config");

            assert_eq!(
                config.metrics_api,
                MetricsApi {
                    host: "host1".into(),
                    port: 50000,
                },
                "Invalid metric_server for config"
            );
            assert_eq!(
                config.hosts,
                vec![
                    Host {
                        name: "host1".into(),
                        address: "my-host.com".into(),
                        infrastructure: Some("linux".into()),
                    },
                    Host {
                        name: "host2".into(),
                        address: "my-host2.com".into(),
                        infrastructure: None,
                    },
                ],
                "Invalid hosts for config"
            );
            assert_eq!(
                config.runners,
                vec![
                    Runner {
                        name: "runner1".into(),
                        host: "host1".into(),
                        port: 2000
                    },
                    Runner {
                        name: "runner2".into(),
                        host: "host2".into(),
                        port: 2001
                    },
                ],
                "Invalid runners for config"
            );
            assert_eq!(
                config.exporters,
                vec![
                    Exporter {
                        name: "test-exporter".into(),
                        host: "host1".into(),
                        port: 9100,
                        kind: "node".into()
                    },
                    Exporter {
                        name: "other-exporter".into(),
                        host: "host2".into(),
                        port: 9101,
                        kind: "temperature".into()
                    },
                ],
                "Invalid exporters for config"
            );

            let experiment_config_file = Path::new("./resources/test_experiment_config.toml");
            assert!(experiment_config_file.exists());
            let experiment_config_str = fs::read_to_string(experiment_config_file)
                .expect("Failed to read experiment config file");
            let experiment_config: ExperimentConfig =
                toml::from_str(&experiment_config_str).expect("Failed to parse experiment config");

            assert_eq!(experiment_config.runs, 1);
            assert_eq!(experiment_config.experiments.len(), 1);
            let experiment: &Experiment = &experiment_config.experiments[0];
            assert_eq!(experiment.name, "test-experiment");
            assert_eq!(experiment.description, "Just a basic test experiment");
            assert_eq!(
                experiment.setup,
                vec![RemoteExecution {
                    runners: vec!["runner1".into()],
                    scripts: vec!["test-setup.sh".into()],
                }],
            );
            assert_eq!(
                experiment.teardown,
                vec![RemoteExecution {
                    runners: vec!["runner2".into()],
                    scripts: vec!["test-teardown.sh".into()],
                }],
            );
            assert_eq!(
                experiment.jobs,
                vec![
                    Job {
                        name: "test-job".into(),
                        runner: "runner1".into(),
                        execute: "actual-work.sh".into(),
                        arguments: 0,
                        exporters: vec!["test-exporter".into(), "other-exporter".into()]
                    },
                    Job {
                        name: "test-job2".into(),
                        runner: "runner2".into(),
                        execute: "actual-work.sh".into(),
                        arguments: 0,
                        exporters: vec!["test-exporter".into()]
                    },
                    Job {
                        name: "test-job3".into(),
                        runner: "runner1".into(),
                        execute: "actual-work.sh".into(),
                        arguments: 0,
                        exporters: vec![],
                    }
                ],
            );
            assert!(ExperimentConfig::validate(&experiment_config, &config).is_ok());
        }
    }
}

pub mod run {
    use crate::parse::{
        Config, DbSaveRequestBody, Experiment, ExperimentConfig, Job, PrometheusRequestBody,
    };
    use reqwest::Client;
    use serde_json::json;
    use std::{any::Any, collections::HashMap, error::Error, hash::Hash};

    // The ExperimentRunner should only be used AFTER parsing has been completed!
    pub struct ExperimentRunner {
        config: Config,
        experiment_config: ExperimentConfig,

        client: Client,
        rt: tokio::runtime::Runtime,
    }

    impl ExperimentRunner {
        pub fn build(
            config: Config,
            experiment_config: ExperimentConfig,
            client: Client,
        ) -> std::io::Result<Self> {
            Ok(Self {
                config,
                experiment_config,
                client,
                rt: tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?,
            })
        }

        pub fn config(&self) -> &Config {
            &self.config
        }

        pub fn experiment_config(&self) -> &ExperimentConfig {
            &self.experiment_config
        }

        // pub fn new(config: Config, experiment_config: ExperimentConfig, client: Client) -> Self {
        //     Self {
        //         config,
        //         experiment_config,
        //         client,
        //
        //     }
        // }

        pub fn run_experiments(&self) -> Result<Vec<String>, String> {
            let successful_experiments = Vec::new();

            for experiment in self.experiment_config.experiments().iter() {
                self.run_experiment(experiment);
            }

            Ok(successful_experiments)
        }

        fn run_experiment(&self, experiment: &Experiment) -> Result<String, Box<dyn Error>> {
            {
                let experiment_id = self.rt.block_on(self.create_experiment(experiment))?;
                experiment.allocate_id(experiment_id)?;
            }

            let exporters_mapping = self.rt.block_on(self.configure_prometheus(experiment))?;
            // TODO(joren): add wait for prometheus ready

            self.rt.block_on(self.experiment_setup(experiment))?;

            let mut job_ids = Vec::new();
            for job in experiment.jobs().iter() {
                let job_id = self.rt.block_on(self.run_job(job, experiment));
                job_ids.push(job_id);
            }

            // exporters_mapping = self.configure_prometheus(experiment_id);
            Ok("yes!".to_string())
        }

        pub async fn check_metrics_api(&self) -> reqwest::Result<()> {
            let endpoint = format!("http://{}/status", self.config.metric_server());
            let response = self.client.get(endpoint).send().await?; // .await?;
            response.error_for_status()?;
            Ok(())
        }

        pub async fn check_runners(&self) -> reqwest::Result<()> {
            for url in self.config.runners().iter() {
                let endpoint = format!("http://{}/status", url);
                let response = self.client.get(endpoint).send().await?;
                response.error_for_status()?;
            }
            Ok(())
        }

        async fn wait_for_jobs() {}

        async fn create_job(&self, job: &Job, experiment: &Experiment) -> reqwest::Result<String> {
            let response = {
                let endpoint = format!(
                    "http://{}/experiment/{}/job",
                    self.config.metric_server(),
                    experiment.id()
                );
                let body = json!({
                    "experimentId": *experiment.id(),
                    "runnerName": job.runner(),
                    "exporters": "yummers",
                    "workload": job.name(),
                    "supplementary": "false",
                    "resultsType": experiment.kind(),
                    "arguments": job.arguments(),
                    "timestampMs": get_time(),
                });
                // let body = HashMap::from([
                //     ("experimentId", experiment.id().clone()),
                //     ("runnerName", job.runner().clone()),
                //     ("exporters", "yummers".into()),
                //     ("workload", job.name().clone()),
                //     ("supplementary", "false".into()),
                //     ("resultsType", experiment.kind().clone()),
                //     ("arguments", job.arguments().to_string()),
                //     ("timestampMs", get_time()),
                // ]);
                self.client.post(endpoint).json(&body).send().await?
            };

            response.error_for_status_ref()?;
            let ret = response.json().await?;
            Ok(ret)
        }

        async fn start_job(
            &self,
            job_id: &str,
            job: &Job,
            experiment: &Experiment,
        ) -> reqwest::Result<()> {
            let response = {
                let endpoint = format!("http://{}/job", job.runner());
                // let args_str = job.arguments().to_string();
                let body = json!({
                    "jobId": job_id,
                    "experimentId": *experiment.id(),
                    "workload": job.name(),
                    "workloadType": experiment.kind(),
                    "arguments": job.arguments()
                });
                // let body: HashMap<&str, &str> = HashMap::from([
                //     ("jobId", job_id),
                //     ("experimentId", &experiment.id()()),
                //     ("workload", job.name()),
                //     ("workloadType", experiment.kind()),
                //     ("arguments", &args_str),
                // ]);
                self.client.post(endpoint).json(&body).send().await?
            };

            response.error_for_status_ref()?;
            // let ret = response.json().await?;
            Ok(())
        }

        async fn run_job(&self, job: &Job, experiment: &Experiment) {
            let job_id = self.create_job(job, experiment).await.unwrap();
            self.start_job(&job_id, job, experiment);
        }
        // fn update_experiment(&self, experiment: &mut Experiment) {}

        async fn create_experiment(&self, experiment: &Experiment) -> reqwest::Result<String> {
            let response = {
                let endpoint = format!("http://{}/experiment", self.config.metric_server());
                let body = json!({
                    "name": experiment.name(),
                    "timestampMs": get_time()
                });
                // let body = HashMap::from([("name", experiment.name()), ("timestamp_ms", &now)]);
                self.client.post(endpoint).json(&body).send().await?
            };

            response.error_for_status_ref()?;
            let experiment_id = response.json().await?;
            // experiment
            //     .allocate_id(experiment_id)
            //     .expect("Somehow the experiment id was already allocated");

            Ok(experiment_id)
        }

        async fn configure_prometheus(
            &self,
            experiment: &Experiment,
        ) -> reqwest::Result<HashMap<String, String>> {
            let response = {
                let endpoint = format!("http://{}/prometheus", self.config.metric_server());
                let body = json!({
                    "experimentId": *experiment.id(),
                    "exporters": self.config.exporters_map()
                });
                // let body = HashMap::from([
                //     (
                //         "experiment_id",
                //         PrometheusRequestBody::String(experiment.id().clone()),
                //     ),
                //     (
                //         "exporters",
                //         PrometheusRequestBody::HashMap(self.config.exporters_map()),
                //     ),
                // ]);
                self.client.post(endpoint).json(&body).send().await?
            };

            response.error_for_status_ref()?;
            let ret = response.json().await?;
            Ok(ret)
        }

        async fn experiment_setup(&self, experiment: &Experiment) -> Result<(), Box<dyn Error>> {
            for setup in experiment.setup().iter() {
                for runner in self.config.runners().iter() {
                    self.default_setup_tasks(runner);

                    let commands = setup.commands()?;
                    self.custom_setup_tasks(runner, &commands);

                    let response = {
                        let endpoint = format!(
                            "http://{}/experiment/{}/setup",
                            self.config.metric_server(),
                            experiment.id()
                        );
                        let body = json!({
                            "experimentId": *experiment.id(),
                            "runner": runner,
                            "commands": commands
                        });
                        self.client.post(endpoint).json(&body).send().await?
                    };
                    response.error_for_status_ref();
                    let ret = response.json().await?;
                }
            }
            Ok(())
        }

        // async fn
        async fn custom_setup_tasks(
            &self,
            runner: &String,
            commands: &Vec<String>,
        ) -> reqwest::Result<()> {
            let response = {
                let endpoint = format!("http://{}/exec", runner);
                let body = json!({
                    "commands": commands
                });
                self.client.post(endpoint).json(&body).send().await?
            };

            response.error_for_status_ref()?;
            let ret = response.json().await?;
            Ok(ret)
        }

        async fn default_setup_tasks(&self, runner: &String) -> reqwest::Result<()> {
            let response = {
                let endpoint = format!("http://{}/default_setup", runner);
                self.client.post(endpoint).send().await?
            };

            response.error_for_status_ref();
            // let _ = response.json().await?;
            Ok(())
        }
        // pub fn run_experiment() -> Result<String, String> {}
    }

    fn get_time() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis()
            .to_string()
    }
}
