use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io};

#[derive(Deserialize, Debug)]
pub struct Config {
    pub brain: BrainConfig,
    pub hosts: Vec<HostConfig>,
    pub runners: Vec<RunnerConfig>,
    pub exporters: Vec<ExporterConfig>,
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
    pub fn from_toml(toml_file: &Path) -> Result<Self, String> {
        let config = match std::fs::read_to_string(toml_file) {
            Ok(config) => config,
            Err(e) => return Err(format!("Error reading config file: {e}")),
        };

        let config = match toml::from_str::<Self>(&config) {
            Ok(config) => config,
            Err(e) => return Err(format!("Error parsing config: {e}")),
        };

        Self::validate(&config)?;
        Ok(config)
    }

    pub fn brain_endpoint(&self) -> String {
        let host_name = &self.brain.host;
        let url = self
            .hosts
            .iter()
            .find(|host| host.name == *host_name)
            .map(|host| &host.address)
            .expect("Couldn't find brain host in config: {host_name}");
        let port = &self.brain.port;

        format!("http://{}:{}", url, port)
    }

    pub fn runner_endpoints(&self) -> Vec<String> {
        let mut urls: Vec<String> = Vec::with_capacity(self.runners.len());
        for runner in self.runners.iter() {
            let runner_host = &runner.host;
            let url = self
                .hosts
                .iter()
                .find(|host| host.name == *runner_host)
                .map(|host| &host.address)
                .expect("Unable to find runner host in config: {runner_host}");

            urls.push(format!("http://{}:{}", url, runner.port));
        }
        urls
    }

    pub fn validate(&self) -> Result<(), String> {
        for host in self.hosts.iter() {
            host.validate()?;
        }

        self.brain.validate(&self.hosts)?;

        for runner in self.runners.iter() {
            runner.validate(&self.hosts)?;
        }

        for exporter in self.runners.iter() {
            exporter.validate(&self.hosts)?;
        }
        Ok(())
    }

    // pub fn exporters_map(&self) -> Vec<HashMap<&str, String>> {
    //     let mut exporters: Vec<HashMap<&str, String>> =
    //         Vec::with_capacity(self.exporters.len());
    //     for exporter in self.exporters.iter() {
    //         exporters.push(HashMap::from([
    //             ("name", exporter.name.clone()),
    //             ("host", exporter.host.clone()),
    //             ("port", exporter.port.to_string()),
    //             ("kind", exporter.kind.clone()),
    //             ("poll_interval", 1.to_string()), // Seconds
    //         ]))
    //     }
    //     exporters
    // }
}

#[derive(Deserialize, Debug, PartialEq)]
struct BrainConfig {
    host: String,
    port: u16,
}

impl BrainConfig {
    fn validate(&self, hosts: &Vec<HostConfig>) -> Result<(), String> {
        hosts
            .iter()
            .find(|host| host.name == self.host)
            .ok_or(format!(
                "The host for [brain] must be defined: {}",
                self.host
            ))?;
        Ok(())
    }
}

#[derive(Deserialize, PartialEq, Debug)]
pub struct HostConfig {
    pub name: String,
    pub address: String,
    pub infrastructure: Option<String>,
}

impl HostConfig {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Deserialize, PartialEq, Debug)]
pub struct RunnerConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
}

impl RunnerConfig {
    fn validate(&self, hosts: &Vec<HostConfig>) -> Result<(), String> {
        hosts
            .iter()
            .find(|host| host.name == self.host)
            .ok_or(format!(
                "The host for runner {} must be defined: {}",
                self.name, self.host
            ))?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct ExporterConfig {
    pub name: String,
    pub host: String,
    // port: u16,
    pub command: String,
    pub setup: Vec<String>,
    #[serde(default = "ExporterConfig::default_poll_interval")]
    pub poll_interval: u16,
}

impl ExporterConfig {
    fn default_poll_interval() -> u16 {
        1
    }

    fn validate(&self, hosts: &Vec<HostConfig>) -> Result<(), String> {
        let a = shlex::split(&self.command).ok_or(format!(
            "The command for exporter {} could not be parsed: {}",
            self.name, self.command
        ))?;

        dbg!(a);

        for command in self.setup.iter() {
            shlex::split(command).ok_or(format!(
                "The setup command for exporter {} could not be parsed: {}",
                self.name, command
            ))?;
        }

        hosts
            .iter()
            .find(|host| host.name == self.host)
            .ok_or(format!(
                "The host for exporter {} must be defined: {}",
                self.name, self.host
            ))?;

        Ok(())
    }
}

#[derive(Deserialize, Debug)]
pub struct ExperimentConfig {
    // The ID is generated externally.
    // pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub execute: PathBuf,
    pub arguments: Option<u8>,
    #[serde(default = "ExperimentConfig::default_runs")]
    pub runs: u16,
    #[serde(default)]
    pub setup: Vec<RemoteExecutionConfig>,
    #[serde(default)]
    pub teardown: Vec<RemoteExecutionConfig>,
    #[serde(default)]
    pub variations: Vec<ExperimentVariation>,
    #[serde(default)]
    pub exporters: Vec<String>,
    #[serde(default)]
    pub runners: Vec<String>,
}

impl ExperimentConfig {
    pub fn from_toml(
        toml_file: &Path,
        config: &Config,
    ) -> Result<Self, String> {
        let experiment_config = match std::fs::read_to_string(toml_file) {
            Ok(experiment_config) => experiment_config,
            Err(e) => return Err(format!("Error reading config file: {e}")),
        };

        let experiment_config = match toml::from_str::<Self>(&experiment_config)
        {
            Ok(experiment_config) => experiment_config,
            Err(e) => return Err(format!("Error parsing config: {e}")),
        };

        Self::validate(&experiment_config, config)?;
        Ok(experiment_config)
    }

    fn default_runs() -> u16 {
        1
    }

    // pub fn set_id(&mut self, id: String) -> Result<(), String> {
    //     if self.id.is_some() {
    //         Err("Experiment ID already allocated!".into())
    //     } else {
    //         self.id = Some(id);
    //         Ok(())
    //     }
    // }

    pub fn validate(&self, config: &Config) -> Result<(), String> {
        check_file_exists(&self.execute)?;

        for setup in self.setup.iter() {
            if let Err(e) = setup.validate(config) {
                return Err(format!("Error parsing setup: {e}"));
            }
        }

        for variation in self.variations.iter() {
            if let Err(e) = variation.validate(config) {
                return Err(format!("Error parsing variation: {e}"));
            }

            if self.runners.is_empty() && variation.runners.is_empty() {
                return Err(format!(
                    "No runners have been defined for this experiment!"
                ));
            }

            if self.arguments.is_none() && variation.arguments.is_none() {
                return Err(format!(
                    "No arguments have been defined for this experiment!"
                ));
            }
        }

        for teardown in self.teardown.iter() {
            if let Err(e) = teardown.validate(config) {
                return Err(format!("Error parsing teardown: {e}"));
            }
        }

        for exporter in self.exporters.iter() {
            if !config
                .runners
                .iter()
                .any(|c_exporter| c_exporter.name == *exporter)
            {
                return Err(format!(
                    "Exporter {:?} was not found in config",
                    exporter
                ));
            }
        }

        Ok(())
    }
}

/// Each variation of an experiment is able to override the top level of the
/// experiment configuration. Runners will override the runners used for the
/// specific variation, and exporters will do the same for exporters. It is
/// an error to not define at least one runner in either the top-level or
/// in the variation.
#[derive(Deserialize, Debug, PartialEq)]
pub struct ExperimentVariation {
    pub name: String,
    #[serde(default)]
    pub runners: Vec<String>,
    pub arguments: Option<u8>,
    #[serde(default)]
    pub exporters: Vec<String>,
}

impl ExperimentVariation {
    pub fn validate(&self, config: &Config) -> Result<(), String> {
        for runner in self.runners.iter() {
            if !config
                .runners
                .iter()
                .any(|c_runner| c_runner.name == *runner)
            {
                return Err(format!(
                    "Runner {:?} was not found in config",
                    runner
                ));
            }
        }

        for exporter in self.exporters.iter() {
            if !config
                .exporters
                .iter()
                .any(|c_exporter| c_exporter.name == *exporter)
            {
                return Err(format!(
                    "Exporter {:?} was not found in config",
                    exporter
                ));
            }
        }

        Ok(())
    }
}

#[derive(Deserialize, Debug, PartialEq)]
pub struct RemoteExecutionConfig {
    pub runners: Vec<String>,
    pub scripts: Vec<PathBuf>,
}

impl RemoteExecutionConfig {
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

    pub fn validate(&self, config: &Config) -> Result<(), String> {
        for runner in self.runners.iter() {
            if !config
                .runners
                .iter()
                .any(|c_runner| c_runner.name == *runner)
            {
                return Err(format!(
                    "Runner {:?} was not found in config",
                    runner
                ));
            }
        }
        check_files_exist(&self.scripts)?;
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
    use std::path::Path;

    #[test]
    fn parsing() {
        let config_file = Path::new("./resources/test_config.toml");
        assert!(config_file.exists());
        let config_str = fs::read_to_string(config_file)
            .expect("Failed to read config file");
        let config: Config =
            toml::from_str(&config_str).expect("Failed to parse test config");

        assert_eq!(
            config.brain,
            BrainConfig {
                host: "host1".into(),
                port: 50000,
            },
            "Invalid metric_server for config"
        );
        assert_eq!(
            config.hosts,
            vec![
                HostConfig {
                    name: "host1".into(),
                    address: "my-host.com".into(),
                    infrastructure: Some("linux".into()),
                },
                HostConfig {
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
                RunnerConfig {
                    name: "runner1".into(),
                    host: "host1".into(),
                    port: 2000
                },
                RunnerConfig {
                    name: "runner2".into(),
                    host: "host2".into(),
                    port: 2001
                },
            ],
            "Invalid runners for config"
        );
        assert_eq!(
            config.runners,
            vec![
                ExporterConfig {
                    name: "test-exporter".into(),
                    host: "host1".into(),
                    port: 9100,
                    kind: "node".into()
                },
                ExporterConfig {
                    name: "other-exporter".into(),
                    host: "host2".into(),
                    port: 9101,
                    kind: "temperature".into()
                },
            ],
            "Invalid exporters for config"
        );

        let experiment_config_file =
            Path::new("./resources/test_experiment_config.toml");
        assert!(experiment_config_file.exists());
        let experiment_config_str = fs::read_to_string(experiment_config_file)
            .expect("Failed to read experiment config file");
        let experiment_config: ExperimentConfig =
            toml::from_str(&experiment_config_str)
                .expect("Failed to parse experiment config");

        assert_eq!(experiment_config.runs, 1);
        assert_eq!(experiment_config.variations.len(), 1);
        assert_eq!(experiment_config.name, "test-experiment");
        assert_eq!(
            experiment_config.description,
            "Just a basic test experiment"
        );
        assert_eq!(
            experiment_config.setup,
            vec![RemoteExecutionConfig {
                runners: vec!["runner1".into()],
                scripts: vec!["test-setup.sh".into()],
            }],
        );
        assert_eq!(
            experiment_config.teardown,
            vec![RemoteExecutionConfig {
                runners: vec!["runner2".into()],
                scripts: vec!["test-teardown.sh".into()],
            }],
        );
        assert_eq!(
            experiment_config.variations,
            vec![ExperimentVariation {
                name: "test-job".into(),
                runners: vec!["runner1".into()],
                arguments: 1,
                exporters: vec![
                    "test-exporter".into(),
                    "other-exporter".into()
                ]
            },],
        );
        assert!(ExperimentConfig::validate(&experiment_config, &config).is_ok());
    }
}
