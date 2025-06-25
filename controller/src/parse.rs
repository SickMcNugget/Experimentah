use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{fs, io};

type Result<T> = std::result::Result<T, ParseError>;

#[derive(Debug)]
pub enum ParseError {
    ParseError {
        message: String,
        source: Box<ParseError>,
    },
    IOError {
        message: String,
        source: std::io::Error,
    },
    DeserializeError {
        message: String,
        source: toml::de::Error,
    },
    ValidationError(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            ParseError::IOError {
                ref message,
                ref source,
            } => write!(f, "{message}: {source}"),

            ParseError::DeserializeError {
                ref message,
                ref source,
            } => write!(f, "{message}: {source}"),

            ParseError::ParseError {
                ref message,
                ref source,
            } => write!(f, "{message}: {source}"),
            ParseError::ValidationError(ref message) => {
                write!(f, "Validation error: {message}")
            }
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::ParseError { ref source, .. } => Some(source),
            Self::IOError { ref source, .. } => Some(source),
            Self::DeserializeError { ref source, .. } => Some(source),
            Self::ValidationError(_) => None,
        }
    }
}

impl From<(String, std::io::Error)> for ParseError {
    fn from(value: (String, std::io::Error)) -> Self {
        ParseError::IOError {
            message: value.0,
            source: value.1,
        }
    }
}

impl From<(&str, std::io::Error)> for ParseError {
    fn from(value: (&str, std::io::Error)) -> Self {
        ParseError::IOError {
            message: value.0.to_owned(),
            source: value.1,
        }
    }
}

impl From<(&str, toml::de::Error)> for ParseError {
    fn from(value: (&str, toml::de::Error)) -> Self {
        ParseError::DeserializeError {
            message: value.0.to_owned(),
            source: value.1,
        }
    }
}

impl From<(&str, ParseError)> for ParseError {
    fn from(value: (&str, ParseError)) -> Self {
        ParseError::ParseError {
            message: value.0.to_string(),
            source: Box::new(value.1),
        }
    }
}

impl From<(String, ParseError)> for ParseError {
    fn from(value: (String, ParseError)) -> Self {
        ParseError::ParseError {
            message: value.0,
            source: Box::new(value.1),
        }
    }
}

#[derive(Deserialize, Debug, PartialEq)]
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

impl FromStr for Config {
    type Err = ParseError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse_toml(s)
    }
}

impl Config {
    pub fn from_file(file: &Path) -> Result<Self> {
        let config_str = std::fs::read_to_string(file)
            .map_err(|e| ParseError::from(("error reading config file", e)))?;
        Config::parse_toml(&config_str).map_err(|e| {
            ParseError::from((
                format!("Error in config file {}", file.to_string_lossy()),
                e,
            ))
        })
    }

    fn parse_toml(toml: &str) -> Result<Self> {
        // let config = toml::from_str::<Self>(toml)
        //     .map_err(|e| format!("Error parsing config: {e}"))?;
        let config = toml::from_str::<Self>(toml)
            .map_err(|e| ParseError::from(("Error parsing config toml", e)))?;

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

    fn validate(&self) -> Result<()> {
        for host in self.hosts.iter() {
            host.validate()?;
        }

        self.brain.validate(&self.hosts)?;

        for runner in self.runners.iter() {
            runner.validate(&self.hosts)?;
        }

        for exporter in self.exporters.iter() {
            exporter.validate(&self.hosts)?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Debug, PartialEq)]
pub struct BrainConfig {
    pub host: String,
    pub port: u16,
}

impl BrainConfig {
    fn validate(&self, hosts: &[ HostConfig ]) -> Result<()> {
        hosts.iter().find(|host| host.name == self.host).ok_or(
            ParseError::ValidationError(format!(
                "The host for [brain] must be defined: {}",
                self.host
            )),
        )?;
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
    fn validate(&self) -> Result<()> {
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
    fn validate(&self, hosts: &[ HostConfig ]) -> Result<()> {
        hosts.iter().find(|host| host.name == self.host).ok_or(
            ParseError::ValidationError(format!(
                "The host for runner {} must be defined: {}",
                self.name, self.host
            )),
        )?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct ExporterConfig {
    pub name: String,
    pub host: String,
    pub command: String,
    #[serde(default)]
    pub setup: Vec<String>,
    #[serde(default = "ExporterConfig::default_poll_interval")]
    pub poll_interval: u16,
}

impl ExporterConfig {
    fn default_poll_interval() -> u16 {
        1
    }

    fn validate(&self, hosts: &[HostConfig]) -> Result<()> {
        let a = shlex::split(&self.command).ok_or(
            ParseError::ValidationError(format!(
                "Invalid command for exporter {}: {}",
                self.name, self.command
            )),
        )?;

        dbg!(a);

        for command in self.setup.iter() {
            shlex::split(command).ok_or(ParseError::ValidationError(
                format!(
                    "Invalid setup command for exporter {}: {}",
                    self.name, command
                ),
            ))?;
        }

        hosts.iter().find(|host| host.name == self.host).ok_or(
            ParseError::ValidationError(format!(
                "Invalid host definition for exporter {}: {}",
                self.name, self.host
            )),
        )?;

        Ok(())
    }
}

#[derive(Deserialize, Debug, PartialEq)]
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
    pub fn from_file(file: &Path, config: &Config) -> Result<Self> {
        let experiment_config_str =
            std::fs::read_to_string(file).map_err(|e| {
                ParseError::from(("error reading experiment config file", e))
            })?;

        ExperimentConfig::parse_toml(&experiment_config_str, config).map_err(
            |e| {
                ParseError::from((
                    format!(
                        "Error in experiment config file {}",
                        file.to_string_lossy()
                    ),
                    e,
                ))
            },
        )
    }

    fn parse_toml(toml: &str, config: &Config) -> Result<Self> {
        let experiment_config = toml::from_str::<Self>(toml).map_err(|e| {
            ParseError::from(("Error parsing experiment config", e))
        })?;

        Self::validate(&experiment_config, config)?;
        Ok(experiment_config)
    }

    pub fn from_str(string: &str, config: &Config) -> Result<Self> {
        Self::parse_toml(string, config)
    }

    fn default_runs() -> u16 {
        1
    }

    pub fn validate(&self, config: &Config) -> Result<()> {
        check_file_exists(&self.execute)?;

        for setup in self.setup.iter() {
            setup.validate(config).map_err(|e| {
                ParseError::from((
                    format!("Invalid setup for experiment {}", self.name),
                    e,
                ))
            })?;
        }

        for variation in self.variations.iter() {
            variation.validate(config).map_err(|e| {
                ParseError::from((
                    format!("Invalid variation for experiment {}", self.name),
                    e,
                ))
            })?;

            if self.runners.is_empty() && variation.runners.is_empty() {
                return Err(ParseError::ValidationError(format!(
                    "No runners have been defined for experiment {}",
                    self.name
                )));
            }

            if self.arguments.is_none() && variation.arguments.is_none() {
                return Err(ParseError::ValidationError(format!(
                    "No arguments have been defined for experiment {}",
                    self.name
                )));
            }
        }

        for teardown in self.teardown.iter() {
            teardown.validate(config).map_err(|e| {
                ParseError::from((
                    format!("Invalid teardown for experiment {}", self.name),
                    e,
                ))
            })?;
        }

        for exporter in self.exporters.iter() {
            if !config
                .exporters
                .iter()
                .any(|c_exporter| c_exporter.name == *exporter)
            {
                Err(ParseError::ValidationError(format!(
                    "Invalid exporter for experiment {}",
                    self.name
                )))?;
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
    pub fn validate(&self, config: &Config) -> Result<()> {
        for runner in self.runners.iter() {
            if !config
                .runners
                .iter()
                .any(|c_runner| c_runner.name == *runner)
            {
                Err(ParseError::ValidationError(format!(
                    "Invalid runner definition for variation {}: {}",
                    self.name, runner
                )))?;
            }
        }

        for exporter in self.exporters.iter() {
            if !config
                .exporters
                .iter()
                .any(|c_exporter| c_exporter.name == *exporter)
            {
                return Err(ParseError::ValidationError(format!(
                    "Invalid exporter definition for variation {}: {}",
                    self.name, exporter
                )));
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
                if !line.is_empty() {
                    commands.push(line.into());
                }
            }
        }

        Ok(commands)
    }

    pub fn validate(&self, config: &Config) -> Result<()> {
        for runner in self.runners.iter() {
            if !config
                .runners
                .iter()
                .any(|c_runner| c_runner.name == *runner)
            {
                Err(ParseError::ValidationError(format!(
                    "Invalid runner definition: {runner}"
                )))?;
            }
        }
        check_files_exist(&self.scripts)?;
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
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

#[derive(Debug, Hash, Eq, PartialEq)]
pub struct Runner<'a> {
    pub name: &'a str,
    pub address: &'a str,
    pub port: &'a u16,
}

impl<'a> Runner<'a> {
    pub fn new(name: &'a str, address: &'a str, port: &'a u16) -> Self {
        Self {
            name,
            address,
            port,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}:{}", self.address, self.port)
    }
}

#[derive(Debug, PartialEq)]
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
        setup: &'a [String],
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
#[derive(Debug, PartialEq)]
pub struct RemoteExecution<'a> {
    pub runners: Vec<Runner<'a>>,
    pub scripts: Vec<&'a Path>,
}

impl<'a> RemoteExecution<'a> {
    fn new(runners: Vec<Runner<'a>>, scripts: &'a [PathBuf]) -> Self {
        let mapped_scripts =
            scripts.iter().map(|script| script.as_path()).collect();
        Self {
            runners,
            scripts: mapped_scripts,
        }
    }
}

fn map_runners<'a>(
    config: &'a Config,
    runners: &'a [ String ],
) -> Vec<Runner<'a>> {
    let mut mapped_runners = Vec::with_capacity(runners.len());
    for c_runner in config.runners.iter() {
        for runner in runners.iter() {
            if c_runner.name == *runner {
                let host = config
                    .hosts
                    .iter()
                    .find(|host| host.name == c_runner.host)
                    .expect(
                        "We should not fail to cross-reference at this point.",
                    );
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

fn map_exporters<'a>(
    config: &'a Config,
    exporters: &'a [String],
) -> Vec<Exporter<'a>> {
    let mut mapped_exporters = Vec::with_capacity(exporters.len());
    for c_exporter in config.exporters.iter() {
        for exporter in exporters.iter() {
            if c_exporter.name == *exporter {
                let host = config
                    .hosts
                    .iter()
                    .find(|host| host.name == c_exporter.host)
                    .expect(
                        "We should not fail to cross-reference at this point.",
                    );
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

fn map_remote_executions<'a>(
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

pub fn to_experiments<'a>(
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
        experiments.push(Experiment {
            id: None,
            name: name.clone(),
            description,
            kind,
            setup: map_remote_executions(config, setup),
            teardown: map_remote_executions(config, teardown),
            runners: map_runners(config, runners),
            execute,
            arguments: arguments.as_ref().unwrap(),
            exporters: map_exporters(config, exporters),
        });
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
        experiments.push(Experiment {
            id: None,
            name,
            description,
            kind,
            setup: map_remote_executions(config, setup),
            teardown: map_remote_executions(config, teardown),
            runners: map_runners(config, runners),
            execute,
            arguments: arguments.unwrap(),
            exporters: map_exporters(config, exporters),
    });
    }
    experiments
}

fn check_file_exists(file: &Path) -> Result<()> {
    let exists = file.try_exists().map_err(|e| {
        ParseError::from((
            format!(
                "Unable to verify if file {} exists",
                file.to_string_lossy()
            ),
            e,
        ))
    })?;

    match exists {
        true => Ok(()),
        false => Err(ParseError::from((
            "File {} does not exist",
            std::io::Error::new(std::io::ErrorKind::NotFound, ""),
        ))),
    }
}

fn check_files_exist(files: &[ PathBuf ]) -> Result<()> {
    for file in files.iter() {
        check_file_exists(file)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::str::FromStr;

    fn test_path() -> PathBuf {
        PathBuf::from_str("test").expect("Failed to parse test_path")
    }

    #[test]
    fn parse_config() {
        let config =
            match Config::from_file(&test_path().join("test_config.toml")) {
                Ok(config) => config,
                Err(e) => panic!("A valid config failed to be validated: {e}"),
            };

        let expected_config = Config {
            hosts: vec![HostConfig {
                name: "localhost".to_string(),
                address: "localhost".to_string(),
                infrastructure: Some("linux".to_string()),
            }],
            brain: BrainConfig {
                host: "localhost".to_string(),
                port: 50000,
            },
            runners: vec![RunnerConfig {
                name: "runner1".to_string(),
                host: "localhost".to_owned(),
                port: 50001,
            }],
            exporters: vec![
                ExporterConfig {
                    name: "test-exporter".to_string(),
                    host: "localhost".to_owned(),
                    command: "sar -o collection.bin".to_string(),
                    setup: vec![
                        "dnf install -y sysstat".into(),
                        "apt install -y sysstat".into(),
                    ],
                    poll_interval: 1,
                },
                ExporterConfig {
                    name: "another-test-exporter".to_string(),
                    host: "localhost".into(),
                    command: "my_collector".into(),
                    setup: vec![],
                    poll_interval: 1,
                },
            ],
        };
        assert_eq!(
            config, expected_config,
            "Configs did not match\nActual\n{config:#?}\nExpected\n{expected_config:#?}"
        )
    }

    #[test]
    fn parse_experiment_config() {
        let config_path = test_path().join("test_config.toml");
        let experiment_config_path =
            test_path().join("test_experiment_config.toml");
        let config = match Config::from_file(&config_path) {
            Ok(config) => config,
            Err(e) => panic!("A valid config failed to be validated: {e}"),
        };

        let experiment_config =
            match ExperimentConfig::from_file(&experiment_config_path, &config)
            {
                Ok(experiment_config) => experiment_config,
                Err(e) => panic!(
                    "A valid experiment config failed to be validated: {e}"
                ),
            };

        let runners = vec!["runner1".into()];
        let expected_experiment_config = ExperimentConfig {
                name: "localhost-experiment".into(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                execute: "./scripts/actual-work.sh".into(),
                arguments: Some(1),
                exporters: vec![],
                runners: runners.clone(),
                runs: 1,
                setup: vec![RemoteExecutionConfig {
                        runners: runners.clone(),
                        scripts: vec!["./scripts/test-setup.sh".into()]
                    }
                ],
                variations: vec![ExperimentVariation {
                        name: "different arg".into(),
                        runners: vec![],
                        arguments: Some(0),
                        exporters: vec![]
                    },
                    ExperimentVariation {
                        name: "with exporter".into(),
                        runners: vec![],
                        arguments: None,
                        exporters: vec!["test-exporter".into()] 
                    },
                    ExperimentVariation {
                        name: "with multiple exporters".into(),
                        runners: vec![],
                        arguments: Some(1),
                        exporters: vec!["test-exporter".into(), "another-test-exporter".into()] 
                    }
                ],
                teardown: vec![RemoteExecutionConfig{
                        runners: runners.clone(),
                        scripts: vec!["./scripts/test-teardown.sh".into()]
                }],
            };

        assert_eq!(
            experiment_config, expected_experiment_config,
            "Experiment configs did not match\nActual\n{experiment_config:#?}\nExpected\n{expected_experiment_config:#?}"
        )
    }

    #[test]
    fn to_experiments() {
        let config_path = test_path().join("test_config.toml");
        let experiment_config_path =
            test_path().join("test_experiment_config.toml");
        let config = match Config::from_file(&config_path) {
            Ok(config) => config,
            Err(e) => panic!("A valid config failed to be validated: {e}"),
        };

        let experiment_config =
            match ExperimentConfig::from_file(&experiment_config_path, &config)
            {
                Ok(experiment_config) => experiment_config,
                Err(e) => panic!(
                    "A valid experiment config failed to be validated: {e}"
                ),
            };

        let experiments = super::to_experiments(&config, &experiment_config);
        let expected_experiments = vec![
            Experiment {
                id: None,
                name: "localhost-experiment".to_string(),
                description: "Testing the functionality of the software completely using localhost",
                kind: "localhost-result",
                setup: vec![RemoteExecution{
                runners: vec![Runner {
                name: "runner1",
                address: "localhost",
                port: &50001},
                ],
                scripts: vec![Path::new("./scripts/test-setup.sh")]}],
                teardown: vec![RemoteExecution{
                    runners: vec![Runner {
                        name: "runner1",
                        address: "localhost",
                        port: &50001
                    }],
                    scripts: vec![Path::new("./scripts/test-teardown.sh")]
                }],
                runners: vec![Runner {
                    name: "runner1",
                    address: "localhost",
                    port: &50001,
                    }],

                execute: Path::new("./scripts/actual-work.sh"),
                arguments: &1,
                exporters: vec![]
            },
            Experiment {
                id: None,
                name: "localhost-experimentdifferent arg".to_string(),
                description: "Testing the functionality of the software completely using localhost",
                kind: "localhost-result",
                setup: vec![RemoteExecution{
                runners: vec![Runner {
                name: "runner1",
                address: "localhost",
                port: &50001},
                ],
                scripts: vec![Path::new("./scripts/test-setup.sh")]}],
                teardown: vec![RemoteExecution{
                    runners: vec![Runner {
                        name: "runner1",
                        address: "localhost",
                        port: &50001
                    }],
                    scripts: vec![Path::new("./scripts/test-teardown.sh")]
                }],
                runners: vec![Runner {
                    name: "runner1",
                    address: "localhost",
                    port: &50001,
                    }],

                execute: Path::new("./scripts/actual-work.sh"),
                arguments: &0,
                exporters: vec![]
            },
            Experiment {
                id: None,
                name: "localhost-experimentwith exporter".to_string(),
                description: "Testing the functionality of the software completely using localhost",
                kind: "localhost-result",
                setup: vec![RemoteExecution{
                runners: vec![Runner {
                name: "runner1",
                address: "localhost",
                port: &50001},
                ],
                scripts: vec![Path::new("./scripts/test-setup.sh")]}],
                teardown: vec![RemoteExecution{
                    runners: vec![Runner {
                        name: "runner1",
                        address: "localhost",
                        port: &50001
                    }],
                    scripts: vec![Path::new("./scripts/test-teardown.sh")]
                }],
                runners: vec![Runner {
                    name: "runner1",
                    address: "localhost",
                    port: &50001,
                    }],

                execute: Path::new("./scripts/actual-work.sh"),
                arguments: &1,
                exporters: vec![Exporter{
                    name: "test-exporter",
                    address: "localhost",
                    command: "sar -o collection.bin",
                    setup: vec!["dnf install -y sysstat", "apt install -y sysstat"],
                    poll_interval: &1
                }]
            },
            Experiment {
                id: None,
                name: "localhost-experimentwith multiple exporters".to_string(),
                description: "Testing the functionality of the software completely using localhost",
                kind: "localhost-result",
                setup: vec![RemoteExecution{
                runners: vec![Runner {
                name: "runner1",
                address: "localhost",
                port: &50001},
                ],
                scripts: vec![Path::new("./scripts/test-setup.sh")]}],
                teardown: vec![RemoteExecution{
                    runners: vec![Runner {
                        name: "runner1",
                        address: "localhost",
                        port: &50001
                    }],
                    scripts: vec![Path::new("./scripts/test-teardown.sh")]
                }],
                runners: vec![Runner {
                    name: "runner1",
                    address: "localhost",
                    port: &50001,
                    }],

                execute: Path::new("./scripts/actual-work.sh"),
                arguments: &1,
                exporters: vec![Exporter{
                    name: "test-exporter",
                    address: "localhost",
                    command: "sar -o collection.bin",
                    setup: vec!["dnf install -y sysstat", "apt install -y sysstat"],
                    poll_interval: &1
                },
                    Exporter { 
                        name: "another-test-exporter",
                        address: "localhost".into(),
                        command: "my_collector".into(),
                        setup: vec![],
                        poll_interval: &1 
                }]
            },
        ];

        assert_eq!(experiments, expected_experiments,
            "Experiments did not match\nActual\n{experiments:#?}\nExpected\n{expected_experiments:#?}"
        )
    }
}
