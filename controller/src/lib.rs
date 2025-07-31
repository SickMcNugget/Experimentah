/// We put all our files (results, setup scripts, execution scripts, teardown scripts) beneath this
/// subdirectory.
/// storage/setup
/// storage/teardown
/// storage/execute
/// storage/results
/// storage/exporters
pub const STORAGE_DIR: &str = "storage";

pub mod parse;
pub mod run;
pub mod ssh;
