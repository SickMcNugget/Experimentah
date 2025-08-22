//! Functionality for queueing and running experiments.

use crate::parse::{
    Experiment, ExperimentRuns, Exporter, FileType, Host, RemoteExecution,
};
use crate::{
    command, file_to_deps_path, time_since_epoch, variation_dir_parts,
    DEFAULT_LIVE_DIR, DEFAULT_REMOTE_DIR, DEFAULT_RESULTS_DIR,
    DEFAULT_STORAGE_DIR, SUBDIRECTORIES,
};

/// Internally I've just set the queue to begin with a capacity of 32 potential [`ExperimentRuns`].
/// This isn't actually a cap on the number of experiments that can be run, but potentially in the
/// future we may have some guidance as to what a reasonable limit should be.
const DEFAULT_MAX_EXPERIMENTS: usize = 32;

/// When there are no experiments inside of the experiment queue, we poll the queue periodically to
/// determine whether there are any experiments available. This is the delay between successive
/// checks on the queue.
const DEFAULT_POLL_SLEEP: Duration = Duration::from_millis(250);

static KILL_EXPORTERS: AtomicBool = AtomicBool::new(false);

use axum::extract::ws::{Message, Utf8Bytes};
use log::{debug, error, info};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

use crate::command::{ExecutionResult, ExecutionType, ShellCommand, SpawnType};
use crate::session::{self, Sessions};

/// A specialised [`Result`] type for runtime problems with experiments
///
/// This type is broadly used across [`crate::run`] for any operation which may produce an
/// error.
///
/// This typedef is generally used to avoid writing out [`crate::run::Error`] directly and is
/// otherwise a direct mapping to [`Result`].
pub type Result<T> = std::result::Result<T, Error>;

/// We represent exporters internally by the Child-type process that they are. Since different
/// libraries have different implementations of a Child type ([`tokio::process::Child`],
/// [`openssh::Child`]), we have to have an enum that can handle both cases.
///
/// The [`Exporters`] type represents this mapping.
pub type Exporters = HashMap<String, Vec<SpawnType>>;

/// To create long-running exporters, we spawn them in the background and retain a handle to them
/// for the rest of the experiment.
/// The [`ExporterHandles`] type represents this mapping
pub type ExportersHandles = HashMap<String, ExporterHandles>;

/// Since each exporter can have multiple hosts within it's configuration, we expect that every
/// exporter will be spawned on multiple hosts, meaning that the smallest divisible unit for
/// spawned exporters should be a vector of them.
pub type ExporterHandles = Vec<JoinHandle<Result<()>>>;

/// The error type for runtime errors.
///
/// Errors are generally handled internally by a main-loop function, which
/// is executed via the [`ExperimentRunner::start`] function which is public to the user. These
/// errors usally wrap an SSH error, since they are the most frequent source of runtime problems.
///
/// All errors include additional context that explains *when* the error occurred during experiment
/// runtime.
#[derive(Debug)]
pub enum Error {
    IOError(String, std::io::Error),
    TimeError(String, std::time::SystemTimeError),
    CommandError(String, crate::command::Error),
    Generic(String),
    JoinError(String, tokio::task::JoinError),
    OpenSSHError(String, openssh::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Error::IOError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            Error::TimeError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            Error::CommandError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            Error::JoinError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            Error::OpenSSHError(ref message, ref source) => {
                write!(f, "{message}: {source}")
            }
            Error::Generic(ref message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Error::IOError(.., ref source) => Some(source),
            Error::TimeError(.., ref source) => Some(source),
            Error::CommandError(.., ref source) => Some(source),
            Error::JoinError(.., ref source) => Some(source),
            Error::OpenSSHError(.., ref source) => Some(source),
            Error::Generic(..) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    /// Converts from an [`std::io::Error`] into an [`Error::IOError`] with a predefined "I/O error"
    /// context message.
    fn from(value: std::io::Error) -> Self {
        Error::IOError("I/O error".to_string(), value)
    }
}

impl From<(&str, std::io::Error)> for Error {
    /// Converts from an [`std::io::Error`] into an [`Error::IOError`] with a custom context
    /// message. This is an ergonomic inclusion, and converts the &str into a String.
    fn from(value: (&str, std::io::Error)) -> Self {
        Error::IOError(value.0.to_string(), value.1)
    }
}

impl From<(String, std::io::Error)> for Error {
    /// Converts from an [`std::io::Error`] into an [`Error::IOError`] with a custom context
    /// message.
    fn from(value: (String, std::io::Error)) -> Self {
        Error::IOError(value.0, value.1)
    }
}

impl From<std::time::SystemTimeError> for Error {
    /// Converts from an [`std::time::SystemTimeError`] into an [`Error::TimeError`] with a
    /// predefined "System Time error" context message.
    fn from(value: std::time::SystemTimeError) -> Self {
        Error::TimeError("System Time error".to_string(), value)
    }
}

impl From<command::Error> for Error {
    /// Converts from a [`command::Error`] into an [`Error::CommandError`] with a
    /// predefined "Command error" context message.
    fn from(value: command::Error) -> Self {
        Error::CommandError("Command error".to_string(), value)
    }
}

impl From<session::Error> for Error {
    /// Converts from a [`crate::session::Error`] into an [`Error::Generic`] with the message
    /// directly taken from [`crate::session::Error`] (without context)
    fn from(value: session::Error) -> Self {
        Error::Generic(value.to_string())
    }
}

impl From<String> for Error {
    /// Converts from a String message to an [`Error::Generic`].
    fn from(value: String) -> Self {
        Error::Generic(value)
    }
}

impl From<&str> for Error {
    /// Converts from a &str message to an [`Error::Generic`].
    fn from(value: &str) -> Self {
        Error::Generic(value.to_string())
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(value: tokio::task::JoinError) -> Self {
        Self::JoinError("tokio task join error".to_string(), value)
    }
}

impl From<openssh::Error> for Error {
    fn from(value: openssh::Error) -> Self {
        Self::OpenSSHError("tokio task join error".to_string(), value)
    }
}

/// The [`ExperimentRunner`] needs a queue for receiving and running experiments.
pub type ExperimentQueue = VecDeque<ExperimentRuns>;

/// An [`ExperimentRunner`] provides the main functionality of Experimentah. It has a queue along
/// with multiple metrics for inspecting the current state of the controller.
pub struct ExperimentRunner {
    /// Contains variables that relevant only to the current experiment being run.
    /// These variables need to accessible from outside the experiment runner (for API
    /// endpoints to view them).
    pub current: ExperimentRunnerCurrent,
    /// A configuration which mainly contains important file paths for storing
    /// experiment/controller information.
    pub configuration: ExperimentRunnerConfiguration,
    /// The queue containing experiments sent in from clients.
    pub experiment_queue: Mutex<ExperimentQueue>,
}

/// Stores important information inside an [`ExperimentRunner`] that is expected to clear at the
/// end of a variation and change during a run.
#[derive(Default)]
pub struct ExperimentRunnerCurrent {
    /// When an experiment is running, this contains the name of the experiment
    pub experiment_name: Arc<Mutex<Option<String>>>,
    /// The timestamp (in milliseconds) when the current experiment started running.
    /// Note that this timestamp is not per-variation.
    pub ts: Arc<Mutex<u128>>,
    /// When an experiment is running, this contains the total number of runs that the experiment
    /// will be repeated for.
    pub runs: AtomicU16,
    /// When an experiment is running, this contains the current run number that the experiment is
    /// up to.
    pub run: AtomicU16,

    /// Contains a mapping from hosts to Sessions (for running commands) for the current
    /// experiment.
    pub sessions: Arc<RwLock<Sessions>>,
    /// Contains a list of the running exporters for the current experiment.
    pub exporters: Arc<RwLock<ExportersHandles>>,
    // pub experiment_directory: Arc<Mutex<PathBuf>>,
}

impl ExperimentRunnerCurrent {
    async fn reset(&self) {
        *self.experiment_name.lock().await = None;
        *self.ts.lock().await = 0;
        self.runs.store(0, Ordering::Relaxed);
        self.run.store(0, Ordering::Relaxed);
        self.exporters.write().await.clear();
        self.sessions.write().await.clear();
    }

    async fn set_experiment_name(&self, experiment: &Experiment) {
        *self.experiment_name.lock().await = Some(experiment.name.clone());
    }

    pub async fn status(&self) -> String {
        let experiment_name = self.experiment_name.lock().await;
        let run = self.run.load(Ordering::Relaxed);
        let runs = self.runs.load(Ordering::Relaxed);
        let sessions = self.sessions.read().await;
        let exporters = self.exporters.read().await;

        let experiment_name =
            if let Some(experiment_name) = experiment_name.as_ref() {
                experiment_name
            } else {
                &"None".to_string()
            };

        format!("experiment_name: {}, {run}/{runs} runs, {} unique hosts, {} exporters spawned", experiment_name, sessions.len(), exporters.len())
    }

    pub async fn experiment_directory(
        &self,
        experimentation_directory: &Path,
    ) -> PathBuf {
        let ts = self.ts.lock().await;
        assert!(
            *ts != 0,
            "This function can't be used until the timestamp has been set"
        );

        PathBuf::from(format!("{}/{}", experimentation_directory.display(), ts))
    }

    pub async fn repeat_directory(
        &self,
        experimentation_directory: &Path,
    ) -> PathBuf {
        let run = self.run.load(Ordering::Relaxed);
        assert!(run > 0);

        self.experiment_directory(experimentation_directory)
            .await
            .join(run.to_string())
    }

    pub async fn variation_directory(
        &self,
        experimentation_directory: &Path,
    ) -> PathBuf {
        let experiment_name = self.experiment_name.lock().await;
        let experiment_name = experiment_name.as_ref().expect("This function should only be called once the experiment name has been set!");

        self.repeat_directory(experimentation_directory)
            .await
            .join(experiment_name)
    }
}

/// The [`ExperimentRunner`] requires a great deal of configuration options (paths, etc) which it
/// must maintain a reference to. These fields are stored in this configuration struct.
pub struct ExperimentRunnerConfiguration {
    /// The amount of time to wait when checking the ExperimentRunner queue for new experiments.
    pub poll_sleep: Duration,
    /// The maximum number of experiments that can be stored at one time within the
    /// ExperimentRunner queue.
    pub max_experiments: usize,
    /// The storage directory to use for storing experiment data
    pub storage_dir: PathBuf,
    /// The subdirectory underneath [`Self::storage_dir`] and [`Self::experimentation_dir`] which experiment
    /// results are stored in
    pub results_dir: PathBuf,
    /// The directory to use on remote hosts when running experiments.
    ///
    /// NOTE: This is likely to be deprecated in favour of a directory that an SSH user is
    /// guaranteed to have access to. (likely to be either $HOME or /tmp).
    pub experimentation_dir: PathBuf,

    /// The subdirectory underneath [`Self::experimentation_dir`] to store
    /// information relating to currently running exporters
    pub live_dir: PathBuf,
}

impl Default for ExperimentRunnerConfiguration {
    fn default() -> Self {
        Self {
            poll_sleep: DEFAULT_POLL_SLEEP,
            max_experiments: DEFAULT_MAX_EXPERIMENTS,
            storage_dir: PathBuf::from(DEFAULT_STORAGE_DIR),
            results_dir: PathBuf::from(DEFAULT_RESULTS_DIR),
            experimentation_dir: PathBuf::from(DEFAULT_REMOTE_DIR),
            live_dir: PathBuf::from(DEFAULT_LIVE_DIR),
        }
    }
}

impl ExperimentRunnerConfiguration {
    /// Returns directories that need to be created for experiments to run correctly
    fn directories(&self) -> Vec<PathBuf> {
        let mut directories = Vec::new();

        for directory in SUBDIRECTORIES {
            directories.push(
                self.experimentation_dir
                    .join(&self.live_dir)
                    .join(directory),
            );
        }
        directories
    }

    // TODO(joren): live_dir almost definitely needs to be changed so that it makes use of the
    // FileType enum. That way we don't have floating magical funny strings around the place.
    fn live_dir(&self, subdir: &str) -> PathBuf {
        self.experimentation_dir.join(&self.live_dir).join(subdir)
    }
}

impl Default for ExperimentRunner {
    fn default() -> Self {
        let current: ExperimentRunnerCurrent = Default::default();
        let configuration: ExperimentRunnerConfiguration = Default::default();
        let experiment_queue =
            Mutex::new(VecDeque::with_capacity(configuration.max_experiments));

        Self {
            current,
            configuration,
            experiment_queue,
        }
    }
}

impl ExperimentRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn experiment_directory(&self) -> PathBuf {
        self.current
            .experiment_directory(&self.configuration.experimentation_dir)
            .await
    }

    pub async fn repeat_directory(&self) -> PathBuf {
        self.current
            .repeat_directory(&self.configuration.experimentation_dir)
            .await
    }

    pub async fn variation_directory(&self) -> PathBuf {
        self.current
            .variation_directory(&self.configuration.experimentation_dir)
            .await
    }

    /// Add a new experiment to the runner
    pub async fn enqueue(&self, experiments: ExperimentRuns) {
        self.experiment_queue.lock().await.push_back(experiments);
    }

    async fn wait_for_experiments(&self) -> ExperimentRuns {
        loop {
            let mut queue = self.experiment_queue.lock().await;
            match queue.pop_front() {
                Some(experiment_runs) => {
                    self.current
                        .runs
                        .store(experiment_runs.0, Ordering::Relaxed);
                    return experiment_runs;
                }
                None => tokio::time::sleep(self.configuration.poll_sleep).await,
            }
        }
    }

    // I really have no idea what I want to do for the ExperimentRunner main loop.
    // Up until this point, start has been both the entry point and the houses the entire loop
    // for ExperimentRunner. I have a feeling that separating the main loop from a user trying
    // to start the main loop will make error handling easier, but I'm just unsure overall as
    // to what the state of this struct needs to be for 1.0.
    // - Joren
    //
    /// Initialise the experiment runner. After this function has been run, the  ExperimentRunner
    /// will begin to consume from it's internal queue until there is nothing left to consume.
    /// Experiments can always be added to an experiment runner before it has been started, so
    /// there may be functionality in the future to pause the runner.
    pub async fn start(&self) {
        info!("Experiment runner now running..");
        loop {
            if let Err(e) = self.main_loop().await {
                error!("Experiment failure, aborting: {e}");
            }
        }
    }

    /// Internally, this is the bread and butter of Experimentah. Everything happens inside of this
    /// function, and it makes it pretty obvious (in my opinion) as to what the pipeline for
    /// running experiments looks like. This loop tries to retrieve experiments from the queue, and
    /// once it succeeds, it runs those experiments until completion for the number of repeats
    /// specified.
    async fn main_loop(&self) -> Result<()> {
        loop {
            let (runs, experiments) = self.wait_for_experiments().await;

            // We create the timestamp once for each full experiment
            // This also sets the experiment directory (indirectly)
            *self.current.ts.lock().await = time_since_epoch()?.as_millis();

            info!(
                "Experiment with {} variations received ({} repeats).",
                experiments.len(),
                runs
            );

            // Generate the sessions we need by connecting to hosts
            self.connect_to_hosts(&experiments).await?;

            // Ensure that our 'well-known' directories are present
            self.make_configuration_directories().await?;

            // TODO(joren): All important files need to be uploaded to the correct
            // directory on all the remote sessions ahead of the actual experiment runs.
            // Whilst we want our results to populate the
            // /srv/experimentah/<timestamp>/<repeat_no>/ directory, we want all
            // resources for that specific experiment to instead populate the
            // /srv/experimentah/<timestamp>/ directory.
            self.upload_files_for_experiments(&experiments).await?;

            // TODO(joren): For dependencies, the current solution is to upload them normally, and
            // then adjust them afterwards to be in the correct subdirectory. It's definitely dumb
            // to do it this way but it's the path of least resistance at the moment.
            self.correct_dependencies(&experiments).await?;

            for run in 1..=runs {
                self.current.run.store(run, Ordering::Relaxed);

                for experiment in experiments.iter() {
                    self.current.set_experiment_name(experiment).await;

                    // Create the variation_directory
                    self.make_variation_directory().await?;

                    info!(
                        "Running experiment '{}' (repeat {}/{})...",
                        &experiment.name,
                        run,
                        self.current.runs.load(Ordering::Relaxed)
                    );

                    // Here we symlink our script dependencies within the current repeat directory
                    self.symlink_dependencies(
                        &experiment.execute,
                        &experiment.dependencies,
                    )
                    .await?;

                    debug!(
                        "Symlinked dependencies for '{:?}': {:?}",
                        &experiment.execute, &experiment.dependencies
                    );

                    self.start_exporters(&experiment.exporters).await?;
                    tokio::time::sleep(Duration::from_secs(60)).await;

                    info!("Started exporters");

                    // TODO(joren): We want to make sure that our setup/teardown
                    // is run from the *variation* directory, and not the experiment
                    // directory. However, we want our setup/teardown to be able to find
                    // the script it needs to run, which should live inside of the
                    // experiment directory.
                    //
                    // Therefore this function needs to change it's responsibilities

                    self.run_remote_executions(&experiment.setup).await?;
                    info!("Variation setup complete");

                    self.run_remote_execution(&experiment.execute).await?;
                    info!("Variation complete");

                    self.run_remote_executions(&experiment.teardown).await?;
                    info!("Variation teardown complete");

                    // Time for us to close our exporters
                    {
                        KILL_EXPORTERS.store(true, Ordering::Relaxed);

                        let mut exporters =
                            self.current.exporters.write().await;
                        for (_exporter_name, handles) in exporters.drain() {
                            for handle in handles {
                                handle.await??;
                            }
                        }

                        assert!(exporters.len() == 0);
                        KILL_EXPORTERS.store(false, Ordering::Relaxed);
                    }
                    info!("Stopped all exporters for variation");

                    self.unlink_dependencies(
                        &experiment.execute,
                        &experiment.dependencies,
                    )
                    .await?;
                    debug!("Unliked dependencies for variation");

                    let result_directory = &self
                        .configuration
                        .storage_dir
                        .join(&self.configuration.results_dir);

                    self.collect_results(result_directory).await?;
                    info!("Collected results for variation");
                }
            }
            // We want to make sure there is no dangling data.
            self.current.reset().await;

            info!("Finished experiments");
        }
    }

    /// Since [`Sessions`] are just a mapping of host addresses to SSH sessions, this functions
    /// allows us to filter down a large mapping of [`Sessions`] to those that are important to our
    /// current scope.
    async fn filter_sessions(&self, addresses: &[String]) -> Sessions {
        self.current
            .sessions
            .read()
            .await
            .iter()
            .filter(|(key, _)| addresses.contains(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// This is the same as the [`filter_sessions`] function, except it takes advantage of the fact
    /// that many structs in Experimentah contain a list of [`Host`]s that we can filter on. Just a
    /// bit of syntactic sugar.
    async fn filter_host_sessions(&self, hosts: &[Host]) -> Sessions {
        let addresses: Vec<String> =
            hosts.iter().map(|host| &host.address).cloned().collect();

        self.filter_sessions(&addresses).await
    }

    // TODO(joren): This function shouldn't even exist.
    // We should make sure that we upload our dependencies into the correct directories from the
    // beginning of each experiment. This workaround doesn't make much sense.
    async fn correct_dependencies(
        &self,
        experiments: &[Experiment],
        // experiment_directory: &Path,
    ) -> Result<()> {
        let sessions = &*self.current.sessions.read().await;

        for experiment in experiments.iter() {
            let execute = &experiment.execute;
            let experiment_directory = self.experiment_directory().await;

            let execute_directory = experiment_directory.join(format!(
                "{}-deps",
                execute
                    .scripts
                    .first()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
            ));

            session::make_directory(sessions, &execute_directory).await?;

            let mut shell_command = ShellCommand::from_command("mv");
            let mut args = vec![];
            for script in experiment.dependencies.iter() {
                args.push(
                    script.file_name().unwrap().to_string_lossy().to_string(),
                );
            }
            args.push(
                execute_directory
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            );
            shell_command.args(&args);
            shell_command.working_directory(&*experiment_directory);

            command::run_command(
                sessions,
                shell_command,
                ExecutionType::Output,
            )
            .await?;
        }
        Ok(())
    }

    async fn setup_exporters(&self, exporters: &[Exporter]) -> Result<()> {
        let mut futures: Vec<JoinHandle<Result<()>>> = Vec::new();

        let variation_directory = self.variation_directory().await;

        let exporters_clone = exporters.to_vec();
        for exporter in exporters_clone.into_iter() {
            let exporter_sessions =
                self.filter_host_sessions(&exporter.hosts).await;

            let exporter_clone = exporter.clone();
            let variation_directory_clone = variation_directory.to_path_buf();

            futures.push(tokio::task::spawn(async move {
                for setup in exporter_clone.setup.iter() {
                    let command_args =
                        shlex::split(setup).ok_or_else(|| {
                            Error::from(format!(
                                "Invalid setup command for exporter '{}': {}",
                                exporter.name, setup
                            ))
                        })?;
                    let mut shell_command =
                        ShellCommand::from_command_args(&command_args);
                    shell_command.working_directory(&variation_directory_clone);
                    command::run_command(
                        &exporter_sessions,
                        shell_command,
                        ExecutionType::Output,
                    )
                    .await?;
                }
                Ok(())
            }));
        }

        for future in futures {
            future.await??;
        }

        Ok(())
    }

    /// Starts long-running exporters, which are expected to collect
    /// system-level metrics whilst an experiment is running.
    /// Think tools like 'sar' or 'ipmitool'
    ///
    async fn start_exporters(&self, exporters: &[Exporter]) -> Result<()> {
        let mut exporters_handles = ExportersHandles::new();

        let variation_directory = self.variation_directory().await;

        // We now perform exporter setup in parallel, nice!
        // NOTE: each step of the setup for each exporter is still serial, as intended.
        self.setup_exporters(exporters).await?;

        for exporter in exporters.iter() {
            let exporter_sessions =
                self.filter_host_sessions(&exporter.hosts).await;

            let mut exporter_files = exporter.shell_files();
            exporter_files.set_base(self.configuration.live_dir("exporter"));

            let parts = shlex::split(&exporter.command).ok_or_else(|| {
                Error::from(format!(
                    "Invalid exporter command for exporter '{}': {}",
                    exporter.name, &exporter.command
                ))
            })?;
            let mut shell_command = ShellCommand::from_command_args(&parts);
            shell_command
                .working_directory(&variation_directory)
                .stdout_file(&exporter_files.stdout)
                .stderr_file(&exporter_files.stderr)
                .pid_file(&exporter_files.pid)
                .advisory_lock_file(&exporter_files.lock);

            let exporter_processes = command::run_command(
                &exporter_sessions,
                shell_command,
                ExecutionType::Spawn,
            )
            .await?;

            let exporter_processes: Vec<SpawnType> = exporter_processes
                .into_iter()
                .map(|exporter_process| {
                    if let ExecutionResult::Spawn(child) = exporter_process {
                        child
                    } else {
                        // Since we specified ExecutionType::Spawn, we should definitely be getting
                        // ExecutionResult::Spawn back from this function
                        panic!();
                    }
                })
                .collect();

            // Here, we register the exporters with our experiment runner

            let mut handles: ExporterHandles = Vec::new();
            for exporter_process in exporter_processes.into_iter() {
                // let exporter_name_clone = exporter.name.clone();
                let pid_file = exporter_files.pid.clone();
                handles.push(tokio::spawn(async move {
                    while !KILL_EXPORTERS.load(Ordering::Relaxed) {
                        tokio::time::sleep(DEFAULT_POLL_SLEEP).await;
                    }

                    match exporter_process {
                        SpawnType::SSH(process) => {
                            // We need to wait until an atomic value is set to KILL our exporter
                            // We need to kill the remote process
                            let session = process.session();
                            let command = ShellCommand::from_command_args(&[
                                "kill".into(),
                                "-KILL".into(),
                                format!(
                                    "$(cat {})",
                                    pid_file.to_string_lossy()
                                ),
                            ]);
                            command::run_command_openssh_session(
                                session,
                                command,
                                ExecutionType::Status,
                            )
                            .await?;
                        }
                        SpawnType::Tokio(mut process) => {
                            process.kill().await?;
                        }
                    }
                    Ok(())
                }));
            }

            exporters_handles.insert(exporter.name.clone(), handles);
        }

        *self.current.exporters.write().await = exporters_handles;

        Ok(())
    }

    //TODO(joren): We need remote commands to write their outputs to a file somewhere.
    //This is so we can retrieve the information on the client if desired.
    //It's important to remember that we want to avoid streaming as much as possible
    //unless the client specifically requests it for debug purposes.
    async fn run_remote_executions(
        &self,
        remote_executions: &[RemoteExecution],
    ) -> Result<()> {
        for remote_execution in remote_executions.iter() {
            self.run_remote_execution(remote_execution).await?;
        }
        Ok(())
    }

    async fn run_remote_execution(
        &self,
        remote_execution: &RemoteExecution,
    ) -> Result<()> {
        let setup_sessions =
            self.filter_host_sessions(&remote_execution.hosts).await;

        let experiment_directory = self.experiment_directory().await;
        let variation_directory = self.variation_directory().await;

        for remote_script in remote_execution.remote_scripts().iter() {
            // This assertion only checks locally.
            // Scripts should exist on the remote at this point.
            // assert!(remotescript.exists());

            let remote_script = experiment_directory.join(remote_script);
            session::run_script_at(
                &setup_sessions,
                remote_script,
                &variation_directory,
            )
            .await?;
            // TODO(joren): Handle error case
            // let remote_script =
            //     experiment_directory.join(script.file_name().unwrap());
            // ssh::run_script(&setup_sessions, &remote_script).await?;
        }

        Ok(())
    }

    async fn make_configuration_directories(&self) -> session::Result<()> {
        session::make_directories(
            &*self.current.sessions.read().await,
            &self.configuration.directories(),
        )
        .await
    }
    async fn make_variation_directory(&self) -> session::Result<()> {
        session::make_directory(
            &*self.current.sessions.read().await,
            &self.variation_directory().await,
        )
        .await
    }

    async fn upload_files_for_experiments(
        &self,
        experiments: &[Experiment],
    ) -> session::Result<()> {
        let files = unique_files_for_experiments(experiments);
        session::upload(
            &*self.current.sessions.read().await,
            &files,
            &self.experiment_directory().await,
        )
        .await
    }

    async fn connect_to_hosts(
        &self,
        experiments: &[Experiment],
    ) -> session::Result<()> {
        let hosts = unique_hosts_for_experiments(experiments);
        let sessions = session::connect_to_hosts(&hosts).await?;
        *self.current.sessions.write().await = sessions;
        Ok(())
    }

    async fn symlink_dependencies(
        &self,
        remote_execution: &RemoteExecution,
        dependencies: &[PathBuf],
    ) -> Result<Vec<ExecutionResult>> {
        let filter_sessions =
            self.filter_host_sessions(&remote_execution.hosts).await;

        let deps_path = file_to_deps_path(
            &self.experiment_directory().await,
            remote_execution.scripts.first().unwrap(),
        );

        let command_args: Vec<String> = [
            "ln".to_string(),
            "-s".to_string(),
            "-t".to_string(),
            self.variation_directory()
                .await
                .to_string_lossy()
                .to_string(),
        ]
        .into_iter()
        .chain(dependencies.iter().map(|dep| {
            deps_path
                .join(dep.file_name().unwrap())
                .to_string_lossy()
                .to_string()
        }))
        .collect();

        let shell_command = ShellCommand::from_command_args(&command_args);
        command::run_command(
            &filter_sessions,
            shell_command,
            ExecutionType::Output,
        )
        .await
        .map_err(Error::from)
    }

    async fn unlink_dependencies(
        &self,
        remote_execution: &RemoteExecution,
        dependencies: &[PathBuf],
    ) -> Result<()> {
        let filter_sessions =
            self.filter_host_sessions(&remote_execution.hosts).await;

        let variation_directory = self.variation_directory().await;

        let args: Vec<String> = dependencies
            .iter()
            .map(|dependency| {
                variation_directory
                    .join(dependency.file_name().unwrap())
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        let mut shell_command = ShellCommand::from_command("rm");
        shell_command.args(&args);
        command::run_command(
            &filter_sessions,
            shell_command,
            ExecutionType::Output,
        )
        .await?;
        Ok(())
        // .map_err(Error::from)
    }

    async fn collect_results(&self, results_dir: &Path) -> Result<()> {
        let variation_directory = self.variation_directory().await;

        //TODO(joren): We can just store the timestamp in the ExperimentRunnerCurrent struct so we
        //don't even need this function anymore.
        let (experiment_name, run, ts) =
            variation_dir_parts(&variation_directory);

        info!(
            "Collecting results for experiment '{}' (repeat {})",
            experiment_name, run
        );

        let local_results_dir = std::path::absolute(
            results_dir.join(format!("{ts}/{run}/{experiment_name}")),
        )
        .map_err(|e| {
            Error::from((
                "Failed to canonicalize local results dir".to_string(),
                e,
            ))
        })?;

        session::download(
            &*self.current.sessions.read().await,
            &[variation_directory],
            local_results_dir,
        )
        .await?;

        Ok(())
    }

    pub async fn tail_stdout_stderr(
        &self,
        thing: crate::routes::Thing,
    ) -> Result<(UnboundedReceiver<Message>, UnboundedReceiver<Message>)> {
        let sessions = self.filter_sessions(&[thing.host]).await;

        let stdout = format!("{}.stdout", thing.identifier);
        let stderr = format!("{}.stderr", thing.identifier);

        let mut stdout_shell_command = ShellCommand::from_command("tail");
        stdout_shell_command
            .args(&["-c".to_string(), "+0".to_string(), stdout])
            .working_directory(
                self.configuration.live_dir(&thing.filetype.to_string()),
            );

        let mut stderr_shell_command = ShellCommand::from_command("tail");
        stderr_shell_command
            .args(&["-c".to_string(), "+0".to_string(), stderr])
            .working_directory(
                self.configuration.live_dir(&thing.filetype.to_string()),
            );

        let stdout_handle = command::run_command(
            &sessions,
            stdout_shell_command,
            ExecutionType::Spawn,
        )
        .await?;

        let stdout_handle = stdout_handle
            .into_iter()
            .map(|stdout_thread| {
                if let ExecutionResult::Spawn(child) = stdout_thread {
                    child
                } else {
                    // Since we specified ExecutionType::Spawn, we should definitely be getting
                    // ExecutionResult::Spawn back from this function
                    panic!();
                }
            })
            .collect::<Vec<SpawnType>>()
            .pop()
            .expect("We should have had a process at this point");

        let (stdout_sender, stdout_recv) =
            tokio::sync::mpsc::unbounded_channel::<Message>();

        match stdout_handle {
            SpawnType::SSH(mut child) => tokio::spawn(async move {
                let mut stdout = child.stdout().take().unwrap();
                let mut buf = [0; 100];
                loop {
                    let n = stdout.read(&mut buf).await?;
                    if n == 0 {
                        return Ok::<(), Error>(());
                    }
                    let message =
                        Message::text(String::from_utf8(buf.to_vec()).unwrap());
                    stdout_sender.send(message);
                }
            }),
            SpawnType::Tokio(child) => tokio::spawn(async move {
                let mut stdout = child.stdout.unwrap();
                let mut buf = [0; 100];
                loop {
                    let n = stdout.read(&mut buf).await?;
                    if n == 0 {
                        return Ok(());
                    }
                    let message =
                        Message::text(String::from_utf8(buf.to_vec()).unwrap());
                    stdout_sender.send(message);
                }
            }),
        };

        let stderr_handle = command::run_command(
            &sessions,
            stderr_shell_command,
            ExecutionType::Spawn,
        )
        .await?;

        let stderr_handle = stderr_handle
            .into_iter()
            .map(|stderr_thread| {
                if let ExecutionResult::Spawn(child) = stderr_thread {
                    child
                } else {
                    // Since we specified ExecutionType::Spawn, we should definitely be getting
                    // ExecutionResult::Spawn back from this function
                    panic!();
                }
            })
            .collect::<Vec<SpawnType>>()
            .pop()
            .expect("We should have had a process at this point");

        let (stderr_sender, stderr_recv) =
            tokio::sync::mpsc::unbounded_channel::<Message>();

        match stderr_handle {
            SpawnType::SSH(mut child) => tokio::spawn(async move {
                // Yes, the stderr is returned from stdout
                let mut stderr = child.stdout().take().unwrap();
                let mut buf = [0; 100];
                loop {
                    let n = stderr.read(&mut buf).await?;
                    if n == 0 {
                        return Ok::<(), Error>(());
                    }
                    let message =
                        Message::text(String::from_utf8(buf.to_vec()).unwrap());
                    stderr_sender.send(message);
                }
            }),
            SpawnType::Tokio(child) => tokio::spawn(async move {
                let mut stderr = child.stdout.unwrap();
                let mut buf = [0; 100];
                loop {
                    let n = stderr.read(&mut buf).await?;
                    if n == 0 {
                        return Ok(());
                    }
                    let message =
                        Message::text(String::from_utf8(buf.to_vec()).unwrap());
                    stderr_sender.send(message);
                }
            }),
        };

        Ok((stdout_recv, stderr_recv))
    }
}

/// Loops through a slice of experiments and returns
/// unique filepaths for that slice.
fn unique_files_for_experiments(experiments: &[Experiment]) -> Vec<&Path> {
    let mut files: Vec<&Path> = experiments
        .iter()
        .flat_map(|experiment| experiment.files())
        .collect();

    files.sort();
    files.dedup();
    files
}

/// Retrieves all the unique hosts used across a list of experiments.
/// This is useful for performing pre-run and post-run actions on these
/// hosts that are necessary for experimentah to function correctly.
/// These hosts should be filtered as required by each individual experiment variation.
fn unique_hosts_for_experiments(experiments: &[Experiment]) -> Vec<&String> {
    let mut hosts: Vec<&String> = experiments
        .iter()
        .flat_map(|experiment| experiment.hosts())
        .collect();

    hosts.sort();
    hosts.dedup();
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn runner() -> ExperimentRunner {
        let runner = ExperimentRunner::new();

        let ts = time_since_epoch()
            .expect("We shouldn't be failing to get time")
            .as_millis();
        let experiment_name = String::from("test_experiment_name");
        let run: u16 = 1;
        let runs: u16 = 10;
        let sessions = Sessions::default();
        let exporters = ExportersHandles::default();

        *runner.current.ts.lock().await = ts;
        *runner.current.experiment_name.lock().await = Some(experiment_name);
        runner.current.run.store(run, Ordering::Relaxed);
        runner.current.runs.store(runs, Ordering::Relaxed);
        *runner.current.sessions.write().await = sessions;
        *runner.current.exporters.write().await = exporters;

        runner
    }

    #[tokio::test]
    async fn runner_reset() {
        let runner = runner().await;
        runner.current.reset().await;

        let ts = runner.current.ts.lock().await;
        let experiment_name = runner.current.experiment_name.lock().await;
        let run: u16 = runner.current.run.load(Ordering::Relaxed);
        let runs: u16 = runner.current.runs.load(Ordering::Relaxed);
        let sessions = runner.current.sessions.read().await;
        let exporters = runner.current.exporters.read().await;

        assert_eq!(*ts, 0);
        assert_eq!(*experiment_name, None);
        assert_eq!(run, 0);
        assert_eq!(runs, 0);
        assert!(sessions.is_empty());
        assert!(exporters.is_empty());
    }
}
