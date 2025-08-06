use axum::http::StatusCode;
use axum::response::IntoResponse;
use log::error;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{fs, io};

use crate::STORAGE_DIR;

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

impl ParseError {
    fn status_code(&self) -> StatusCode {
        match *self {
            ParseError::ParseError { ref source, .. } => source.status_code(),
            ParseError::IOError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ParseError::DeserializeError { .. } => StatusCode::BAD_REQUEST,
            ParseError::ValidationError(..) => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for ParseError {
    fn into_response(self) -> axum::response::Response {
        error!("{}", self);
        let body = self.to_string();
        let code = self.status_code();
        (code, body).into_response()
    }
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

impl From<std::io::Error> for ParseError {
    fn from(value: std::io::Error) -> Self {
        ParseError::IOError {
            message: "IO Error".to_string(),
            source: value,
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

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Config {
    pub hosts: Vec<HostConfig>,
    pub exporters: Vec<ExporterConfig>,
}

// #[derive(Serialize)]
// pub enum PrometheusRequestBody<'a> {
//     String(String),
//     HashMap(Vec<HashMap<&'a str, String>>),
// }
//
// #[derive(Serialize)]
// pub enum DbSaveRequestBody {
//     String(String),
//     Commands(Vec<String>),
// }
//
// #[derive(Serialize)]
// pub enum CreateJobRequest {
//     String(String),
//     Exporters(Vec<String>),
//     Bool(bool),
//     U8(u8),
//     Timestamp(u128),
// }
//
// #[derive(Serialize)]
// pub enum CreateExperimentRequest {
//     String(String),
//     Time(u128),
// }

impl FromStr for Config {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self> {
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
        let config = toml::from_str::<Self>(toml)
            .map_err(|e| ParseError::from(("Error parsing config toml", e)))?;
        Ok(config)
    }

    // pub fn brain_endpoint(&self) -> String {
    //     let host_name = &self.brain.host;
    //     let url = self
    //         .hosts
    //         .iter()
    //         .find(|host| host.name == *host_name)
    //         .map(|host| &host.address)
    //         .expect("Couldn't find brain host in config: {host_name}");
    //     let port = &self.brain.port;
    //
    //     format!("http://{}:{}", url, port)
    // }

    pub fn host_endpoints(&self) -> Vec<String> {
        self.hosts.iter().map(|host| host.address.clone()).collect()
    }

    pub fn validate(&self) -> Result<()> {
        for host in self.hosts.iter() {
            host.validate()?;
        }

        // self.brain.validate(&self.hosts)?;

        for exporter in self.exporters.iter() {
            exporter.validate(&self.hosts)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct BrainConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct HostConfig {
    pub name: String,
    pub address: String,
}

impl HostConfig {
    fn validate(&self) -> Result<()> {
        // TODO(joren): The only validation we can do is
        // check whether we have a valid address
        Ok(())
    }
}

// #[derive(Serialize, Deserialize, PartialEq, Debug)]
// pub struct RunnerConfig {
//     pub name: String,
//     pub host: String,
// }
//
// impl RunnerConfig {
//     fn validate(&self, hosts: &[HostConfig]) -> Result<()> {
//         // Ensure the hosts exist
//         hosts.iter().find(|host| host.name == self.host).ok_or(
//             ParseError::ValidationError(format!(
//                 "The host for runner {} must be defined: {}",
//                 self.name, self.host
//             )),
//         )?;
//         Ok(())
//     }
// }

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct ExporterConfig {
    pub name: String,
    pub hosts: Vec<String>,
    pub command: String,
    #[serde(default)]
    pub setup: Vec<String>,
}

impl ExporterConfig {
    fn validate(&self, hosts: &[HostConfig]) -> Result<()> {
        // Ensure we have a valid command
        shlex::split(&self.command).ok_or(ParseError::ValidationError(
            format!(
                "Invalid command for exporter {}: {}",
                self.name, self.command
            ),
        ))?;

        // Ensure we have valid setup commands
        for command in self.setup.iter() {
            shlex::split(command).ok_or(ParseError::ValidationError(
                format!(
                    "Invalid setup command for exporter {}: {}",
                    self.name, command
                ),
            ))?;
        }

        // Ensure the hosts exist
        for host in self.hosts.iter() {
            hosts.iter().find(|c_host| c_host.name == *host).ok_or(
                ParseError::ValidationError(format!(
                    "Invalid host definition for exporter {}: {}",
                    self.name, host
                )),
            )?;
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ExperimentConfig {
    // The ID is generated externally.
    // pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub execute: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
    pub expected_arguments: Option<usize>,
    #[serde(default = "ExperimentConfig::default_runs")]
    pub runs: u16,
    #[serde(default)]
    pub setup: Vec<RemoteExecutionConfig>,
    #[serde(default)]
    pub teardown: Vec<RemoteExecutionConfig>,
    #[serde(default)]
    pub variations: Vec<VariationConfig>,
    #[serde(default)]
    pub exporters: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
}

impl FromStr for ExperimentConfig {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse_toml(s)
    }
}

impl ExperimentConfig {
    pub fn from_file(file: &Path) -> Result<Self> {
        let experiment_config_str =
            std::fs::read_to_string(file).map_err(|e| {
                ParseError::from(("error reading experiment config file", e))
            })?;

        ExperimentConfig::parse_toml(&experiment_config_str).map_err(|e| {
            ParseError::from((
                format!(
                    "Error in experiment config file {}",
                    file.to_string_lossy()
                ),
                e,
            ))
        })
    }

    fn parse_toml(toml: &str) -> Result<Self> {
        let experiment_config = toml::from_str::<Self>(toml).map_err(|e| {
            ParseError::from(("Error parsing experiment config", e))
        })?;

        Ok(experiment_config)
    }

    fn default_runs() -> u16 {
        1
    }

    fn validate_arguments(
        name: &str,
        expected: Option<usize>,
        args: &[String],
    ) -> Result<()> {
        if expected.is_some_and(|a| a != args.len()) {
            Err(ParseError::ValidationError(format!(
                "Error in experiment '{}': Expected {} arguments, got {}",
                name,
                expected.unwrap(),
                args.len()
            )))?;
        }
        Ok(())
    }

    pub fn validate_files(&self) -> Result<()> {
        check_file_exists(&map_remote_execution_script(
            &self.execute,
            &RemoteExecutionType::Execute,
        )?)
        .map_err(|e| {
            ParseError::from((
                format!("Missing files for experiment '{}'", self.name),
                e,
            ))
        })?;

        for setup in self.setup.iter() {
            setup
                .validate_files(&RemoteExecutionType::Setup)
                .map_err(|e| {
                    ParseError::from((
                        format!(
                            "Missing files in setup for experiment '{}'",
                            &self.name
                        ),
                        e,
                    ))
                })?;
        }

        for teardown in self.teardown.iter() {
            teardown
                .validate_files(&RemoteExecutionType::Teardown)
                .map_err(|e| {
                    ParseError::from((
                        format!(
                            "Missing files in teardown for experiment '{}'",
                            &self.name
                        ),
                        e,
                    ))
                })?;
        }

        Ok(())
    }

    /// Validates all members of an ExperimentConfig.
    /// This cross-references with the Config passed in, to ensure that there
    /// is consistency between both of the files.
    /// Note that this function cannot check whether files exist on disk,
    /// and the validate_files function should be used instead for this functionality.
    /// Clients shouldn't worry about validating files in general, as they need to be uploaded to
    /// the controller, which will check for files
    pub fn validate(&self, config: &Config) -> Result<()> {
        if self.execute.clone().into_os_string().is_empty() {
            Err(ParseError::ValidationError(
                "The 'execute' field in an experiment config cannot be empty"
                    .to_string(),
            ))?;
        }

        for setup in self.setup.iter() {
            setup.validate(config, &self.name).map_err(|e| {
                ParseError::from((
                    format!("Invalid setup for experiment '{}'", &self.name),
                    e,
                ))
            })?;
        }

        // ExperimentConfig::validate_arguments(
        //     &self.name,
        //     self.expected_arguments,
        //     &self.arguments,
        // )?;

        hosts_check(&config.hosts, &self.hosts, &self.name)?;
        exporters_check(&config.exporters, &self.exporters, &self.name)?;

        for (i, variation) in self.variations.iter().enumerate() {
            let name = variation.name.as_ref().unwrap_or(&self.name);
            variation.validate(config, &self.name).map_err(|e| {
                ParseError::from((
                    format!(
                        "Invalid variation (index {i}) for experiment '{}'",
                        self.name
                    ),
                    e,
                ))
            })?;

            // Ensures that we have the correct number of arguments (if defined)
            ExperimentConfig::validate_arguments(
                name,
                variation.expected_arguments,
                &variation.arguments,
            )?;

            if self.hosts.is_empty() && variation.hosts.is_empty() {
                return Err(ParseError::ValidationError(format!(
                    "No hosts have been defined for experiment '{}'",
                    self.name
                )));
            }
        }

        for teardown in self.teardown.iter() {
            teardown.validate(config, &self.name).map_err(|e| {
                ParseError::from((
                    format!(
                        "Invalid teardown for experiment '{}': {:?}",
                        self.name, teardown
                    ),
                    e,
                ))
            })?;
        }

        Ok(())
    }
}

// fn

fn hosts_check(
    c_hosts: &[HostConfig],
    hosts: &[String],
    experiment_name: &str,
) -> Result<()> {
    for host in hosts.iter() {
        if !c_hosts.iter().any(|c_host| c_host.name == *host) {
            Err(ParseError::ValidationError(format!(
                "Invalid host definition for experiment '{experiment_name}': got '{host}', expected one of {:?}",
                c_hosts
                    .iter()
                    .map(|c_host| &c_host.name)
                    .collect::<Vec<&String>>()
            )))?;
        }
    }
    Ok(())
}

fn exporters_check(
    c_exporters: &[ExporterConfig],
    exporters: &[String],
    experiment_name: &str,
) -> Result<()> {
    for exporter in exporters.iter() {
        if !c_exporters
            .iter()
            .any(|c_exporter| c_exporter.name == *exporter)
        {
            Err(ParseError::ValidationError(format!(
                "Invalid exporter for experiment '{experiment_name}': got '{exporter}', expected one of {:?}",
                c_exporters.iter().map(|c_exporter| &c_exporter.name).collect::<Vec<&String>>()
            )))?;
        }
    }
    Ok(())
}

/// Each variation of an experiment is able to override the top level of the
/// experiment configuration. Runners will override the runners used for the
/// specific variation, and exporters will do the same for exporters. It is
/// an error to not define at least one runner in either the top-level or
/// in the variation.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct VariationConfig {
    pub name: Option<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    pub expected_arguments: Option<usize>,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub exporters: Vec<String>,
}

impl VariationConfig {
    pub fn validate(
        &self,
        config: &Config,
        experiment_name: &str,
    ) -> Result<()> {
        let name = match &self.name {
            Some(name) => name,
            None => &experiment_name.to_string(),
        };

        hosts_check(&config.hosts, &self.hosts, name)?;
        exporters_check(&config.exporters, &self.exporters, name)?;

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct RemoteExecutionConfig {
    pub hosts: Vec<String>,
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

    fn validate(&self, config: &Config, experiment_name: &str) -> Result<()> {
        hosts_check(&config.hosts, &self.hosts, experiment_name)
    }

    fn validate_files(
        &self,
        remote_execution_type: &RemoteExecutionType,
    ) -> Result<()> {
        let scripts =
            map_remote_execution_scripts(self, remote_execution_type)?;

        check_files_exist(&scripts).map_err(|e| {
            ParseError::from((
                format!(
                    "Missing files for {}",
                    remote_execution_type.to_string()
                ),
                e,
            ))
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct Experiment {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub setup: Vec<RemoteExecution>,
    pub teardown: Vec<RemoteExecution>,
    pub hosts: Vec<Host>,
    // Note that execute should only contain 1 script.
    // We make it a RemoteExecution for convenience, though
    pub execute: RemoteExecution,
    pub arguments: Vec<String>,
    pub expected_arguments: Option<usize>,
    pub exporters: Vec<Exporter>,
}

impl Experiment {
    pub fn hosts(&self) -> Vec<String> {
        let mut hosts: HashSet<String> = HashSet::new();
        for host in self.hosts.iter() {
            hosts.insert(host.address.clone());
        }

        for exporter in self.exporters.iter() {
            for host in exporter.hosts.iter() {
                hosts.insert(host.address.clone());
            }
        }

        for stage in self.setup.iter() {
            for host in stage.hosts.iter() {
                hosts.insert(host.address.clone());
            }
        }

        for stage in self.teardown.iter() {
            for host in stage.hosts.iter() {
                hosts.insert(host.address.clone());
            }
        }

        hosts.drain().collect::<Vec<String>>()
    }
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Host {
    pub name: String,
    pub address: String,
}

// impl Runner {
//     // pub fn new(name: &'a str, address: &'a str, port: &'a u16) -> Self {
//     //     Self {
//     //         name,
//     //         address,
//     //         port,
//     //     }
//     // }
//
//     // pub fn url(&self) -> String {
//     //     format!("http://{}:{}", self.address, self.port)
//     // }
// }

// #[derive(Debug, Hash, Eq, PartialEq)]
// pub struct Runner<'a> {
//     pub name: &'a str,
//     pub address: &'a str,
//     pub port: &'a u16,
// }
//
// impl<'a> Runner<'a> {
//     pub fn new(name: &'a str, address: &'a str, port: &'a u16) -> Self {
//         Self {
//             name,
//             address,
//             port,
//         }
//     }
//
//     pub fn url(&self) -> String {
//         format!("http://{}:{}", self.address, self.port)
//     }
// }

#[derive(Clone, Debug, PartialEq)]
pub struct Exporter {
    pub name: String,
    pub hosts: Vec<Host>,
    // pub address: String,
    pub command: String,
    pub setup: Vec<String>,
}

impl Exporter {
    fn new(
        name: String,
        hosts: Vec<Host>,
        command: String,
        setup: Vec<String>,
    ) -> Self {
        Self {
            name,
            hosts,
            command,
            setup,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteExecution {
    pub hosts: Vec<Host>,
    pub scripts: Vec<PathBuf>,
}

impl RemoteExecution {
    fn new(hosts: Vec<Host>, scripts: Vec<PathBuf>) -> Self {
        Self { hosts, scripts }
    }
}

enum RemoteExecutionType {
    Setup,
    Teardown,
    Execute,
}

impl Display for RemoteExecutionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Setup => write!(f, "setup"),
            Self::Teardown => write!(f, "teardown"),
            Self::Execute => write!(f, "execute"),
        }
    }
}

impl FromStr for RemoteExecutionType {
    type Err = ParseError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let re_type = match s {
            "setup" => Self::Setup,
            "teardown" => Self::Teardown,
            "execute" => Self::Execute,
            &_ => Err(ParseError::ValidationError(format!(
                "Invalid remote execution type, got {s}"
            )))?,
        };

        Ok(re_type)
    }
}

fn map_hosts(config: &Config, hosts: &[String]) -> Vec<Host> {
    let mapped_hosts = config
        .hosts
        .iter()
        .filter(|c_host| hosts.contains(&c_host.name))
        .map(|c_host| Host {
            name: c_host.name.clone(),
            address: c_host.address.clone(),
        })
        .collect::<Vec<Host>>();

    mapped_hosts
}

fn map_exporters(config: &Config, exporters: &[String]) -> Vec<Exporter> {
    config
        .exporters
        .iter()
        .filter(|c_exporter| {
            exporters
                .iter()
                .any(|exporter| exporter == &c_exporter.name)
        })
        .map(|c_exporter| {
            let hosts = map_hosts(config, &c_exporter.hosts);

            Exporter::new(
                c_exporter.name.to_string(),
                hosts,
                c_exporter.command.to_string(),
                c_exporter.setup.clone(),
            )
        })
        .collect()
}

fn remap_execute(
    config: &Config,
    execute: &Path,
    hosts: &[String],
) -> Result<RemoteExecution> {
    let hosts = map_hosts(config, hosts);

    // This is always one script for the execute key
    let scripts = vec![map_remote_execution_script(
        execute,
        &RemoteExecutionType::Execute,
    )?];

    Ok(RemoteExecution::new(hosts, scripts))
}

fn map_remote_executions(
    config: &Config,
    remote_executions: &[RemoteExecutionConfig],
    remote_execution_type: RemoteExecutionType,
) -> Result<Vec<RemoteExecution>> {
    let mut mapped_remote_executions =
        Vec::with_capacity(remote_executions.len());

    for remote_execution in remote_executions.iter() {
        let hosts = map_hosts(config, &remote_execution.hosts);

        let mapped_scripts = map_remote_execution_scripts(
            remote_execution,
            &remote_execution_type,
        )?;

        mapped_remote_executions
            .push(RemoteExecution::new(hosts, mapped_scripts));
    }
    Ok(mapped_remote_executions)
}

fn map_remote_execution_scripts(
    remote_execution: &RemoteExecutionConfig,
    remote_execution_type: &RemoteExecutionType,
) -> Result<Vec<PathBuf>> {
    Ok(remote_execution
        .scripts
        .iter()
        .map(|script| {
            map_remote_execution_script(script, remote_execution_type)
        })
        .collect::<Result<Vec<PathBuf>>>()?)
}

fn map_remote_execution_script(
    script: &Path,
    remote_execution_type: &RemoteExecutionType,
) -> Result<PathBuf> {
    let basename = script.file_name().expect(
        format!("The script {:?} should have had a filename", script).as_str(),
    );
    let new_path =
        PathBuf::from(format!("{STORAGE_DIR}/{remote_execution_type}"))
            .join(basename);
    Ok(std::path::absolute(new_path)?)
}

pub type ExperimentRuns = (u16, Vec<Experiment>);

pub fn generate_experiments(
    config: &Config,
    experiment_config: &ExperimentConfig,
) -> Result<ExperimentRuns> {
    let name = &experiment_config.name;
    let description = &experiment_config.description;
    let kind = &experiment_config.kind;
    let execute = &experiment_config.execute;
    let setup = &experiment_config.setup;
    let teardown = &experiment_config.teardown;
    let variations = &experiment_config.variations;

    let mut experiments = Vec::with_capacity(variations.len());

    for variation in experiment_config.variations.iter() {
        let name = match &variation.name {
            Some(name) => name,
            None => name,
        };
        let expected_arguments = variation
            .expected_arguments
            .or(experiment_config.expected_arguments);

        let arguments = match variation.arguments.is_empty() {
            true => &experiment_config.arguments,
            false => &variation.arguments,
        };
        let hosts = match variation.hosts.is_empty() {
            true => &experiment_config.hosts,
            false => &variation.hosts,
        };
        // let runners = match variation.runners.is_empty() {
        //     true => &experiment_config.runners,
        //     false => &variation.runners,
        // };
        let exporters = match variation.exporters.is_empty() {
            true => &experiment_config.exporters,
            false => &variation.exporters,
        };

        experiments.push(Experiment {
            id: None,
            name: name.clone(),
            description: description.clone(),
            kind: kind.clone(),
            setup: map_remote_executions(
                config,
                setup,
                RemoteExecutionType::Setup,
            )?,
            teardown: map_remote_executions(
                config,
                teardown,
                RemoteExecutionType::Teardown,
            )?,
            // TODO(joren): These hosts may no longer be needed,
            // they are included in the struct where needed
            hosts: map_hosts(config, hosts),
            execute: remap_execute(config, execute, hosts)?,
            expected_arguments,
            arguments: arguments.clone(),
            exporters: map_exporters(config, exporters),
        });
    }
    Ok((experiment_config.runs, experiments))
}

fn check_file_exists(file: &Path) -> io::Result<()> {
    let exists = file.try_exists()?;

    match exists {
        true => Ok(()),
        false => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("File '{}' does not exist", file.to_string_lossy()),
        )),
    }
}

fn check_files_exist(files: &[PathBuf]) -> io::Result<()> {
    for file in files.iter() {
        check_file_exists(file)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::str::FromStr;

    const TEST_PATH: &str = "test";
    const VALID_CONFIG: &str = "valid_config.toml";
    const VALID_EXPERIMENT_CONFIG: &str = "valid_experiment_config.toml";

    fn test_path() -> PathBuf {
        PathBuf::from_str(TEST_PATH).expect("Failed to parse test_path")
    }

    #[test]
    fn parse_config() {
        let config = match Config::from_file(&test_path().join(VALID_CONFIG)) {
            Ok(config) => config,
            Err(e) => panic!("A valid config failed to be validated: {e}"),
        };

        if let Err(e) = config.validate() {
            panic!("Unable to validate config: {e}");
        }

        let expected_config = Config {
            hosts: vec![HostConfig {
                name: "runner1".to_string(),
                address: "localhost".to_string(),
            }],
            exporters: vec![
                ExporterConfig {
                    name: "test-exporter".to_string(),
                    hosts: vec!["runner1".to_owned()],
                    command: "sar -o collection.bin".to_string(),
                    setup: vec![
                        "dnf install -y sysstat".into(),
                        "apt install -y sysstat".into(),
                    ],
                },
                ExporterConfig {
                    name: "another-test-exporter".to_string(),
                    hosts: vec!["runner1".into()],
                    command: "my_collector".into(),
                    setup: vec![],
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
        let config_path = test_path().join(VALID_CONFIG);
        let experiment_config_path = test_path().join(VALID_EXPERIMENT_CONFIG);
        let config = match Config::from_file(&config_path) {
            Ok(config) => config,
            Err(e) => panic!("A valid config couldn't be parsed: {e}"),
        };

        if let Err(e) = config.validate() {
            panic!("Unable to validate config: {e}");
        }

        let experiment_config =
            match ExperimentConfig::from_file(&experiment_config_path) {
                Ok(experiment_config) => experiment_config,
                Err(e) => {
                    panic!("A valid experiment config couldn't be parsed: {e}")
                }
            };

        if let Err(e) = experiment_config.validate(&config) {
            panic!("Unable to validate experiment config: {e}");
        }

        let hosts = vec!["runner1".into()];
        let expected_experiment_config = ExperimentConfig {
                name: "localhost-experiment".into(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                execute: "./scripts/actual-work.sh".into(),
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![],
                hosts: hosts.clone(),
                runs: 1,
                setup: vec![RemoteExecutionConfig {
                        hosts: hosts.clone(),
                        scripts: vec!["./scripts/test-setup.sh".into()]
                    }
                ],
                variations: vec![VariationConfig {
                    name: None,
                    hosts: vec![],
                    expected_arguments: None,
                    arguments: vec![],
                    exporters: vec![]
                    },
                    VariationConfig {
                        name: Some("different args".into()),
                        hosts: vec![],
                        expected_arguments: Some(1),
                        arguments: vec!["Argument 1".into()],
                        exporters: vec![]
                    },
                    VariationConfig {
                        name: Some("with exporter".into()),
                        hosts: vec![],
                        expected_arguments: None,
                        arguments: vec![],
                        exporters: vec!["test-exporter".into()]
                    },
                    VariationConfig {
                        name: Some("with multiple exporters".into()),
                        hosts: vec![],
                        expected_arguments: None,
                        arguments: vec![],
                        exporters: vec!["test-exporter".into(), "another-test-exporter".into()]
                    }
                ],
                teardown: vec![RemoteExecutionConfig{
                        hosts: hosts.clone(),
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
        let config_path = test_path().join(VALID_CONFIG);
        let experiment_config_path = test_path().join(VALID_EXPERIMENT_CONFIG);
        let config = match Config::from_file(&config_path) {
            Ok(config) => config,
            Err(e) => panic!("A valid config couldn't be parsed: {e}"),
        };

        if let Err(e) = config.validate() {
            panic!("Unable to validate config: {e}");
        }

        let experiment_config =
            match ExperimentConfig::from_file(&experiment_config_path) {
                Ok(experiment_config) => experiment_config,
                Err(e) => {
                    panic!("A valid experiment config couldn't be parsed: {e}")
                }
            };

        if let Err(e) = experiment_config.validate(&config) {
            panic!("Unable to validate experiment config: {e}");
        }

        let experiments =
            super::generate_experiments(&config, &experiment_config).unwrap();

        let basepath = std::path::absolute(PathBuf::from("storage")).unwrap();

        let re_host = Host {
            name: "runner1".to_string(),
            address: "localhost".to_string(),
        };

        let scripts: HashMap<&str, PathBuf> = HashMap::from([
            ("setup", basepath.join("setup/test-setup.sh")),
            ("teardown", basepath.join("teardown/test-teardown.sh")),
            ("execute", basepath.join("execute/actual-work.sh")),
        ]);

        let mut remote_executions: HashMap<&str, Vec<RemoteExecution>> =
            HashMap::with_capacity(scripts.len());
        for (stage, script) in scripts.iter() {
            remote_executions.insert(
                stage,
                vec![RemoteExecution {
                    hosts: vec![re_host.clone()],
                    scripts: vec![script.clone()],
                }],
            );
        }

        let exporters = HashMap::from([
            (
                "test-exporter",
                Exporter {
                    name: "test-exporter".to_string(),
                    hosts: vec![re_host.clone()],
                    command: "sar -o collection.bin".to_string(),
                    setup: vec![
                        "dnf install -y sysstat".to_string(),
                        "apt install -y sysstat".to_string(),
                    ],
                },
            ),
            (
                "another-test-exporter",
                Exporter {
                    name: "another-test-exporter".into(),
                    hosts: vec![Host {
                        name: "runner1".into(),
                        address: "localhost".into(),
                    }],
                    command: "my_collector".into(),
                    setup: vec![],
                },
            ),
        ]);

        let expected_experiments = vec![
            Experiment {
                id: None,
                name: "localhost-experiment".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setup: remote_executions["setup"].clone(),
                teardown: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute: remote_executions["execute"].first().unwrap().clone(),
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![]
            },
            Experiment {
                id: None,
                name: "different args".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setup: remote_executions["setup"].clone(),
                teardown: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute: remote_executions["execute"].first().unwrap().clone(),
                arguments: vec!["Argument 1".into()],
                expected_arguments: Some(1),
                exporters: vec![]
            },
            Experiment {
                id: None,
                name: "with exporter".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setup: remote_executions["setup"].clone(),
                teardown: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute: remote_executions["execute"].first().unwrap().clone(),
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![exporters["test-exporter"].clone()]
            },
            Experiment {
                id: None,
                name: "with multiple exporters".to_string(),
                description: "Testing the functionality of the software completely using localhost".into(),
                kind: "localhost-result".into(),
                setup: remote_executions["setup"].clone(),
                teardown: remote_executions["teardown"].clone(),
                hosts: vec![re_host.clone()],
                execute:  remote_executions["execute"].first().unwrap().clone(),
                expected_arguments: Some(2),
                arguments: vec!["Argument 1".into(), "Argument 2".into()],
                exporters: vec![exporters["test-exporter"].clone(), exporters["another-test-exporter"].clone()]
            },
        ];

        let expected_runs = 1;
        assert_eq!(experiments.0, expected_runs,
            "Experiment runs did not match\nActual\n{:#?}\nExpected\n{expected_runs:#?}",
            experiments.0
        );

        assert_eq!(experiments.1, expected_experiments,
            "Experiments did not match\nActual\n{:#?}\nExpected\n{expected_experiments:#?}",
            experiments.1
        )
    }
}
