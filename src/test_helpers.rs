use std::env;
use std::path::PathBuf;

pub fn test_manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn test_testcases_dir() -> PathBuf {
    test_manifest_dir().join("testcases")
}

pub fn test_cases_glob(key: &str) -> impl Iterator<Item = PathBuf> {
    let pattern: String = test_testcases_dir()
        .join(key)
        .join("*.in")
        .to_str()
        .unwrap()
        .into();

    glob::glob(&pattern).unwrap().filter_map(|p| p.ok())
}
