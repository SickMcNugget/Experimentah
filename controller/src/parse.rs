//! Functionality for parsing experiment configuration files and converting them into an internal
//! representation is provided in this module.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use log::error;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{fs, io};

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
    pub hosts: Vec<HostConfig>,
    /// A vector of [`ExporterConfig`]s which represent distinct exporters for use in experiments.
    pub exporters: Vec<ExporterConfig>,
}

impl FromStr for Config {
    type Err = Error;
    /// Allows the generation of a [`Config`] from a TOML string
    fn from_str(s: &str) -> Result<Self> {
        Self::parse_toml(s)
    }
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
}

/// A [`HostConfig`] represents an SSH host which may be needed in an experiment.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct HostConfig {
    /// An identifier to be used in place of the address in other config fields.
    pub name: String,
    /// An SSH address. It should have the format: "user@remoteaddress" or "localhost", to specify
    /// that actions should be performed locally instead of on a remote machine.
    pub address: String,
}

impl HostConfig {
    fn validate(&self) -> Result<()> {
        // TODO(joren): The only validation we can do is
        // check whether we have a valid address
        Ok(())
    }
}

//TODO(joren): Exporters should follow similar rules to normal execution.
//Either a command or a file should be usable, it's the 21st century.
//Don't waste time on that now, though

/// An [`ExporterConfig`] stores information relating to metric collectors (which we call exporters
/// in our framework).
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct ExporterConfig {
    /// An identifier for referring to this exporter within an [`ExperimentConfig`].
    pub name: String,
    /// A list of host identifiers referencing [`HostConfig`]s found inside of a [`Config`].
    pub hosts: Vec<String>,
    /// A command to be run for collecting metrics.
    pub command: String,
    /// A list of setup commands that are run before trying to run the exporter.
    #[serde(default)]
    pub setup: Vec<String>,
}

impl ExporterConfig {
    fn validate(&self, hosts: &[HostConfig]) -> Result<()> {
        // Ensure we have a valid command
        shlex::split(&self.command).ok_or(Error::ValidationError(format!(
            "Invalid command for exporter {}: {}",
            self.name, self.command
        )))?;

        // Ensure we have valid setup commands
        for command in self.setup.iter() {
            shlex::split(command).ok_or(Error::ValidationError(format!(
                "Invalid setup command for exporter {}: {}",
                self.name, command
            )))?;
        }

        // Ensure the hosts exist
        for host in self.hosts.iter() {
            hosts.iter().find(|c_host| c_host.name == *host).ok_or(
                Error::ValidationError(format!(
                    "Invalid host definition for exporter {}: {}",
                    self.name, host
                )),
            )?;
        }

        Ok(())
    }
}

/// An [`ExperimentConfig`] defines an experiment to be run. Relies on a valid [`Config`] file
/// existing before one can be defined.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ExperimentConfig {
    // The ID is generated externally.
    // pub id: Option<String>,
    /// An identifier for an experiment. The name of an experiment is with the name
    /// given inside of a [`VariationConfig`] using a '-' character. Names must be non-empty.
    pub name: String,
    /// A description of the experiment which may provide important information for later
    /// reference. Descriptions may be empty.
    pub description: String,
    /// A 'kind' is another identifier that may be useful for determining the output type of an
    /// experiment. If the format of results changes for an experiment, it may be worth updating
    /// the 'kind' field to indicate this.
    pub kind: String,
    /// The [`ExperimentConfig::execute`] file points to the main shell which runs the experiment.
    pub execute: PathBuf,
    #[serde(default)]
    /// Dependencies is a list of filepaths that the [`ExperimentConfig::execute`] script relies on. These files are
    /// sent to remote hosts inside of a directory named *execute*-deps when an experiment begins.
    /// For each variation/repeat of the experiment, these files are then symlinked into the
    /// current variation directory so that the script can detect them.
    pub dependencies: Vec<PathBuf>,
    /// Arguments contains a list of command line arguments that are sent to the [`ExperimentConfig::execute`] script.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// expected_arguments is an optional field for validating that each variation of the
    /// experiment is providing the correct number of arguments to the [`ExperimentConfig::execute`] script.
    pub expected_arguments: Option<usize>,
    /// runs represents the number of times an experiment should be repeated. This value can be any
    /// number in the range [1,65536). Note that the order of runs is to perform every single
    /// variation of an experiment and then repeat. This breadth-first ordering is generally
    /// preferable when running experiments.
    #[serde(default = "ExperimentConfig::default_runs")]
    pub runs: u16,
    /// Setup contains a list of scripts to run on various remote hosts for preparing an
    /// experiment.
    #[serde(default)]
    pub setup: Vec<RemoteExecutionConfig>,
    /// Teardown contains a list of scripts to run on various remote hosts for preparing an
    /// experiment.
    #[serde(default)]
    pub teardown: Vec<RemoteExecutionConfig>,
    /// Variations allow an experiment to be run with minor variations, such as changing the input
    /// arguments/number of arguments, changing the exporters and/or changing the hosts which the
    /// experiment runs on.
    #[serde(default)]
    pub variations: Vec<VariationConfig>,
    /// A list of identifiers representing [`ExporterConfig`]s defined in the main [`Config`].
    #[serde(default)]
    pub exporters: Vec<String>,
    /// A list of identifiers representing [`HostConfig`]s defined in the main [`Config`].
    #[serde(default)]
    pub hosts: Vec<String>,
}

impl FromStr for ExperimentConfig {
    type Err = Error;

    /// Allows the generation of an [`ExperimentConfig`] from a TOML string
    fn from_str(s: &str) -> Result<Self> {
        Self::parse_toml(s)
    }
}

impl ExperimentConfig {
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
        let experiment_config = toml::from_str::<Self>(toml)
            .map_err(|e| Error::from(("Error parsing experiment config", e)))?;

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
            Err(Error::ValidationError(format!(
                "Error in experiment '{}': Expected {} arguments, got {}",
                name,
                expected.unwrap(),
                args.len()
            )))?;
        }
        Ok(())
    }

    // NOTE: This function is always called BEFORE we generate experiments from the configuration,
    // therefore all paths must be remapped correctly within this function
    /// Ensures that all the files required by an [`ExperimentConfig`] are actually present on
    /// disk. This function shouldn't be used clientside, but it should be used the server which
    /// distributes files to remote hosts during runtime.
    pub fn validate_files<P: AsRef<Path>>(&self, storage_dir: P) -> Result<()> {
        check_file_exists(&remap_filepath(
            &self.execute,
            &FileType::Execute,
            &storage_dir,
        )?)
        .map_err(|e| {
            Error::from((
                format!("Missing files for experiment '{}'", self.name),
                e,
            ))
        })?;

        validate_files(
            &self.dependencies,
            &FileType::Dependency,
            &storage_dir,
        )?;

        for setup in self.setup.iter() {
            validate_files(&setup.scripts, &FileType::Setup, &storage_dir)
                .map_err(|e| {
                    Error::from((
                        format!(
                            "Missing files in setup for experiment '{}'",
                            &self.name
                        ),
                        e,
                    ))
                })?;
        }

        for teardown in self.teardown.iter() {
            validate_files(
                &teardown.scripts,
                &FileType::Teardown,
                &storage_dir,
            )
            .map_err(|e| {
                Error::from((
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
        if self.execute.clone().into_os_string().is_empty() {
            Err(Error::ValidationError(
                "The 'execute' field in an experiment config cannot be empty"
                    .to_string(),
            ))?;
        }

        for setup in self.setup.iter() {
            setup.validate(config, &self.name).map_err(|e| {
                Error::from((
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

        // Ensure that all variations are valid,
        // and also ensure that no 2 variations have the same name
        for (i, variation) in self.variations.iter().enumerate() {
            let name = variation.name.as_ref();
            variation.validate(config, &self.name).map_err(|e| {
                Error::from((
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
                return Err(Error::ValidationError(format!(
                    "No hosts have been defined for experiment '{}'",
                    self.name
                )));
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
            return Err(Error::ValidationError(format!(
                "There were variations with duplicate names for experiment '{}'", self.name
            )));
        }

        for teardown in self.teardown.iter() {
            teardown.validate(config, &self.name).map_err(|e| {
                Error::from((
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

fn hosts_check(
    c_hosts: &[HostConfig],
    hosts: &[String],
    experiment_name: &str,
) -> Result<()> {
    for host in hosts.iter() {
        if !c_hosts.iter().any(|c_host| c_host.name == *host) {
            Err(Error::ValidationError(format!(
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
            Err(Error::ValidationError(format!(
                "Invalid exporter for experiment '{experiment_name}': got '{exporter}', expected one of {:?}",
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
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct VariationConfig {
    /// An identifier for this variation. This identifier is joined with the upper
    /// [`ExperimentConfig::name`] using a '-' character. It must be a non-empty String,
    /// unique to this variation within the [`ExperimentConfig`].
    pub name: String,
    /// A list of identifiers representing [`HostConfig`]s defined in the main [`Config`].
    #[serde(default)]
    pub hosts: Vec<String>,
    /// An optional integer specifying the expected number of script arguments required.
    pub expected_arguments: Option<usize>,
    /// A list of strings containing arguments to be passed to the [`ExperimentConfig::execute`]
    /// script.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// A list of identifiers representing [`ExporterConfig`]s defined in the main [`Config`].
    #[serde(default)]
    pub exporters: Vec<String>,
}

impl VariationConfig {
    /// Ensures that all fields of a [`VariationConfig`] are valid.
    ///
    /// A [`VariationConfig`] must contain a non-empty name, and the exporters/hosts referenced in
    /// here must also exist inside of the associated [`Config`] file.
    pub fn validate(
        &self,
        config: &Config,
        experiment_name: &str,
    ) -> Result<()> {
        if self.name.is_empty() {
            Err(Error::ValidationError(format!(
                "Invalid variation for experiment '{experiment_name}': empty name",
                )))?;
        }

        hosts_check(&config.hosts, &self.hosts, &self.name)?;
        exporters_check(&config.exporters, &self.exporters, &self.name)?;

        Ok(())
    }
}

/// A [`RemoteExecutionConfig`] represents 1 or more scripts to be run remotely on 1 or more hosts.
/// This struct is generally used for setup, teardown and execution of the main test script.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct RemoteExecutionConfig {
    /// A list of host identifiers referencing [`HostConfig`]s found inside of a [`Config`].
    pub hosts: Vec<String>,
    /// A list of file paths representing the scripts to be run, in order, on the hosts defined in
    /// [`RemoteExecutionConfig::hosts`].
    pub scripts: Vec<PathBuf>,
}

impl RemoteExecutionConfig {
    #[deprecated = "Scripts are now executed directly by a shell."]
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
    let scripts = remap_filepaths(files, file_type, storage_dir)?;

    check_files_exist(&scripts)
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
    pub setup: Vec<RemoteExecution>,
    /// A list of [`RemoteExecution`]s for performing teardown after the experiment ends.
    pub teardown: Vec<RemoteExecution>,
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
                .chain(self.setup.iter().flat_map(|stage| {
                    stage.hosts.iter().map(|host| &host.address)
                }))
                .chain(self.teardown.iter().flat_map(|stage| {
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
        let mut files: Vec<&Path> = self
            .execute
            .scripts
            .iter()
            .map(|script| script.as_path())
            .chain(self.setup.iter().flat_map(|setup| {
                setup.scripts.iter().map(|script| script.as_path())
            }))
            .chain(self.teardown.iter().flat_map(|teardown| {
                teardown.scripts.iter().map(|script| script.as_path())
            }))
            .chain(self.dependencies.iter().map(|script| script.as_path()))
            .collect();

        files.sort();
        files.dedup();
        files
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

/// An [`Exporter`] is nearly identical to a [`ExporterConfig`], except for the [`Exporter::hosts`]
/// field, which contains full hosts instead of identifiers, making it a bit easier to use in
/// practice.
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
    pub scripts: Vec<PathBuf>,
    /// A [`FileType`] stating the nature of the scripts in this struct.
    pub re_type: FileType,
}

impl RemoteExecution {
    fn new(hosts: Vec<Host>, scripts: Vec<PathBuf>, re_type: FileType) -> Self {
        Self {
            hosts,
            scripts,
            re_type,
        }
    }

    /// Since scripts are stored as absolute paths, this function just returns a vector, taking
    /// only the filename of each script.
    pub fn remote_scripts(&self) -> Vec<PathBuf> {
        //TODO(joren): Decide whether it would be worth organising scripts into
        //setup/teardown/execute directories when they are on remotes.
        self.scripts
            .iter()
            .map(|script| {
                let filename = script.file_name().unwrap().to_string_lossy();
                PathBuf::from(filename.to_string())
                // PathBuf::from(format!(
                //     "{}/{filename}",
                //     self.re_type.to_string()
                // ))
            })
            .collect()
    }
}

/// Describes the significance of a file for remote use.
#[derive(Clone, Debug, PartialEq)]
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
}

impl Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Setup => write!(f, "setup"),
            Self::Teardown => write!(f, "teardown"),
            Self::Execute => write!(f, "execute"),
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
        } else {
            Ok(Self::Execute)
        }
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
    let scripts =
        vec![remap_filepath(execute, &FileType::Execute, storage_dir)?];

    Ok(RemoteExecution::new(hosts, scripts, FileType::Execute))
}

fn remap_dependencies<P, R>(
    dependencies: &[P],
    storage_dir: R,
) -> Result<Vec<PathBuf>>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    dependencies
        .iter()
        .map(|dep| remap_dependency(dep, &storage_dir))
        .collect()
}

fn remap_dependency<P, R>(dependency: P, storage_dir: R) -> Result<PathBuf>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    remap_filepath(dependency, &FileType::Dependency, storage_dir)
}

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

        let mapped_scripts = remap_filepaths(
            &remote_execution.scripts,
            &remote_execution_type,
            &storage_dir,
        )?;

        mapped_remote_executions.push(RemoteExecution::new(
            hosts,
            mapped_scripts,
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
) -> Result<Vec<PathBuf>>
where
    P: AsRef<Path>,
    R: AsRef<Path>,
{
    files
        .iter()
        .map(|script| remap_filepath(script, file_type, &storage_dir))
        .collect::<Result<Vec<PathBuf>>>()
}

fn remap_filepath<P, R>(
    file: P,
    file_type: &FileType,
    storage_dir: R,
) -> Result<PathBuf>
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
    Ok(std::path::absolute(new_path)?)
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
    let name = &experiment_config.name;
    let description = &experiment_config.description;
    let kind = &experiment_config.kind;
    let execute = &experiment_config.execute;
    let dependencies = &experiment_config.dependencies;
    let setup = &experiment_config.setup;
    let teardown = &experiment_config.teardown;
    let variations = &experiment_config.variations;

    let mut experiments = Vec::with_capacity(variations.len());

    for variation in experiment_config.variations.iter() {
        let name = format!("{name}-{}", &variation.name);

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
            name,
            description: description.clone(),
            kind: kind.clone(),
            setup: map_remote_executions(
                config,
                setup,
                FileType::Setup,
                &storage_dir,
            )?,
            teardown: map_remote_executions(
                config,
                teardown,
                FileType::Teardown,
                &storage_dir,
            )?,
            // TODO(joren): These hosts may no longer be needed,
            // they are included in the struct where needed
            hosts: map_hosts(config, hosts),
            execute: remap_execute(config, execute, hosts, &storage_dir)?,
            dependencies: remap_dependencies(dependencies, &storage_dir)?,
            expected_arguments,
            arguments: arguments.clone(),
            exporters: map_exporters(config, exporters),
        });
    }
    Ok((experiment_config.runs, experiments))
}

pub fn check_file_exists<P: AsRef<Path>>(file: P) -> io::Result<()> {
    let exists = file.as_ref().try_exists()?;

    match exists {
        true => Ok(()),
        false => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("File '{}' does not exist", file.as_ref().display()),
        )),
    }
}

pub fn check_files_exist<P: AsRef<Path>>(files: &[P]) -> io::Result<()> {
    for file in files.iter() {
        check_file_exists(file)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // use super::*;
    // use std::collections::HashMap;
    // use std::path::PathBuf;
    // use std::str::FromStr;
}
