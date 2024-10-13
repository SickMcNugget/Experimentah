pub mod parse {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[derive(Deserialize, Debug)]
    pub struct Config {
        metric_server: MetricServer,
        hosts: Vec<Host>,
        runners: Vec<Runner>,
        exporters: Vec<Exporter>,
    }

    #[derive(Serialize)]
    pub enum PrometheusRequestBody<'a> {
        String(String),
        HashMap(Vec<HashMap<&'a str, String>>),
    }

    impl Config {
        pub fn metric_server(&self) -> String {
            let host_name = &self.metric_server.host;
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
            let port = &self.metric_server.port;

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
    struct MetricServer {
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

    #[derive(Deserialize, Debug)]
    pub struct Experiment {
        name: String,
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
    struct RemoteExecution {
        runners: Vec<String>,
        scripts: Vec<PathBuf>,
    }

    #[derive(Deserialize, PartialEq, Debug)]
    struct Job {
        name: String,
        runner: String,
        execute: PathBuf,
        arguments: u8,
        #[serde(default)]
        exporters: Vec<String>,
    }

    impl Job {
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
                config.metric_server,
                MetricServer {
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
    use crate::parse::{Config, Experiment, ExperimentConfig, PrometheusRequestBody};
    use reqwest::Client;
    use std::collections::HashMap;

    // The ExperimentRunner should only be used AFTER parsing has been completed!
    pub struct ExperimentRunner {
        config: Config,
        experiment_config: ExperimentConfig,
        client: Client,
    }

    impl ExperimentRunner {
        pub fn new(config: Config, experiment_config: ExperimentConfig, client: Client) -> Self {
            Self {
                config,
                experiment_config,
                client,
            }
        }

        fn run_experiments(&self) -> Result<Vec<String>, String> {
            let successful_experiments = Vec::new();

            Ok(successful_experiments)
        }

        fn run_experiment(&self, experiment: &Experiment) -> Result<String, String> {
            let experiment_id = { self.create_experiment(experiment) };

            exporters_mapping = self.configure_prometheus(experiment_id);
            Ok("yes!".to_string())
        }

        async fn create_experiment(&self, experiment: &Experiment) -> reqwest::Result<String> {
            let response = {
                let endpoint = format!("http://{}/experiment", self.config.metric_server());
                let now = get_time();
                let body = HashMap::from([("name", experiment.name()), ("timestamp_ms", &now)]);
                self.client.post(endpoint).json(&body).send().await?
            };

            response.error_for_status_ref()?;
            let ret = response.json().await?;
            Ok(ret)
        }

        async fn configure_prometheus(
            &self,
            experiment_id: String,
        ) -> reqwest::Result<HashMap<String, String>> {
            let response = {
                let endpoint = format!("http://{}/prometheus", self.config.metric_server());
                let body = HashMap::from([
                    (
                        "experiment_id",
                        PrometheusRequestBody::String(experiment_id),
                    ),
                    (
                        "exporters",
                        PrometheusRequestBody::HashMap(self.config.exporters_map()),
                    ),
                ]);
                self.client.post(endpoint).json(&body).send().await?
            };

            response.error_for_status_ref()?;
            let ret = response.json().await?;
            Ok(ret)
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
