//! `AutoFertilizer-config.json`, next to the DLL. Same file and keys as the original plugin, plus
//! `AddMissing`; written with the defaults if it doesn't exist.

use std::{fs, path::Path};

use serde::Deserialize;

const DEFAULT_CONFIG: &str = include_str!("../AutoFertilizer-config.json");
const MIN_CHECK_RATE: u64 = 500;

#[derive(Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Config {
    /// Milliseconds between checks.
    pub check_rate: u64,
    /// Half-price fertilizer, for the in-game event that discounts it.
    pub event: bool,
    /// Buy the fertilizers a hunter doesn't have. The original always did.
    pub add_missing: bool,
    // `Toggle` is in the original file but the original never read it, so it's ignored here too.
}

impl Default for Config {
    fn default() -> Self {
        Self { check_rate: 5_000, event: false, add_missing: true }
    }
}

impl Config {
    /// Returns the config and the problems to log.
    pub fn load(path: &Path) -> (Self, Vec<String>) {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(_) => {
                let _ = fs::write(path, DEFAULT_CONFIG);
                DEFAULT_CONFIG.to_owned()
            }
        };
        let mut problems = Vec::new();
        let mut config: Self = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
            Ok(config) => config,
            Err(e) => {
                problems.push(format!("config error, using the defaults: {e}"));
                Self::default()
            }
        };
        if config.check_rate < MIN_CHECK_RATE {
            problems.push(format!("config: CheckRate {} is below {MIN_CHECK_RATE}, using {MIN_CHECK_RATE}", config.check_rate));
            config.check_rate = MIN_CHECK_RATE;
        }
        (config, problems)
    }
}
