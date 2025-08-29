use std::path::PathBuf;

pub const VALID_CONFIG: &str = "valid_config.toml";
pub const VALID_EXPERIMENT_CONFIG: &str = "valid_experiment_config.toml";

pub fn test_path() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("tests")
}

// pub fn storage_path() -> PathBuf {
//     PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("storage")
// }
