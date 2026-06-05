use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct TlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cert: String,
    #[serde(default)]
    pub key: String,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cert: String::new(),
            key: String::new(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_port")]
    pub port: String,
    pub token: String,
    #[serde(default)]
    pub tls: TlsConfig,
}

fn default_port() -> String {
    "8888".to_string()
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let data = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path))?;
        let cfg: Config = serde_yaml::from_str(&data)
            .with_context(|| "failed to parse config YAML")?;
        Ok(cfg)
    }
}
