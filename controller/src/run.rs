//! Functionality for queueing and running experiments.

use crate::parse::{
    Experiment, ExperimentRuns, Exporter, Host, RemoteExecution,
};
use crate::{
    file_to_deps_path, time_since_epoch, variation_dir_parts, EXPORTER_DIR,
    REMOTE_DIR,
};

/// Internally I've just set the queue to begin with a capacity of 32 potential [`ExperimentRuns`].
/// This isn't actually a cap on the number of experiments that can be run, but potentially in the
/// future we may have some guidance as to what a reasonable limit should be.
const MAX_EXPERIMENTS: usize = 32;

/// When there are no experiments inside of the experiment queue, we poll the queue periodically to
/// determine whether there are any experiments available. This is the delay between successive
/// checks on the queue.
const RUN_POLL_SLEEP: Duration = Duration::from_millis(250);

use log::{debug, error, info};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;

use crate::ssh::{self, BackgroundProcesses, Sessions};
use crate::{RESULTS_DIR, STORAGE_DIR};

/// A specialised [`Result`] type for runtime problems with experiments
///
/// This type is broadly used across [`crate::run`] for any operation which may produce an
/// error.
///
/// This typedef is generally used to avoid writing out [`crate::run::Error`] directly and is
/// otherwise a direct mapping to [`Result`].
pub type Result<T> = std::result::Result<T, Error>;

/// To create long-running exporters, we spawn them in the background and
/// retain a handle to them for the rest of the experiment.
/// The [`Exporters`] type represents this mapping.
///
/// In the future, this type may also take into account exporters run on the local system, too.
pub type Exporters = HashMap<String, BackgroundProcesses>;

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
    Generic(String),
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

impl From<ssh::Error> for Error {
    /// Converts from a [`crate::ssh::Error`] into an [`Error::Generic`] with the message
    /// directly taken from [`crate::ssh::Error`] (without context)
    fn from(value: ssh::Error) -> Self {
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

/// The [`ExperimentRunner`] needs a queue for receiving and running experiments.
/// This type contains that queue, which is within a mutex for internal mutability.
pub type ExperimentQueue = Mutex<VecDeque<ExperimentRuns>>;

/// An [`ExperimentRunner`] provides the main functionality of Experimentah. It has a queue along
/// with multiple metrics for inspecting the current state of the controller.
pub struct ExperimentRunner {
    /// When an experiment is running, this contains the name of the experiment
    pub current_experiment: Mutex<Option<String>>,
    /// When an experiment is running, this contains the total number of runs that the experiment
    /// will be repeated for.
    pub current_runs: AtomicU16,
    /// When an experiment is running, this contains the current run number that the experiment is
    /// up to.
    pub current_run: AtomicU16,
    /// The queue containing experiments sent in from clients.
    pub experiment_queue: ExperimentQueue,
}

impl Default for ExperimentRunner {
    fn default() -> Self {
        Self {
            current_experiment: Mutex::new(None),
            current_runs: 0.into(),
            current_run: 0.into(),
            experiment_queue: Mutex::new(VecDeque::with_capacity(
                MAX_EXPERIMENTS,
            )),
        }
    }
}

impl ExperimentRunner {
    pub fn new() -> Self {
        Self::default()
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
                    self.current_runs
                        .store(experiment_runs.0, Ordering::Relaxed);
                    return experiment_runs;
                }
                None => tokio::time::sleep(RUN_POLL_SLEEP).await,
            }
        }
    }

    async fn reset_metadata(&self) {
        self.current_runs.store(0, Ordering::Relaxed);
        self.current_run.store(0, Ordering::Relaxed);
        *self.current_experiment.lock().await = None;
    }

    async fn update_current_experiment(
        &self,
        experiment: &Experiment,
        run: u16,
    ) {
        info!(
            "Running experiment '{}' (repeat {}/{})...",
            &experiment.name,
            run,
            self.current_runs.load(Ordering::Relaxed)
        );
        let mut current_experiment = self.current_experiment.lock().await;
        *current_experiment = Some(experiment.name.clone());
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
            let ts = time_since_epoch()?.as_millis();

            info!(
                "Experiment with {} variations received ({} repeats).",
                experiments.len(),
                runs
            );

            let hosts = Self::unique_hosts_for_all_experiments(&experiments);
            let sessions = ssh::connect_to_hosts(&hosts).await?;

            let experiment_directory =
                PathBuf::from(format!("{REMOTE_DIR}/{ts}"));

            // Ensure that our 'well-known' directories are present
            ssh::make_directories(
                &sessions,
                &[Path::new(REMOTE_DIR), Path::new(EXPORTER_DIR)],
            )
            .await?;

            // TODO(joren): All important files need to be uploaded to the correct
            // directory on all the remote sessions ahead of the actual experiment runs.
            // Whilst we want our results to populate the
            // /srv/experimentah/<timestamp>/<repeat_no>/ directory, we want all
            // resources for that specific experiment to instead populate the
            // /srv/experimentah/<timestamp>/ directory.
            let files = Self::unique_files_for_all_experiments(&experiments);
            ssh::upload(&sessions, &files, &experiment_directory).await?;

            // TODO(joren): For dependencies, the current solution is to upload them normally, and
            // then adjust them afterwards to be in the correct subdirectory. It's definitely dumb
            // to do it this way but it's the path of least resistance at the moment.
            Self::correct_dependencies(
                &sessions,
                &experiments,
                &experiment_directory,
            )
            .await?;

            for run in 1..=runs {
                self.current_run.store(run, Ordering::Relaxed);

                let repeat_directory: &Path =
                    &experiment_directory.join(run.to_string());

                for experiment in experiments.iter() {
                    let variation_directory: &Path =
                        &repeat_directory.join(&experiment.name);

                    self.update_current_experiment(experiment, run).await;
                    ssh::make_directory(&sessions, variation_directory).await?;

                    // Here we symlink our script dependencies within the current repeat directory
                    Self::symlink_dependencies(
                        &sessions,
                        &experiment.execute,
                        &experiment.dependencies,
                        &experiment_directory,
                        variation_directory,
                    )
                    .await?;
                    debug!(
                        "Symlinked dependencies for '{:?}': {:?}",
                        &experiment.execute, &experiment.dependencies
                    );

                    let _exporters = Self::start_exporters(
                        &sessions,
                        &experiment.exporters,
                        &experiment_directory,
                        variation_directory,
                    )
                    .await?;
                    info!("Started exporters");

                    // TODO(joren): We want to make sure that our setup/teardown
                    // is run from the *variation* directory, and not the experiment
                    // directory. However, we want our setup/teardown to be able to find
                    // the script it needs to run, which should live inside of the
                    // experiment directory.
                    //
                    // Therefore this function needs to change it's responsibilities

                    Self::run_remote_executions(
                        &sessions,
                        &experiment.setup,
                        &experiment_directory,
                        variation_directory,
                    )
                    .await?;
                    info!("Variation setup complete");

                    Self::run_remote_execution(
                        &sessions,
                        &experiment.execute,
                        &experiment_directory,
                        variation_directory,
                    )
                    .await?;
                    info!("Variation complete");

                    // ssh::upload(&sessions, source_path, destination_path)

                    // run_variation(&sessions, &experiment);
                    Self::run_remote_executions(
                        &sessions,
                        &experiment.teardown,
                        &experiment_directory,
                        variation_directory,
                    )
                    .await?;
                    info!("Variation teardown complete");

                    Self::unlink_dependencies(
                        &sessions,
                        &experiment.execute,
                        &experiment.dependencies,
                        variation_directory,
                    )
                    .await?;
                    debug!("Unliked dependencies for variation");

                    Self::collect_results(&sessions, variation_directory)
                        .await?;
                    info!("Collected results for variation");
                }
            }
            info!("Finished experiments");

            self.reset_metadata().await;
        }
    }

    /// Since [`Sessions`] are just a mapping of host addresses to SSH sessions, this functions
    /// allows us to filter down a large mapping of [`Sessions`] to those that are important to our
    /// current scope.
    fn filter_sessions(sessions: &Sessions, addresses: &[String]) -> Sessions {
        sessions
            .iter()
            .filter(|(key, _)| addresses.contains(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// This is the same as the [`filter_sessions`] function, except it takes advantage of the fact
    /// that many structs in Experimentah contain a list of [`Host`]s that we can filter on. Just a
    /// bit of syntactic sugar.
    fn filter_host_sessions(sessions: &Sessions, hosts: &[Host]) -> Sessions {
        let addresses: Vec<String> =
            hosts.iter().map(|host| &host.address).cloned().collect();

        Self::filter_sessions(sessions, &addresses)
    }

    /// Retrieves all the unique hosts used across a list of experiments.
    /// This is useful for performing pre-run and post-run actions on these
    /// hosts that are necessary for experimentah to function correctly.
    /// These hosts should be filtered as required by each individual experiment variation.
    fn unique_hosts_for_all_experiments(
        experiments: &[Experiment],
    ) -> Vec<&String> {
        let mut hosts: Vec<&String> = experiments
            .iter()
            .flat_map(|experiment| experiment.hosts())
            .collect();

        hosts.sort();
        hosts.dedup();
        hosts
    }

    // TODO(joren): This function shouldn't even exist.
    // We should make sure that we upload our dependencies into the correct directories from the
    // beginning of each experiment. This workaround doesn't make much sense.
    async fn correct_dependencies(
        sessions: &Sessions,
        experiments: &[Experiment],
        experiment_directory: &Path,
    ) -> Result<()> {
        for experiment in experiments.iter() {
            let execute = &experiment.execute;
            // let fname = execute
            //     .scripts
            //     .first()
            //     .unwrap()
            //     .file_name()
            //     .unwrap()
            //     .to_string_lossy();
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

            ssh::make_directory(sessions, &execute_directory).await?;
            // ssh::run_command_at(sessions, &command_args, experiment_directory)
            //     .await?;

            let mut command_args = vec!["mv".to_string()];
            for script in experiment.dependencies.iter() {
                command_args.push(
                    script.file_name().unwrap().to_string_lossy().to_string(),
                );
            }
            command_args.push(
                execute_directory
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            );
            ssh::run_command_at(sessions, &command_args, experiment_directory)
                .await?;
        }
        Ok(())
    }

    /// Loops through a slice of experiments and returns
    /// unique filepaths for that slice.
    fn unique_files_for_all_experiments(
        experiments: &[Experiment],
    ) -> Vec<&Path> {
        let mut files: Vec<&Path> = experiments
            .iter()
            .flat_map(|experiment| experiment.files())
            .collect();

        files.sort();
        files.dedup();
        files
    }

    //     fn unique_dependencies_for_all_experiments(
    //         experiments: &[Experiment],
    //     ) -> Vec<&Path> {
    //         let mut files: Vec<&Path> = experiments.
    // iter().flat_map(|experiment| experiment.dependencies()).collect();
    //     }

    /// Starts long-running exporters, which are expected to collect
    /// system-level metrics whilst an experiment is running.
    /// Think tools like 'sar' or 'ipmitool'
    async fn start_exporters(
        sessions: &Sessions,
        exporters: &[Exporter],
        _experiment_directory: &Path,
        variation_directory: &Path,
    ) -> Result<Exporters> {
        let mut live_exporters: Exporters = Exporters::default();
        for exporter in exporters.iter() {
            let exporter_sessions =
                Self::filter_host_sessions(sessions, &exporter.hosts);

            // TODO(joren): It's okay now since we're early in development to do this serially, but
            // ideally in the future the setup process should be done in parallel, if possible.

            for setup in exporter.setup.iter() {
                // TODO(joren): Handle shlex error
                let setup_comm = shlex::split(setup).unwrap();
                ssh::run_command_at(
                    &exporter_sessions,
                    &setup_comm,
                    variation_directory,
                )
                .await?;
            }

            let (stdout, stderr) = exporter.redir_files();

            //TODO(joren): Handle shlex error
            let comm: Vec<String> = shlex::split(&exporter.command)
                .unwrap()
                .into_iter()
                .chain([format!(">{stdout}"), format!("2>{stderr}")])
                .collect();

            let exporter_processes = ssh::run_background_command_at(
                &exporter_sessions,
                &comm,
                variation_directory,
            )
            .await?;

            live_exporters.insert(exporter.name.clone(), exporter_processes);
            // tokio::time::sleep(Duration::from_secs(5)).await;

            // info!("Started exporter '{}'", exporter.name);
            // dbg!(running_exporters);

            // tokio::time::sleep(Duration::from_secs(60)).await;

            // The exporter needs a file to be created which we can refer to in the event
            // of an experimentah crash.
            // We can make use of the flock command when running our exporters
            // match ssh::run_background_command(&exporter_sessions, &comm).await {
            //     Ok(child) => info!("Started exporter '{}'", exporter.name),
            //     Err(e) => {
            //         error!(
            //             "Failed to start exporter '{}': {}",
            //             exporter.name, e
            //         )
            //     }
            // }

            // ssh::run_command(&exporter_sessions, &comm).await?;
            // match ssh::run_command(&exporter_sessions, &comm).await {
            //     Ok(()) => info!("Started exporter '{}'", exporter.name),
            //     Err(e) => {
            //         error!(
            //             "Failed to start exporter '{}': {}",
            //             exporter.name, e
            //         )
            //     }
            // }
        }
        Ok(live_exporters)
    }

    //TODO(joren): We need remote commands to write their outputs to a file somewhere.
    //This is so we can retrieve the information on the client if desired.
    //It's important to remember that we want to avoid streaming as much as possible
    //unless the client specifically requests it for debug purposes.
    async fn run_remote_executions(
        sessions: &Sessions,
        remote_executions: &[RemoteExecution],
        experiment_directory: &Path,
        variation_directory: &Path,
    ) -> Result<()> {
        for remote_execution in remote_executions.iter() {
            Self::run_remote_execution(
                sessions,
                remote_execution,
                experiment_directory,
                variation_directory,
            )
            .await?;
        }
        Ok(())
    }

    async fn run_remote_execution(
        sessions: &Sessions,
        remote_execution: &RemoteExecution,
        experiment_directory: &Path,
        variation_directory: &Path,
    ) -> Result<()> {
        let setup_sessions =
            Self::filter_host_sessions(sessions, &remote_execution.hosts);
        for remote_script in remote_execution.remote_scripts().iter() {
            // This assertion only checks locally.
            // Scripts should exist on the remote at this point.
            // assert!(remotescript.exists());

            let remote_script = experiment_directory.join(remote_script);
            ssh::run_script_at(
                &setup_sessions,
                remote_script,
                variation_directory,
            )
            .await?;
            // TODO(joren): Handle error case
            // let remote_script =
            //     experiment_directory.join(script.file_name().unwrap());
            // ssh::run_script(&setup_sessions, &remote_script).await?;
        }

        Ok(())
    }

    async fn symlink_dependencies(
        sessions: &Sessions,
        remote_execution: &RemoteExecution,
        dependencies: &[PathBuf],
        experiment_directory: &Path,
        variation_directory: &Path,
    ) -> Result<()> {
        let filter_sessions =
            Self::filter_host_sessions(sessions, &remote_execution.hosts);

        let deps_path = file_to_deps_path(
            experiment_directory,
            remote_execution.scripts.first().unwrap(),
        );

        let mut command = vec![
            "ln".to_string(),
            "-s".into(),
            "-t".into(),
            variation_directory.to_string_lossy().into(),
        ];
        for dependency in dependencies.iter() {
            let dep = deps_path.join(dependency.file_name().unwrap());
            command.push(dep.to_string_lossy().to_string());
        }

        ssh::run_command(&filter_sessions, &command)
            .await
            .map_err(Error::from)
    }

    async fn unlink_dependencies(
        sessions: &Sessions,
        remote_execution: &RemoteExecution,
        dependencies: &[PathBuf],
        variation_directory: &Path,
    ) -> Result<()> {
        let filter_sessions =
            Self::filter_host_sessions(sessions, &remote_execution.hosts);

        let mut command = vec!["rm".to_string()];
        for dependency in dependencies.iter() {
            let dep = variation_directory.join(dependency.file_name().unwrap());
            command.push(dep.to_string_lossy().to_string());
        }

        ssh::run_command(&filter_sessions, &command)
            .await
            .map_err(Error::from)
    }

    async fn collect_results(
        sessions: &Sessions,
        variation_directory: &Path,
    ) -> Result<()> {
        let (experiment_name, run, ts) =
            variation_dir_parts(variation_directory);

        info!(
            "Collecting results for experiment '{}' (repeat {})",
            experiment_name, run
        );

        let local_results_dir = std::path::absolute(PathBuf::from(format!(
            "{}/{}/{}/{}/{}",
            STORAGE_DIR, RESULTS_DIR, ts, run, experiment_name
        )))
        .map_err(|e| {
            Error::from((
                "Failed to canonicalize local results dir".to_string(),
                e,
            ))
        })?;

        ssh::download(sessions, &[variation_directory], &local_results_dir)
            .await?;

        Ok(())
    }
}

// #[cfg(test)]
// mod tests {
//     #[test]
//     fn enqueue_experiment() {}
// }
