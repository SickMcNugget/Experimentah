use serde::Deserialize;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use toml;

#[derive(Deserialize, Debug)]
struct Config {
    metric_server: MetricServer,
    hosts: Vec<Host>,
    runners: Vec<Runner>,
    exporters: Vec<Exporter>,
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

#[derive(Deserialize, PartialEq, Debug)]
struct Exporter {
    name: String,
    host: String,
    port: u16,
    kind: String,
}

#[derive(Deserialize, Debug)]
struct ExperimentConfig {
    #[serde(default = "ExperimentConfig::default_runs")]
    runs: u16,
    experiments: Vec<Experiment>,
}

impl ExperimentConfig {
    fn default_runs() -> u16 {
        1
    }

    fn validate(experiment_config: &ExperimentConfig, config: &Config) -> Result<(), String> {
        for experiment in experiment_config.experiments.iter() {
            Experiment::validate(experiment, config)?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Debug)]
struct Experiment {
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

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        panic!("Please run with 2 args: config path, experiment path");
    }

    let config_file = Path::new(&args[1]);
    let experiment_config_file = Path::new(&args[2]);

    let config: Config = {
        let config_str = fs::read_to_string(config_file).expect("Unable to read config file");
        toml::from_str(&config_str).expect("Unable to parse config toml")
    };
    let experiment_config: ExperimentConfig = {
        let experiment_config_str = fs::read_to_string(experiment_config_file)
            .expect("Unable to read experiment config file");
        toml::from_str(&experiment_config_str).expect("Unable to parse experiment config toml")
    };

    if let Err(e) = ExperimentConfig::validate(&experiment_config, &config) {
        panic!("{e}");
    }

    println!(
        "--- Config ---\n{:#?}\n--- Experiment Config ---\n{:#?}",
        config, experiment_config
    );

    // Need to validate host endpoints in the jobs for each experiment
    //  This requires comparing against config
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
