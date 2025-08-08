use std::{
    path::Path,
    time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH},
};

pub mod db;
pub mod parse;
pub mod run;
pub mod ssh;

/// We put all our files (results, setup scripts, execution scripts, teardown scripts) beneath this
/// subdirectory.
/// Some important subdirectories are:
/// - storage/setup - Contains setup scripts
/// - storage/teardown - Contains teardown scripts
/// - storage/execute - Contains execute scripts
/// - storage/results - Contains collected results
/// - storage/exporters - Contains exporter binaries
///
/// Some important files are:
/// - storage/<DATABASE>
pub const STORAGE_DIR: &str = "storage";

/// We want all our remote operations to occur in a well-known directory, so that we can avoid.
///
/// Overwriting anything that's pre-existing on the target system.
///
/// As such, we use `/srv/experimentah` as the base directory for our remote operations.
/// Any subdirectories created within `/srv/experimentah` are deleted once results are retrieved.
pub const REMOTE_DIR: &str = "/srv/experimentah";

/// The results directory should be relative to both:
/// - the variation directory: /srv/experimentah/<timestamp>/<repeat>/results
/// - the controller storage directory: storage/results
pub const RESULTS_DIR: &str = "results";

/// We need a way to determine if any exporters are currently running on our remote machines
/// (meaning that they were launched by Experimentah in either the current session or a previous
/// one).
///
/// To do so, we use advisory locks inside this directory to state that the process is, in fact,
/// currently running.
pub const EXPORTER_DIR: &str = "/srv/experimentah/live_exporters";

/// By default, we currently assume that bash is the default interpreter and that it will be
/// available on PATH in some manner by the remote SSH user.
pub const INTERPRETER: &str = "bash";

/// The philosophy of Experimentah is to do everything in *files*, to remove the complexity of
/// using databases everywhere.
///
/// However, it is important that we use a database for error recovery. It's really easy to just
/// query a database for what you were doing when you last ran the program, and we get some atomic
/// guarantees that could be nice if we ever want to support running multiple experiments
/// simultaneously.
///
/// This will *NEVER* be used to store the outputs of experiments, as we want the filesystem to do
/// the heavy lifting here.
///
/// The database file should be relative to the STORAGE_DIR
pub const DATABASE_NAME: &str = "experimentah";

fn time_since_epoch() -> Result<Duration, SystemTimeError> {
    SystemTime::now().duration_since(UNIX_EPOCH)
}

/// This function converts a path of format:
/// <PREFIX>/<TIMESTAMP>/<RUN>/<NAME> into
/// (<NAME>, <RUN>, <TIMESTAMP>)
fn variation_dir_parts(variation_directory: &Path) -> (String, u16, u128) {
    let experiment_name = variation_directory
        .file_name()
        .expect("Variation directory did not contain an experiment name")
        .to_string_lossy()
        .to_string();
    let run: u16 = variation_directory
        .parent()
        .unwrap()
        .file_name()
        .expect("Variation directory did not contain a run number")
        .to_string_lossy()
        .parse()
        .expect("Run number was not a valid u16");
    let ts = variation_directory
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .file_name()
        .expect("Variation directory did not contain a timestamp")
        .to_string_lossy()
        .parse()
        .expect("Timestamp was not a valid u128 (milliseconds)");

    (experiment_name, run, ts)
}
