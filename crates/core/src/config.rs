//! `kare.toml` configuration for diagnostics thresholds, cost, and scoring
//! weights.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Config {
    #[serde(default)]
    pub thresholds: Thresholds,
    #[serde(default)]
    pub cost: Cost,
    #[serde(default)]
    pub weights: Weights,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Thresholds {
    pub slow_sec: f64,
    pub flaky_window_runs: usize,
    pub regression_factor: f64,
    pub regression_min_sec: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Cost {
    /// Cost per minute of test time. `0.0` disables cost reporting.
    pub per_min: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Weights {
    pub flaky: u32,
    pub flaky_max: u32,
    pub slow: u32,
    pub slow_max: u32,
    pub regression: u32,
    pub regression_max: u32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            slow_sec: 1.0,
            flaky_window_runs: 10,
            regression_factor: 2.0,
            regression_min_sec: 0.5,
        }
    }
}

impl Default for Cost {
    fn default() -> Self {
        Cost { per_min: 0.0 }
    }
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            flaky: 8,
            flaky_max: 32,
            slow: 2,
            slow_max: 20,
            regression: 4,
            regression_max: 16,
        }
    }
}

/// Loads config from `path`.
///
/// A missing file is not an error: it resolves to `Config::default()`. A
/// partial TOML file (e.g. only `thresholds.slow_sec`) fills the remaining
/// fields with their defaults. Malformed TOML is an error.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let contents = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source: e,
            })
        }
    };
    toml::from_str(&contents).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        source: e,
    })
}
