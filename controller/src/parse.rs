//! Functionality for parsing experiment configuration files and converting them into an internal
//! representation is provided in this module.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use log::error;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;

// The special string to use for localhost connections
const LOCALHOST: &str = "localhost";

// Represents an SSH connection string.
// Currently, user is required.
static RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\w+@)([a-zA-Z0-9.-]+)(:\d+)?$")
        .expect("Unable to compile regex")
});

/// A specialised [`Result`] type for Parsing operations.
///
/// This type is broadly used across [`crate::parse`] for any operation which may produce an
/// error.
///
/// This typedef is generally used to avoid writing out [`crate::parse::Error`] directly and is
/// otherwise a direct mapping to [`Result`].
///
/// # Examples
///
/// ```
/// use controller::parse::{self, Config};
///
/// fn check_my_config(config: Config) -> parse::Result<()> {
///     config.validate()
/// }
/// ```
pub type Result<T> = std::result::Result<T, Error>;

/// The error type for Parsing operations.
///
/// Errors are generally exposed by the [`Config::validate`], [`ExperimentConfig::validate`],
/// [`ExperimentConfig::validate_files`] and [`generate_experiments`] functions. Errors generally
/// result from an invalid configuration file, missing files on disk, or errors cross-referencing
/// the information between an experiment config and it's corresponding config file.
///
/// All errors include additional context that explains *when* the error occurred during the
/// parsing pipeline.
#[derive(Debug)]
pub enum Error {
    Error(String, Box<Error>),
    IOError(String, std::io::Error),
    DeserializeError(String, toml::de::Error),
    ValidationError(String),
}

impl Error {
    /// Best-effort status codes for use with HTTP Servers.
    pub fn status_code(&self) -> StatusCode {
        match *self {
            Error::Error(.., ref source) => source.status_code(),
            Error::IOError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::DeserializeError(..) => StatusCode::BAD_REQUEST,
            Error::ValidationError(..) => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        error!("{}", self);
        let body = self.to_string();
        let code = self.status_code();
        (code, body).into_response()
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Error::IOError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }

            Error::DeserializeError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }

            Error::Error(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            Error::ValidationError(ref message) => {
                write!(f, "Validation error: {message}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::Error(.., ref source) => Some(source),
            Self::IOError(.., ref source) => Some(source),
            Self::DeserializeError(.., ref source) => Some(source),
            Self::ValidationError(_) => None,
        }
    }
}

impl From<(String, std::io::Error)> for Error {
    /// Converts from an [`io::Error`] into a [`Error::IOError`] with a custom context
    /// message
    fn from(value: (String, std::io::Error)) -> Self {
        Error::IOError(value.0, value.1)
    }
}

impl From<(&str, std::io::Error)> for Error {
    /// Converts from an [`io::Error`] into a [`Error::IOError`] with a custom context
    /// message. This is an ergonomic inclusion, and converts the &str into a String.
    fn from(value: (&str, std::io::Error)) -> Self {
        Error::IOError(value.0.into(), value.1)
    }
}

impl From<std::io::Error> for Error {
    /// Converts from an [`io::Error`] into a [`Error::IOError`] with a predefined "IO Error"
    /// context message.
    fn from(value: std::io::Error) -> Self {
        Error::IOError("IO Error".to_string(), value)
    }
}

impl From<(&str, toml::de::Error)> for Error {
    /// Converts from a [`toml::de::Error`] into a [`Error::DeserializeError`] with a custom
    /// context message.
    fn from(value: (&str, toml::de::Error)) -> Self {
        Error::DeserializeError(value.0.to_owned(), value.1)
    }
}

impl From<(&str, Error)> for Error {
    /// Converts from a [`toml::de::Error`] into a [`Error::DeserializeError`] with a custom
    /// context message. This is an ergonomic inclusion, and converts the &str into a String.
    fn from(value: (&str, Error)) -> Self {
        Error::Error(value.0.to_string(), Box::new(value.1))
    }
}

impl From<(String, Error)> for Error {
    /// Converts from an [`Error`] into a [`Error::Error`] with a custom
    /// context message. This allows for nesting errors with additional levels of context.
    fn from(value: (String, Error)) -> Self {
        Error::Error(value.0, Box::new(value.1))
    }
}

/// A configuration file containing reusable components which can be referenced from any number of
/// [`ExperimentConfig`]s.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Config {
    /// A Vector of [`HostConfig`]s which represents distinct hosts for use in experiments.
    #[serde(default)]
    pub hosts: Vec<HostConfig>,
    /// A vector of [`ExporterConfig`]s which represent distinct exporters for use in experiments.
    #[serde(default)]
    pub exporters: Vec<ExporterConfig>,
}

impl Config {
    /// Generates a [`Config`] from a TOML file.
    pub fn from_file<P: AsRef<Path>>(file: P) -> Result<Self> {
        let config_str = std::fs::read_to_string(file.as_ref())
            .map_err(|e| Error::from(("error reading config file", e)))?;
        Config::parse_toml(&config_str).map_err(|e| {
            Error::from((
                format!("Error in config file {}", file.as_ref().display()),
                e,
            ))
        })
    }

    fn parse_toml(toml: &str) -> Result<Self> {
        let config = toml::from_str::<Self>(toml)
            .map_err(|e| Error::from(("Error parsing config toml", e)))?;
        Ok(config)
    }

    /// From a slice of [`HostConfig`], generates a slice containing the remote address
    /// corresponding to each host.
    pub fn host_endpoints(&self) -> Vec<String> {
        self.hosts.iter().map(|host| host.address.clone()).collect()
    }

    /// Ensures that all fields of a [`Config`] are valid.
    ///
    /// Since a [`Config`] consists only of other structs, it delegates to their
    /// *internal* validate functions.
    pub fn validate(&self) -> Result<()> {
        for host in self.hosts.iter() {
            host.validate()?;
        }

        for exporter in self.exporters.iter() {
            exporter.validate(&self.hosts)?;
        }
        Ok(())
    }

    pub fn validate_files<P: AsRef<Path>>(&self, storage_dir: P) -> Result<()> {
        for exporter in self.exporters.iter() {
            exporter.validate_files(&storage_dir)?;
        }
        Ok(())
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Config")?;
        for host in self.hosts.iter() {
            writeln!(f, "{host}")?;
        }

        for exporter in self.exporters.iter() {
            writeln!(f, "{exporter}")?;
        }
        Ok(())
    }
}

impl FromStr for Config {
    type Err = Error;
    /// Allows the generation of a [`Config`] from a TOML string
    fn from_str(s: &str) -> Result<Self> {
        Self::parse_toml(s)
    }
}

/// A [`HostConfig`] represents an SSH host which may be needed in an experiment.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct HostConfig {
    /// An identifier to be used in place of the address in other config fields.
    pub name: String,
    /// An SSH address. It should have the format: "user@remoteaddress" or "localhost", to specify
    /// that actions should be performed locally instead of on a remote machine.
    pub address: String,
}

impl HostConfig {
    pub fn new<S: Into<String>>(name: S, address: S) -> Self {
        Self {
            name: name.into(),
            address: address.into(),
        }
    }

    fn validation_error(&self, message: &str) -> Error {
        Error::ValidationError(format!(
            "Invalid host '{}': {}",
            &self.name, message
        ))
    }

    fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(self.validation_error("An empty name is not allowed"));
        }

        if self.name == LOCALHOST || self.address == LOCALHOST {
            return Err(self.validation_error(&format!(
                "Using a reserved keyword as it's address/name: {LOCALHOST}"
            )));
        }

        if !RE.is_match(&self.address) {
            return Err(self.validation_error(&format!(
                "Invalid SSH connection string: {}",
                &self.address
            )));
        }

        Ok(())
    }
}

impl Display for HostConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}: {}", self.name, self.address)
    }
}

//TODO(joren): Exporters should follow similar rules to normal execution.
//Either a command or a file should be usable, it's the 21st century.
//Don't waste time on that now, though

/// An [`ExporterConfig`] stores information relating to metric collectors (which we call exporters
/// in our framework).
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct ExporterConfig {
    /// An identifier for referring to this exporter within an [`ExperimentConfig`].
    pub name: String,
    /// A list of host identifiers referencing [`HostConfig`]s found inside of a [`Config`].
    pub hosts: Vec<String>,
    /// Either a shell command or a script to run for collecting metrics.
    #[serde(flatten)]
    pub command: CommandSource,
    /// Either a shell command or a script which prepares any resources the exporter needs.
    pub setup: Option<CommandSource>,
    // /// A command to be run for collecting metrics.
    // pub command: Option<String>,
    // /// A script to run for collecting metrics.
    // pub script: Option<PathBuf>,
    // /// A setup command which prepares any resources the exporter needs.
    // pub setup_command: Option<String>,
    // /// A setup script which prepares any resources the exporter needs.
    // pub setup_script: Option<PathBuf>,
}

impl ExporterConfig {
    pub fn new<S>(name: S, hosts: Vec<S>, command: CommandSource) -> Self
    where
        S: Into<String>,
    {
        let hosts: Vec<String> =
            hosts.into_iter().map(|host| host.into()).collect();

        Self {
            name: name.into(),
            hosts,
            command,
            setup: None,
        }
    }

    pub fn with_setup(&mut self, setup: CommandSource) -> &mut Self {
        self.setup = Some(setup);
        self
    }

    fn validation_error(&self, message: &str) -> Error {
        Error::ValidationError(format!(
            "Invalid exporter '{}': {}",
            self.name, message
        ))
    }

    fn io_error(&self, message: &str, e: io::Error) -> Error {
        Error::ValidationError(format!(
            "Invalid exporter '{}': {}: {e}",
            self.name, message
        ))
    }

    fn validate(&self, hosts: &[HostConfig]) -> Result<()> {
        if self.hosts.is_empty() {
            Err(self.validation_error("No hosts were defined"))?;
        }

        // Ensure we have valid hosts
        hosts_check(hosts, &self.hosts)
            .map_err(|e| self.validation_error(&e.to_string()))?;

        self.command
            .validate()
            .map_err(|e| self.validation_error(&e.to_string()))?;

        if let Some(setup) = &self.setup {
            setup
                .validate()
                .map_err(|e| self.validation_error(&e.to_string()))?;
        }

        Ok(())
    }

    fn validate_files<P: AsRef<Path>>(&self, storage_dir: P) -> Result<()> {
        self.command
            .validate_files(&FileType::Exporter, &storage_dir)
            .map_err(|e| self.io_error("Invalid setup command", e))?;

        if let Some(setup_command) = &self.setup {
            setup_command
                .validate_files(&FileType::Exporter, &storage_dir)
                .map_err(|e| self.io_error("Invalid setup command", e))?;
        }
        Ok(())
    }
}

impl Display for ExporterConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.name)?;
        for host in self.hosts.iter() {
            writeln!(f, "{host}")?;
        }
        writeln!(f, "command: {}", &self.command)?;

        if let Some(setup) = &self.setup {
            writeln!(f, "setup.command: {}", setup)?;
        }
        Ok(())
    }
}

/// A command type allows us to select between either a direct shell command,
/// or a .sh file for non *Config structs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CommandSource {
    Command(String),
    Script(PathBuf),
}

impl CommandSource {
    pub fn from_command<S: Into<String>>(command: S) -> Self {
        Self::Command(command.into())
    }

    pub fn from_script<P: Into<PathBuf>>(script: P) -> Self {
        Self::Script(script.into())
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Command(command) => {
                if command.is_empty() {
                    return Err(Error::ValidationError(
                        "An empty command is not permitted".to_string(),
                    ));
                }
            }
            Self::Script(script) => {
                if script.file_name().is_none() {
                    return Err(Error::ValidationError(
                        "A script path must end with a file name".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_files<P: AsRef<Path>>(
        &self,
        file_type: &FileType,
        storage_dir: P,
    ) -> io::Result<()> {
        if let Self::Script(script) = self {
            check_files_exist(&[script], file_type, &storage_dir)?;
        }
        Ok(())
    }
}

impl Display for CommandSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(command) => writeln!(f, "{command}"),
            Self::Script(script) => {
                writeln!(f, "{}", script.to_string_lossy())
            }
        }
    }
}

/// An [`ExperimentConfig`] defines an experiment to be run. Relies on a valid [`Config`] file
/// existing before one can be defined.
#[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct ExperimentConfig {
    // The ID is generated externally.
    // pub id: Option<String>,
    /// An identifier for an experiment. The name of an experiment is with the name
    /// given inside of a [`VariationConfig`] using a '-' character. Names must be non-empty.
    pub name: String,
    /// A description of the experiment which may provide important information for later
    /// reference. Descriptions may be empty.
    pub description: String,
    /// The [`ExperimentConfig::script`] file points to the main shell which runs the experiment.
    pub script: PathBuf,
    /// A list of identifiers representing [`HostConfig`]s defined in the main [`Config`].
    pub hosts: Vec<String>,
    /// Dependencies is a list of paths that the [`ExperimentConfig::script`] script relies on. These files/directories are
    /// sent to remote hosts inside of a directory named *execute*-deps when an experiment begins.
    /// For each variation/repeat of the experiment, these files are then symlinked into the
    /// current variation directory so that the script can detect them.
    #[serde(default)]
    pub dependencies: Vec<PathBuf>,
    /// Arguments contains a list of command line arguments that are sent to the [`ExperimentConfig::script`] script.
    #[serde(default)]
    pub arguments: Option<Vec<String>>,
    /// expected_arguments is an optional field for validating that each variation of the
    /// experiment is providing the correct number of arguments to the [`ExperimentConfig::script`] script.
    pub expected_arguments: Option<usize>,
    /// A 'kind' is another identifier that may be useful for determining the output type of an
    /// experiment. If the format of results changes for an experiment, it may be worth updating
    /// the 'kind' field to indicate this.
    pub kind: Option<String>,
    /// runs represents the number of times an experiment should be repeated. This value can be any
    /// number in the range [1,65536). Note that the order of runs is to perform every single
    /// variation of an experiment and then repeat. This breadth-first ordering is generally
    /// preferable when running experiments.
    pub runs: Option<u16>,
    /// A list of identifiers representing [`ExporterConfig`]s defined in the main [`Config`].
    #[serde(default)]
    pub exporters: Vec<String>,
    /// Setups contains a list of scripts to run on various remote hosts for preparing an
    /// experiment.
    #[serde(default)]
    pub setups: Vec<RemoteExecutionConfig>,
    /// Teardowns contains a list of scripts to run on various remote hosts for preparing an
    /// experiment.
    #[serde(default)]
    pub teardowns: Vec<RemoteExecutionConfig>,
    /// Variations allow an experiment to be run with minor variations, such as changing the input
    /// arguments/number of arguments, changing the exporters and/or changing the hosts which the
    /// experiment runs on.
    #[serde(default)]
    pub variations: Vec<VariationConfig>,
    /// If this boolean is enabled, a top-level Experiment is not created.
    /// This means that ONLY variations will be used to create experiments.
    /// Defaults to false
    #[serde(default)]
    pub variations_only: bool,
}

impl ExperimentConfig {
    const DEFAULT_RUNS: u16 = 1;
    const DEFAULT_KIND: &str = "default";

    pub fn new<S, P>(name: S, description: S, script: P, hosts: &[S]) -> Self
    where
        S: Into<String> + Clone,
        P: Into<PathBuf>,
    {
        let hosts = hosts.iter().map(|host| host.clone().into()).collect();

        Self {
            name: name.into(),
            description: description.into(),
            kind: None,
            script: script.into(),
            hosts,
            ..Default::default()
        }
    }

    pub fn with_kind<S: Into<String>>(&mut self, kind: S) -> &mut Self {
        self.kind = Some(kind.into());
        self
    }

    pub fn with_runs(&mut self, runs: u16) -> &mut Self {
        self.runs = Some(runs);
        self
    }

    pub fn with_dependencies<P: Into<PathBuf> + Clone>(
        &mut self,
        dependencies: &[P],
    ) -> &mut Self {
        let new_dependencies: Vec<PathBuf> = dependencies
            .iter()
            .map(|dependency| dependency.clone().into())
            .collect();

        self.dependencies = new_dependencies;
        self
    }

    pub fn with_arguments<S: Into<String> + Clone>(
        &mut self,
        arguments: &[S],
    ) -> &mut Self {
        let new_arguments: Vec<String> = arguments
            .iter()
            .map(|argument| argument.clone().into())
            .collect();

        self.arguments = Some(new_arguments);
        self
    }

    pub fn with_hosts<S: Into<String> + Clone>(
        &mut self,
        hosts: &[S],
    ) -> &mut Self {
        let new_hosts: Vec<String> =
            hosts.iter().map(|host| host.clone().into()).collect();

        self.hosts = new_hosts;
        self
    }

    pub fn with_expected_arguments(
        &mut self,
        expected_arguments: usize,
    ) -> &mut Self {
        self.expected_arguments = Some(expected_arguments);
        self
    }

    pub fn with_exporters<S: Into<String> + Clone>(
        &mut self,
        exporters: &[S],
    ) -> &mut Self {
        let new_exporters: Vec<String> = exporters
            .iter()
            .map(|exporter| exporter.clone().into())
            .collect();

        self.exporters = new_exporters;
        self
    }

    pub fn with_setup(&mut self, setup: &[RemoteExecutionConfig]) -> &mut Self {
        self.setups = setup.to_vec();
        self
    }

    pub fn with_teardown(
        &mut self,
        teardown: &[RemoteExecutionConfig],
    ) -> &mut Self {
        self.teardowns = teardown.to_vec();
        self
    }

    pub fn with_variations(
        &mut self,
        variations: &[VariationConfig],
    ) -> &mut Self {
        self.variations = variations.to_vec();
        self
    }

    pub fn with_only_variations(&mut self, only_variations: bool) -> &mut Self {
        self.variations_only = only_variations;
        self
    }

    /// Generates an [`ExperimentConfig`] from a TOML file.
    pub fn from_file<P: AsRef<Path>>(file: P) -> Result<Self> {
        let experiment_config_str = std::fs::read_to_string(file.as_ref())
            .map_err(|e| {
                Error::from(("error reading experiment config file", e))
            })?;

        ExperimentConfig::parse_toml(&experiment_config_str).map_err(|e| {
            Error::from((
                format!(
                    "Error in experiment config file {}",
                    file.as_ref().display()
                ),
                e,
            ))
        })
    }

    fn parse_toml(toml: &str) -> Result<Self> {
        let mut experiment_config = toml::from_str::<Self>(toml)
            .map_err(|e| Error::from(("Error parsing experiment config", e)))?;

        for setup in experiment_config.setups.iter_mut() {
            setup.file_type = Some(FileType::Setup);
        }

        for teardown in experiment_config.teardowns.iter_mut() {
            teardown.file_type = Some(FileType::Teardown);
        }

        Ok(experiment_config)
    }

    fn validation_error(&self, message: &str) -> Error {
        Error::ValidationError(format!(
            "Invalid experiment '{}': {message}",
            self.name
        ))
    }

    fn io_error(&self, message: &str, e: io::Error) -> Error {
        Error::ValidationError(format!(
            "Invalid experiment '{}': {message}: {e}",
            self.name
        ))
    }

    // NOTE: This function is always called BEFORE we generate experiments from the configuration,
    // therefore all paths must be remapped correctly within this function
    /// Ensures that all the files required by an [`ExperimentConfig`] are actually present on
    /// disk. This function shouldn't be used clientside, but it should be used the server which
    /// distributes files to remote hosts during runtime.
    pub fn validate_files<P: AsRef<Path>>(&self, storage_dir: P) -> Result<()> {
        check_files_exist(&[&self.script], &FileType::Execute, &storage_dir)
            .map_err(|e| self.io_error("Missing script", e))?;

        check_files_exist(
            &self.dependencies,
            &FileType::Dependency,
            &storage_dir,
        )
        .map_err(|e| self.io_error("Missing dependency", e))?;

        for setup in self.setups.iter() {
            setup.validate_files(&storage_dir)?;
        }

        for teardown in self.teardowns.iter() {
            teardown.validate_files(&storage_dir)?;
        }

        Ok(())
    }

    // Validates all members of an ExperimentConfig.
    // This cross-references with the Config passed in, to ensure that there
    // is consistency between both of the files.
    // Note that this function cannot check whether files exist on disk,
    // and the validate_files function should be used instead for this functionality.
    // Clients shouldn't worry about validating files in general, as they need to be uploaded to
    // the controller, which will check for files

    /// Ensures that all fields of an [`ExperimentConfig`] are valid.
    ///
    /// Any members of [`ExperimentConfig`] which are simple structs are validated here, any
    /// complex fields are delegated to their associated struct for validation.
    pub fn validate(&self, config: &Config) -> Result<()> {
        if self.name.is_empty() {
            Err(self
                .validation_error("An empty experiment name is not allowed"))?;
        }

        if self.description.is_empty() {
            Err(self.validation_error(
                "An empty experiment description is not allowed",
            ))?;
        }

        if let Some(kind) = &self.kind {
            if kind.is_empty() {
                Err(self.validation_error(
                    "If kind is defined, it cannot be empty",
                ))?;
            }
        }

        if let Some(runs) = &self.runs {
            if *runs == 0 {
                Err(self.validation_error(
                    "If runs is defined, it must be greater than 0",
                ))?;
            }
        }

        if self.script.to_string_lossy().is_empty() {
            Err(self.validation_error("An experiment script cannot be empty"))?;
        }

        for dependency in self.dependencies.iter() {
            if dependency.to_string_lossy().is_empty() {
                Err(self.validation_error(
                    "An empty experiment dependency is not allowed",
                ))?;
            }
        }

        if self.variations_only {
            arguments_check_lax(
                self.expected_arguments,
                self.arguments.as_ref(),
            )?;
        } else {
            arguments_check(self.expected_arguments, self.arguments.as_ref())?;
        }

        if self.hosts.is_empty() {
            Err(self.validation_error("No hosts have been defined"))?;
        }
        hosts_check(&config.hosts, &self.hosts)?;

        exporters_check(&config.exporters, &self.exporters)?;

        for setup in self.setups.iter() {
            setup.validate(config)?;
        }

        for teardown in self.teardowns.iter() {
            teardown.validate(config)?;
        }

        if self.variations_only && self.variations.is_empty() {
            Err(self.validation_error("If only_variations is true, there must be at least one variation"))?;
        }

        for variation in self.variations.iter() {
            variation.validate(config)?;

            // We need to cross reference if the variation hasn't defined arg
            match (variation.expected_arguments, &variation.arguments) {
                (None, Some(v_arguments)) => {
                    arguments_check(self.expected_arguments, Some(v_arguments))?
                }
                (Some(v_expected), None) => {
                    arguments_check(Some(v_expected), self.arguments.as_ref())?
                }
                _ => {}
            }
        }

        let mut names: Vec<&String> = self
            .variations
            .iter()
            .map(|variation| &variation.name)
            .collect();
        names.sort_unstable();
        names.dedup();
        if names.len() != self.variations.len() {
            Err(
                self.validation_error("Variations cannot have duplicate names")
            )?;
        }

        Ok(())
    }
}

impl Display for ExperimentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "ExperimentConfig")?;
        writeln!(f, "name: {}", &self.name)?;
        writeln!(f, "description: {}", &self.description)?;
        writeln!(
            f,
            "kind: {}",
            self.kind.as_ref().unwrap_or(&String::from("None"))
        )?;
        writeln!(f, "script: {}", &self.script.to_string_lossy())?;

        write!(f, "runs: ")?;
        match &self.runs {
            Some(runs) => writeln!(f, "{runs}")?,
            None => writeln!(f, "None")?,
        };

        writeln!(f, "Dependencies")?;
        for dependency in self.dependencies.iter() {
            writeln!(f, "{}", dependency.to_string_lossy())?;
        }

        writeln!(f, "Arguments")?;
        if let Some(arguments) = &self.arguments {
            for argument in arguments.iter() {
                writeln!(f, "{}", argument)?;
            }
        }

        write!(f, "expected arguments: ")?;
        match &self.expected_arguments {
            Some(expected_arguments) => writeln!(f, "{expected_arguments}")?,
            None => writeln!(f, "None")?,
        };

        writeln!(f, "Hosts")?;
        for host in self.hosts.iter() {
            writeln!(f, "{host}")?;
        }

        writeln!(f, "Exporters")?;
        for exporter in self.exporters.iter() {
            writeln!(f, "{exporter}")?;
        }

        writeln!(f, "Setup")?;
        for re in self.setups.iter() {
            writeln!(f, "{re}")?;
        }

        writeln!(f, "Teardown")?;
        for re in self.teardowns.iter() {
            writeln!(f, "{re}")?;
        }

        writeln!(f, "Variations")?;
        for variation in self.variations.iter() {
            writeln!(f, "{variation}")?;
        }

        Ok(())
    }
}

impl FromStr for ExperimentConfig {
    type Err = Error;

    /// Allows the generation of an [`ExperimentConfig`] from a TOML string
    fn from_str(s: &str) -> Result<Self> {
        Self::parse_toml(s)
    }
}

fn arguments_check(
    expected_arguments: Option<usize>,
    arguments: Option<&Vec<String>>,
) -> Result<()> {
    match (expected_arguments, arguments) {
        (Some(expected), None) => {
            if expected != 0 {
                Err(Error::ValidationError("expected_arguments should be set to 0 if arguments is unset".to_string()))?;
            }
        }
        (Some(expected), Some(arguments)) => {
            if expected != arguments.len() {
                Err(Error::ValidationError(format!(
                    "Expected {expected} arguments, got {}",
                    arguments.len()
                )))?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn arguments_check_lax(
    expected_arguments: Option<usize>,
    arguments: Option<&Vec<String>>,
) -> Result<()> {
    if let (Some(expected), Some(arguments)) = (expected_arguments, arguments) {
        if expected != arguments.len() {
            Err(Error::ValidationError(format!(
                "Expected {expected} arguments, got {}",
                arguments.len()
            )))?;
        }
    }

    Ok(())
}

fn hosts_check(c_hosts: &[HostConfig], hosts: &[String]) -> Result<()> {
    for host in hosts.iter() {
        if (c_hosts.iter().all(|c_host| c_host.name != *host)
            && host != LOCALHOST)
            || host.is_empty()
        {
            Err(Error::ValidationError(format!(
                "Invalid host definition, got '{host}', expected one of {:?}",
                c_hosts
                    .iter()
                    .map(|c_host| &c_host.name)
                    .chain(std::iter::once(&LOCALHOST.to_string()))
                    .collect::<Vec<&String>>()
            )))?;
        }
    }
    Ok(())
}

fn exporters_check(
    c_exporters: &[ExporterConfig],
    exporters: &[String],
) -> Result<()> {
    for exporter in exporters.iter() {
        if !c_exporters
            .iter()
            .any(|c_exporter| c_exporter.name == *exporter)
        {
            Err(Error::ValidationError(format!(
                "Invalid exporter definition, got '{exporter}', expected one of {:?}",
                c_exporters.iter().map(|c_exporter| &c_exporter.name).collect::<Vec<&String>>()
            )))?;
        }
    }
    Ok(())
}

// Each variation of an experiment is able to override the top level of the
// experiment configuration. Runners will override the runners used for the
// specific variation, and exporters will do the same for exporters. It is
// an error to not define at least one runner in either the top-level or
// in the variation.
//
//
/// A [`VariationConfig`] defines a variation of an [`ExperimentConfig`] to be run. This allows
/// changing the arguments fed to a test to assess different inputs, or changing the system metrics
/// to be collected.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct VariationConfig {
    /// An identifier for this variation. This identifier is joined with the upper
    /// [`ExperimentConfig::name`] using a '-' character. It must be a non-empty String,
    /// unique to this variation within the [`ExperimentConfig`].
    pub name: String,
    /// A description for this variation. This description overwrites any existing description
    /// from the upper [`ExperimentConfig::description`].
    pub description: Option<String>,
    /// A list of identifiers representing [`HostConfig`]s defined in the main [`Config`].
    #[serde(default)]
    pub hosts: Vec<String>,
    /// An optional integer specifying the expected number of script arguments required.
    pub expected_arguments: Option<usize>,
    /// A list of strings containing arguments to be passed to the [`ExperimentConfig::script`]
    /// script.
    #[serde(default)]
    pub arguments: Option<Vec<String>>,
    /// A list of identifiers representing [`ExporterConfig`]s defined in the main [`Config`].
    #[serde(default)]
    pub exporters: Vec<String>,
}

impl VariationConfig {
    pub fn new<S: Into<String>>(name: S) -> Self {
        Self {
            name: name.into(),
            description: None,
            hosts: vec![],
            expected_arguments: None,
            arguments: None,
            exporters: vec![],
        }
    }

    pub fn with_description<S: Into<String>>(
        &mut self,
        description: S,
    ) -> &mut Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_hosts<S: Into<String> + Clone>(
        &mut self,
        hosts: &[S],
    ) -> &mut Self {
        let hosts = hosts.iter().map(|host| host.clone().into()).collect();
        self.hosts = hosts;
        self
    }

    pub fn with_expected_arguments(
        &mut self,
        expected_arguments: usize,
    ) -> &mut Self {
        self.expected_arguments = Some(expected_arguments);
        self
    }

    pub fn with_arguments<S: Into<String> + Clone>(
        &mut self,
        arguments: &[S],
    ) -> &mut Self {
        let arguments = arguments
            .iter()
            .map(|argument| argument.clone().into())
            .collect();
        self.arguments = Some(arguments);
        self
    }

    pub fn with_exporters<S: Into<String> + Clone>(
        &mut self,
        exporters: &[S],
    ) -> &mut Self {
        let exporters = exporters
            .iter()
            .map(|exporter| exporter.clone().into())
            .collect();
        self.exporters = exporters;
        self
    }

    fn validation_error(&self, message: &str) -> Error {
        Error::ValidationError(format!(
            "Invalid experiment variation '{}': {message}",
            self.name
        ))
    }
    /// Ensures that all fields of a [`VariationConfig`] are valid.
    ///
    /// A [`VariationConfig`] must contain a non-empty name, and the exporters/hosts referenced in
    /// here must also exist inside of the associated [`Config`] file.
    pub fn validate(&self, config: &Config) -> Result<()> {
        if self.name.is_empty() {
            Err(self.validation_error(
                "An experiment variation cannot have an empty name",
            ))?;
        }

        if let Some(description) = &self.description {
            if description.is_empty() {
                Err(self.validation_error(
                    "If defined, and experiment description cannot be empty",
                ))?;
            }
        }

        if self.hosts.is_empty()
            && self.arguments.is_none()
            && self.exporters.is_empty()
        {
            Err(self.validation_error("At least one of 'hosts', 'arguments' or 'exporters' needs to be defined."))?;
        }

        hosts_check(&config.hosts, &self.hosts)?;
        arguments_check(self.expected_arguments, self.arguments.as_ref())?;
        exporters_check(&config.exporters, &self.exporters)?;

        Ok(())
    }
}

impl Display for VariationConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "name: {}", self.name)?;

        writeln!(
            f,
            "description: {}",
            self.description.as_ref().unwrap_or(&String::from("None"))
        )?;

        writeln!(f, "Hosts")?;
        for host in self.hosts.iter() {
            writeln!(f, "{host}")?;
        }

        write!(f, "expected_arguments: ")?;
        match &self.expected_arguments {
            Some(expected_arguments) => writeln!(f, "{expected_arguments}")?,
            None => writeln!(f, "None")?,
        };

        writeln!(f, "Arguments")?;
        if let Some(arguments) = &self.arguments {
            for argument in arguments.iter() {
                writeln!(f, "{argument}")?;
            }
        }

        writeln!(f, "Exporters")?;
        for exporter in self.exporters.iter() {
            writeln!(f, "{exporter}")?;
        }

        Ok(())
    }
}

/// A [`RemoteExecutionConfig`] represents 1 or more scripts to be run remotely on 1 or more hosts.
/// This struct is generally used for setup, teardown and execution of the main test script.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RemoteExecutionConfig {
    /// A list of host identifiers referencing [`HostConfig`]s found inside of a [`Config`].
    pub hosts: Vec<String>,
    /// The shell command/script to be run in parallel on all hosts defined in
    /// [`RemoteExecutionConfig::hosts`].
    #[serde(flatten)]
    pub command: CommandSource,
    // /// The path to a script to be run in parallel on all hosts defined in
    // /// [`RemoteExecutionConfig::hosts`].
    // pub script: Option<PathBuf>,
    // /// The command to run in parallel on all hosts defined in
    // /// [`RemoteExecutionConfig::hosts`]
    // pub command: Option<String>,
    /// A list of dependencies that the command/script requires to function correctly.
    #[serde(default)]
    pub dependencies: Vec<PathBuf>,
    /// The FileType of this remote execution
    #[serde(skip)]
    pub file_type: Option<FileType>,
}

impl RemoteExecutionConfig {
    pub fn new<S: Into<String> + Clone>(
        hosts: &[S],
        command: CommandSource,
    ) -> Self {
        let hosts: Vec<String> =
            hosts.iter().map(|host| host.clone().into()).collect();

        Self {
            hosts,
            command,
            dependencies: Default::default(),
            file_type: None,
        }
    }

    pub fn with_dependencies<P: Into<PathBuf> + Clone>(
        &mut self,
        dependencies: &[P],
    ) -> &mut Self {
        self.dependencies = dependencies
            .iter()
            .map(|dependency| dependency.clone().into())
            .collect();

        self
    }

    pub fn with_file_type(&mut self, file_type: FileType) -> &mut Self {
        self.file_type = Some(file_type);
        self
    }

    fn validation_error(&self, message: &str) -> Error {
        let ft = self
            .file_type
            .as_ref()
            .expect("FileType has not been assigned yet");
        Error::ValidationError(format!(
            "Invalid '{ft}' remote execution: {message}",
        ))
    }

    fn io_error(&self, message: &str, e: io::Error) -> Error {
        let ft = self
            .file_type
            .as_ref()
            .expect("FileType has not been assigned yet");
        Error::ValidationError(format!(
            "Invalid '{ft}' remote execution: {message}: {e}"
        ))
    }

    fn validate(&self, config: &Config) -> Result<()> {
        if self.hosts.is_empty() {
            Err(self.validation_error("No hosts were defined"))?;
        }

        hosts_check(&config.hosts, &self.hosts)?;

        self.command
            .validate()
            .map_err(|e| self.validation_error(&e.to_string()))?;

        for dependency in self.dependencies.iter() {
            if dependency.to_string_lossy().is_empty() {
                Err(self.validation_error(
                    "If dependencies are defined, they cannot be empty",
                ))?;
            }
        }

        Ok(())
    }

    fn validate_files<P: AsRef<Path>>(&self, storage_dir: P) -> Result<()> {
        self.command
            .validate_files(self.file_type.as_ref().unwrap(), &storage_dir)
            .map_err(|e| self.io_error("Invalid command", e))?;

        check_files_exist(
            &self.dependencies,
            &FileType::Dependency,
            &storage_dir,
        )?;

        Ok(())
    }
}

impl Display for RemoteExecutionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "RemoteExecutionConfig")?;
        for host in self.hosts.iter() {
            writeln!(f, "{host}")?;
        }

        writeln!(f, "command: {}", &self.command)?;

        writeln!(f, "Dependencies")?;
        for dependency in self.dependencies.iter() {
            writeln!(f, "{}", dependency.to_string_lossy())?;
        }

        write!(f, "file_type: ")?;
        match &self.file_type {
            Some(file_type) => writeln!(f, "{file_type}")?,
            None => writeln!(f, "None")?,
        };

        Ok(())
    }
}

fn validate_files<P, R>(
    files: &[P],
    file_type: &FileType,
    storage_dir: R,
) -> Result<()>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    // let scripts = remap_filepaths(files, file_type, storage_dir)?;

    check_files_exist(files, file_type, &storage_dir)
        .map_err(|e| Error::from((format!("Missing {} file", file_type), e)))
}

/// An [`Experiment`] contains an internal representation of an [`ExperimentConfig`] after it has
/// been validated against a [`Config`]. Once parsing is complete, an [`Experiment`] is expected to
/// be infallible in the sense that all configuration parameters should be correct. Runtime Errors
/// are still expected, however.
#[derive(Debug, PartialEq)]
pub struct Experiment {
    // TODO(joren): Remove this?
    pub id: Option<String>,
    /// an identifier to easily separate this experiment variation from others
    pub name: String,
    /// A description (which may be empty) to provide more context about the current experiment
    /// variation
    pub description: String,
    /// A kind is a metadata field which may prove useful when experiments change their output
    /// formats over time. This can contain any string which could help with differentiating
    /// output formats.
    pub kind: String,
    /// A list of [`RemoteExecution`]s for performing setup before the experiment begins.
    pub setups: Vec<RemoteExecution>,
    /// A list of [`RemoteExecution`]s for performing teardown after the experiment ends.
    pub teardowns: Vec<RemoteExecution>,
    /// A list of [`Host`]s which are used to establish SSH connections used throughout an
    /// experiment's runtime.
    pub hosts: Vec<Host>,
    /// A [`RemoteExecution`] for running the experiment variation. It's important to note that
    /// execute should only ever contain one script (currently).
    pub execute: RemoteExecution,
    /// The dependencies of the [`Experiment::execute`] script required for the experiment to run.
    pub dependencies: Vec<PathBuf>,
    /// A list of arguments to be passed to the [`Experiment::execute`] script.
    pub arguments: Vec<String>,
    /// An integer representing the number of arguments that should be passed to the
    /// [`Experiment::execute`] script. Probably not required anymore.
    pub expected_arguments: Option<usize>,
    /// A list of exporters to run in parallel to the [`Experiment::execute`] script. These
    /// exporters collect any metrics that are auxiliary to the experiment (CPU, power).
    pub exporters: Vec<Exporter>,
}

impl Experiment {
    /// Returns a vector containing all the unique host addresses for this experiment.
    pub fn hosts(&self) -> Vec<&String> {
        let mut hosts: Vec<&String> =
            self.hosts
                .iter()
                .map(|host| &host.address)
                .chain(self.exporters.iter().flat_map(|exporter| {
                    exporter.hosts.iter().map(|host| &host.address)
                }))
                .chain(self.setups.iter().flat_map(|stage| {
                    stage.hosts.iter().map(|host| &host.address)
                }))
                .chain(self.teardowns.iter().flat_map(|stage| {
                    stage.hosts.iter().map(|host| &host.address)
                }))
                .collect();

        hosts.sort();
        hosts.dedup();
        hosts
    }

    //TODO(joren): Currently we are getting all the files for all stages of the experiment and
    //uploading them to all hosts. Want we really want to do is upload the specific files
    //needed for each host to that host. This means that if Host 1 requires files A, B and C,
    //whereas host 2 requires files B, C and D, then Host 1 and 2 should be sent the
    //corresponding 3 files each.
    //Dumb solution is good for testing though.
    /// Returns a vector containing all the unique files for this experiment.
    ///
    /// This function exists so that dependencies can be uploaded to remote hosts before the
    /// experiment begins.
    pub fn files(&self) -> Vec<&Path> {
        let mut files: Vec<&Path> = Vec::new();
        if let CommandSource::Script(script) = &self.execute.command {
            files.push(script);
        }

        for re in self.setups.iter() {
            if let CommandSource::Script(script) = &re.command {
                files.push(script);
            }
        }

        for re in self.teardowns.iter() {
            if let CommandSource::Script(script) = &re.command {
                files.push(script);
            }
        }

        for dep in self.dependencies.iter() {
            files.push(dep.as_path());
        }

        files.sort();
        files.dedup();
        files
    }
}

impl TryFrom<(&Config, &ExperimentConfig, &Path)> for Experiment {
    type Error = Error;

    fn try_from(value: (&Config, &ExperimentConfig, &Path)) -> Result<Self> {
        let (config, experiment_config, storage_dir) = value;
        let kind = experiment_config
            .kind
            .clone()
            .unwrap_or(ExperimentConfig::DEFAULT_KIND.to_string());

        let setups = map_remote_executions(
            config,
            &experiment_config.setups,
            FileType::Setup,
            storage_dir,
        )?;

        let teardowns = map_remote_executions(
            config,
            &experiment_config.teardowns,
            FileType::Teardown,
            storage_dir,
        )?;

        let hosts = map_hosts(config, &experiment_config.hosts);
        let execute = remap_execute(
            config,
            &experiment_config.script,
            &experiment_config.hosts,
            storage_dir,
        )?;
        let dependencies =
            remap_dependencies(&experiment_config.dependencies, storage_dir)?;

        let arguments = experiment_config.arguments.clone().unwrap_or(vec![]);
        let exporters = map_exporters(config, &experiment_config.exporters);

        Ok(Self {
            id: None,
            name: experiment_config.name.clone(),
            description: experiment_config.description.clone(),
            kind,
            setups,
            teardowns,
            hosts,
            execute,
            dependencies,
            arguments,
            expected_arguments: experiment_config.expected_arguments,
            exporters,
        })
    }
}

impl TryFrom<(&Config, &ExperimentConfig, &VariationConfig, &Path)>
    for Experiment
{
    type Error = Error;

    fn try_from(
        value: (&Config, &ExperimentConfig, &VariationConfig, &Path),
    ) -> Result<Self> {
        let (config, experiment_config, variation, storage_dir) = value;
        let kind = experiment_config
            .kind
            .clone()
            .unwrap_or(ExperimentConfig::DEFAULT_KIND.to_string());

        let setups = map_remote_executions(
            config,
            &experiment_config.setups,
            FileType::Setup,
            storage_dir,
        )?;

        let teardowns = map_remote_executions(
            config,
            &experiment_config.teardowns,
            FileType::Teardown,
            storage_dir,
        )?;

        let execute = remap_execute(
            config,
            &experiment_config.script,
            &experiment_config.hosts,
            storage_dir,
        )?;

        let dependencies =
            remap_dependencies(&experiment_config.dependencies, storage_dir)?;

        let name = format!("{}-{}", &experiment_config.name, &variation.name);
        let description = variation
            .description
            .clone()
            .unwrap_or(experiment_config.description.clone());

        let hosts = {
            let hosts = match variation.hosts.is_empty() {
                true => experiment_config.hosts.clone(),
                false => variation.hosts.clone(),
            };
            map_hosts(config, &hosts)
        };

        let expected_arguments = variation
            .expected_arguments
            .or(experiment_config.expected_arguments);

        let arguments = match (
            variation.arguments.clone(),
            experiment_config.arguments.clone(),
        ) {
            (Some(arguments), None) => arguments,
            (None, Some(arguments)) => arguments.to_vec(),
            (Some(arguments), Some(_)) => arguments,
            (None, None) => vec![],
        };

        let exporters = {
            let exporters = match variation.exporters.is_empty() {
                true => &experiment_config.exporters,
                false => &variation.exporters,
            };
            map_exporters(config, exporters)
        };

        Ok(Self {
            id: None,
            name,
            description,
            kind,
            setups,
            teardowns,
            hosts,
            execute,
            dependencies,
            arguments,
            expected_arguments,
            exporters,
        })
    }
}

impl Display for Experiment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Experiment")?;
        write!(f, "id: ")?;
        match &self.id {
            Some(id) => writeln!(f, "{id}")?,
            None => writeln!(f, "None")?,
        };

        writeln!(f, "name: {}", self.name)?;
        writeln!(f, "description: {}", self.description)?;
        writeln!(f, "kind: {}", self.kind)?;

        writeln!(f, "Setups")?;
        for re in self.setups.iter() {
            writeln!(f, "{re}")?;
        }

        writeln!(f, "Teardowns")?;
        for re in self.teardowns.iter() {
            writeln!(f, "{re}")?;
        }

        writeln!(f, "Hosts")?;
        for host in self.hosts.iter() {
            writeln!(f, "{host}")?
        }

        writeln!(f, "{}", &self.execute)?;

        writeln!(f, "Dependencies")?;
        for dependency in self.dependencies.iter() {
            writeln!(f, "{}", dependency.display())?;
        }

        writeln!(f, "Arguments")?;
        for argument in self.arguments.iter() {
            writeln!(f, "{argument}")?;
        }

        write!(f, "expected_arguments: ")?;
        match self.expected_arguments {
            Some(expected) => writeln!(f, "{expected}")?,
            None => writeln!(f, "None")?,
        };

        writeln!(f, "Exporters")?;
        for exporter in self.exporters.iter() {
            writeln!(f, "{exporter}")?;
        }

        Ok(())
    }
}

/// A [`Host`] is similar to a [`HostConfig`] (currently identical), except for the fact that it is
/// not meant to be Serialized/Deserialized, and is meant to be an internal representation of a
/// Host, after parsing is complete.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Host {
    pub name: String,
    pub address: String,
}

impl Host {
    pub fn localhost() -> Self {
        Self {
            name: LOCALHOST.to_string(),
            address: LOCALHOST.to_string(),
        }
    }
}

impl Display for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "name: {}", &self.name)?;
        writeln!(f, "address: {}", &self.address)
    }
}

/// An [`Exporter`] is nearly identical to a [`ExporterConfig`], except for the [`Exporter::hosts`]
/// field, which contains full hosts instead of identifiers, making it a bit easier to use in
/// practice.
#[derive(Clone, Debug, PartialEq)]
pub struct Exporter {
    pub name: String,
    pub hosts: Vec<Host>,
    // pub address: String,
    pub command: CommandSource,
    pub setup: Option<CommandSource>,
}

impl Exporter {
    fn new(
        name: String,
        hosts: Vec<Host>,
        command: CommandSource,
        setup: Option<CommandSource>,
    ) -> Self {
        Self {
            name,
            hosts,
            command,
            setup,
        }
    }

    /// Returns the filenames for the future shell command.
    pub fn shell_files(&self) -> ExporterFiles {
        ExporterFiles {
            stdout: PathBuf::from(format!("{}.stdout", self.name)),
            stderr: PathBuf::from(format!("{}.stderr", self.name)),
            pid: PathBuf::from(format!("{}.pid", self.name)),
            // Not sure if we should use this or just the PID file. This has the advantage of not
            // overlapping with a PID file if there is some reason that we want advisory locking,
            // but not PID tracking.
            lock: PathBuf::from(format!("{}.lock", self.name)),
        }
    }
}

impl Display for Exporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "name: {}", &self.name)?;

        writeln!(f, "Hosts")?;
        for host in self.hosts.iter() {
            writeln!(f, "{host}")?;
        }
        writeln!(f, "command: {}", &self.command)?;

        write!(f, "setup: ")?;
        match &self.setup {
            Some(setup) => writeln!(f, "{setup}"),
            None => writeln!(f, "None"),
        }
    }
}

pub struct ExporterFiles {
    pub stdout: PathBuf,
    pub stderr: PathBuf,
    pub pid: PathBuf,
    pub lock: PathBuf,
}

impl ExporterFiles {
    pub fn set_base<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();
        self.stdout = path.join(&self.stdout);
        self.stderr = path.join(&self.stderr);
        self.pid = path.join(&self.pid);
        self.lock = path.join(&self.lock);
    }
}

/// A [`RemoteExecution`] is similar to a [`RemoteExecutionConfig`], except it contains an
/// additional field to state what kind of file/files are being used. Refer to [`FileType`] for
/// more information.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteExecution {
    /// A list of [`Host`]s for remote connections.
    pub hosts: Vec<Host>,
    /// A list of scripts to be run on the [`RemoteExecution::hosts`].
    pub command: CommandSource,
    // pub scripts: Vec<PathBuf>,
    /// A [`FileType`] stating the nature of the scripts in this struct.
    pub re_type: FileType,
}

impl RemoteExecution {
    fn new(
        hosts: Vec<Host>,
        command: CommandSource,
        re_type: FileType,
    ) -> Self {
        Self {
            hosts,
            command,
            re_type,
        }
    }

    /// Since scripts are stored as absolute paths, this function just returns a vector, taking
    /// only the filename of each script.
    pub fn remote_scripts(&self) -> Option<PathBuf> {
        //TODO(joren): Decide whether it would be worth organising scripts into
        //setup/teardown/execute directories when they are on remotes.
        if let CommandSource::Script(script) = &self.command {
            let filename = script.file_name().unwrap().to_string_lossy();
            Some(PathBuf::from(filename.to_string()))
        } else {
            None
        }
        // self.script.
        // self.scripts
        //     .iter()
        //     .map(|script| {
        //         let filename = script.file_name().unwrap().to_string_lossy();
        //         PathBuf::from(filename.to_string())
        //         // PathBuf::from(format!(
        //         //     "{}/{filename}",
        //         //     self.re_type.to_string()
        //         // ))
        //     })
        //     .collect()
    }
}

impl Display for RemoteExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Hosts")?;
        for host in self.hosts.iter() {
            writeln!(f, "{host}")?;
        }

        writeln!(f, "{}", &self.command)?;
        writeln!(f, "{}", &self.re_type)
    }
}

/// Describes the significance of a file for remote use.
#[derive(Clone, Debug, PartialEq, strum::EnumIter)]
pub enum FileType {
    /// Setup files are used during the setup stage of an experiment.
    Setup,
    /// Teardown files are used during the teardown stage of an experiment.
    Teardown,
    /// Execute files are the main script to be run for an experiment.
    Execute,
    /// Dependency files are (currently) files that are required for the main script of an
    /// experiment to function correctly. In the future dependencies may exist more broadly.
    Dependency,
    Exporter,
}

impl Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Setup => write!(f, "setup"),
            Self::Teardown => write!(f, "teardown"),
            Self::Execute => write!(f, "execute"),
            Self::Exporter => write!(f, "exporter"),
            Self::Dependency => write!(f, "dependency"),
        }
    }
}

impl FromStr for FileType {
    type Err = Error;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let re_type = match s {
            "setup" => Self::Setup,
            "teardown" => Self::Teardown,
            "execute" => Self::Execute,
            "exporter" => Self::Exporter,
            "dependency" => Self::Dependency,
            &_ => Err(Error::ValidationError(format!(
                "Invalid remote execution type, got {s}"
            )))?,
        };

        Ok(re_type)
    }
}

impl TryFrom<&Path> for FileType {
    type Error = Error;
    fn try_from(value: &Path) -> std::result::Result<Self, Self::Error> {
        let filename = value
            .file_name()
            .ok_or(Error::ValidationError(
                "Tried to parse a file type from a non-file path".to_string(),
            ))?
            .to_string_lossy();

        if filename.contains("setup") {
            Ok(Self::Setup)
        } else if filename.contains("teardown") {
            Ok(Self::Teardown)
        } else if filename.contains("dependency") {
            Ok(Self::Dependency)
        } else if filename.contains("exporter") {
            Ok(Self::Exporter)
        } else {
            Ok(Self::Execute)
        }
    }
}

fn map_hosts(config: &Config, hosts: &[String]) -> Vec<Host> {
    let mut mapped_hosts = config
        .hosts
        .iter()
        .filter(|c_host| hosts.contains(&c_host.name))
        .map(|c_host| Host {
            name: c_host.name.clone(),
            address: c_host.address.clone(),
        })
        .collect::<Vec<Host>>();

    for host in hosts.iter() {
        if host == LOCALHOST {
            mapped_hosts.push(Host::localhost());
        }
    }

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

            let command = c_exporter.command.clone();
            let setup = c_exporter.setup.clone();

            // let command = {
            //     if let Some(command) = &c_exporter.command {
            //         CommandSource::Command(command.clone())
            //     } else if let Some(script) = &c_exporter.script {
            //         CommandSource::Script(script.clone())
            //     } else {
            //         panic!(
            //             "A command somehow wasn't defined for exporter {}",
            //             c_exporter.name
            //         );
            //     }
            // };
            //
            // let setup = {
            //     if let Some(command) = &c_exporter.setup_command {
            //         Some(CommandSource::Command(command.clone()))
            //     } else if let Some(script) = &c_exporter.setup_script {
            //         Some(CommandSource::Script(script.clone()))
            //     } else {
            //         None
            //     }
            // };

            Exporter::new(c_exporter.name.to_string(), hosts, command, setup)
        })
        .collect()
}

fn remap_execute<P, R>(
    config: &Config,
    execute: P,
    hosts: &[String],
    storage_dir: R,
) -> Result<RemoteExecution>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    let hosts = map_hosts(config, hosts);

    // This is always one script for the execute key
    let script = remap_filepath(execute, &FileType::Execute, storage_dir)?;

    Ok(RemoteExecution::new(
        hosts,
        CommandSource::Script(script),
        FileType::Execute,
    ))
}

fn remap_dependencies<P, R>(
    dependencies: &[P],
    storage_dir: R,
) -> io::Result<Vec<PathBuf>>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    dependencies
        .iter()
        .map(|dep| remap_dependency(dep, &storage_dir))
        .collect()
}

fn remap_dependency<P, R>(dependency: P, storage_dir: R) -> io::Result<PathBuf>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    remap_filepath(dependency, &FileType::Dependency, storage_dir)
}

// TODO(joren): RemoteExecution needs to be updated for this to work.
fn map_remote_executions<P: AsRef<Path>>(
    config: &Config,
    remote_executions: &[RemoteExecutionConfig],
    remote_execution_type: FileType,
    storage_dir: P,
) -> Result<Vec<RemoteExecution>> {
    let mut mapped_remote_executions =
        Vec::with_capacity(remote_executions.len());

    for remote_execution in remote_executions.iter() {
        let hosts = map_hosts(config, &remote_execution.hosts);

        let command = match remote_execution.command {
            CommandSource::Script(ref script) => {
                CommandSource::Script(remap_filepath(
                    script.clone(),
                    &remote_execution_type,
                    &storage_dir,
                )?)
            }
            CommandSource::Command(ref command) => {
                CommandSource::Command(command.clone())
            }
        };

        mapped_remote_executions.push(RemoteExecution::new(
            hosts,
            command,
            remote_execution_type.clone(),
        ));
    }
    Ok(mapped_remote_executions)
}

// TODO(joren): We can't actually remap filepaths until we know what directory the user wants to use for
// storage. We currently have it set up so that we assume the default path is being used when we
// validate files. We should really add a configurable storage root for checking/remapping file
// paths.
fn remap_filepaths<P, R>(
    files: &[P],
    file_type: &FileType,
    storage_dir: R,
) -> io::Result<Vec<PathBuf>>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    files
        .iter()
        .map(|script| remap_filepath(script, file_type, &storage_dir))
        .collect::<io::Result<Vec<PathBuf>>>()
}

fn remap_filepath<P, R>(
    file: P,
    file_type: &FileType,
    storage_dir: R,
) -> io::Result<PathBuf>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    let basename = file.as_ref().file_name().unwrap_or_else(|| {
        panic!("The script {:?} should have had a filename", file.as_ref())
    });
    let new_path = storage_dir
        .as_ref()
        .to_path_buf()
        .join(file_type.to_string())
        .join(basename);

    std::path::absolute(new_path)
}

/// A type representing a list of experiment variations and the number of times that they need to
/// be run.
pub type ExperimentRuns = (u16, Vec<Experiment>);

/// Generates a list of [`Experiment`]s based on a [`Config`] and an [`ExperimentConfig`].
/// All fields are owned, so they are cloned from the config/experiment config as required.
///
/// Currently an experiment without any variations cannot be turned into an experiment at all. This
/// functionality is likely to change in the future, so that a valid top-level experiment in an
/// experiment config can be turned into an experiment as well.
///
/// NOTE: This function should only be run AFTER config/experiment config validation has been done.
pub fn generate_experiments<P: AsRef<Path>>(
    config: &Config,
    experiment_config: &ExperimentConfig,
    storage_dir: P,
) -> Result<ExperimentRuns> {
    let variations = &experiment_config.variations;
    let runs = experiment_config
        .runs
        .unwrap_or(ExperimentConfig::DEFAULT_RUNS);

    let mut experiments = Vec::with_capacity(variations.len() + 1);

    if !experiment_config.variations_only {
        experiments.push(Experiment::try_from((
            config,
            experiment_config,
            storage_dir.as_ref(),
        ))?);
    }

    for variation in experiment_config.variations.iter() {
        experiments.push(Experiment::try_from((
            config,
            experiment_config,
            variation,
            storage_dir.as_ref(),
        ))?);
    }
    Ok((runs, experiments))
}

pub fn check_files_exist<P: AsRef<Path>, R: AsRef<Path>>(
    files: &[P],
    file_type: &FileType,
    storage_dir: &R,
) -> io::Result<()> {
    let files = remap_filepaths(files, file_type, storage_dir)?;
    for file in files.iter() {
        let exists = file.try_exists()?;

        match exists {
            true => {}
            false => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("File '{}' does not exist", file.display()),
            ))?,
        };
    }

    Ok(())
}
