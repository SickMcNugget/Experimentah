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

pub mod parse;
pub mod run;
pub mod ssh;
