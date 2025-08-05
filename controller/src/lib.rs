use std::time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH};

/// We put all our files (results, setup scripts, execution scripts, teardown scripts) beneath this
/// subdirectory.
/// Some important subdirectories are:
/// - storage/setup - Contains setup scripts
/// - storage/teardown - Contains teardown scripts
/// - storage/execute - Contains execute scripts
/// - storage/results - Contains collected results
/// - storage/exporters - Contains exporter binaries
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

fn time_since_epoch() -> Result<Duration, SystemTimeError> {
    SystemTime::now().duration_since(UNIX_EPOCH)
}

pub mod parse;
pub mod run;
pub mod ssh;
