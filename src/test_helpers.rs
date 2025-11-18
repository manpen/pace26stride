use std::env;
use std::path::PathBuf;

pub fn test_cases_glob(key: &str) -> impl Iterator<Item = PathBuf> {
    let pattern: String = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testcases")
        .join(key)
        .join("*.in")
        .to_str()
        .unwrap()
        .into();

    glob::glob(&pattern).unwrap().filter_map(|p| p.ok())
}
