use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "sysmetrics-rs",
    about = "Linux system metrics Prometheus exporter"
)]
pub struct Cli {
    /// Address to listen on
    #[arg(long, default_value = "0.0.0.0:9101")]
    pub listen: Option<String>,

    /// Path to configuration file
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Log format: text or json
    #[arg(long, default_value = "text")]
    pub log_format: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub collectors: CollectorsConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_metrics_path")]
    #[allow(dead_code)]
    pub metrics_path: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            metrics_path: default_metrics_path(),
            log_format: default_log_format(),
        }
    }
}

fn default_listen() -> String {
    "0.0.0.0:9101".to_string()
}

fn default_metrics_path() -> String {
    "/metrics".to_string()
}

fn default_log_format() -> String {
    "text".to_string()
}

#[derive(Debug, Deserialize)]
pub struct CollectorsConfig {
    #[serde(default = "default_true")]
    pub cpu: bool,
    #[serde(default = "default_true")]
    pub memory: bool,
    #[serde(default = "default_true")]
    pub disk: bool,
    #[serde(default = "default_true")]
    pub network: bool,
    #[serde(default)]
    pub disk_config: DiskConfig,
    #[serde(default)]
    pub network_config: NetworkConfig,
}

impl Default for CollectorsConfig {
    fn default() -> Self {
        Self {
            cpu: true,
            memory: true,
            disk: true,
            network: true,
            disk_config: DiskConfig::default(),
            network_config: NetworkConfig::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct DiskConfig {
    #[serde(default = "default_disk_exclude")]
    pub exclude_pattern: String,
}

impl Default for DiskConfig {
    fn default() -> Self {
        Self {
            exclude_pattern: default_disk_exclude(),
        }
    }
}

fn default_disk_exclude() -> String {
    "^(loop|ram|dm-)".to_string()
}

#[derive(Debug, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_network_exclude")]
    pub exclude_pattern: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            exclude_pattern: default_network_exclude(),
        }
    }
}

fn default_network_exclude() -> String {
    "^(lo|veth)".to_string()
}

impl Config {
    /// Load configuration from file (if it exists) and apply CLI overrides.
    pub fn load(cli: &Cli) -> anyhow::Result<Self> {
        let mut config = if let Some(ref path) = cli.config {
            let content = std::fs::read_to_string(path)?;
            toml::from_str(&content)?
        } else {
            Config::default()
        };

        // CLI overrides
        if let Some(ref listen) = cli.listen {
            config.server.listen = listen.clone();
        }
        if let Some(ref log_format) = cli.log_format {
            config.server.log_format = log_format.clone();
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.listen, "0.0.0.0:9101");
        assert_eq!(config.server.metrics_path, "/metrics");
        assert!(config.collectors.cpu);
        assert!(config.collectors.memory);
        assert!(config.collectors.disk);
        assert!(config.collectors.network);
        assert_eq!(
            config.collectors.disk_config.exclude_pattern,
            "^(loop|ram|dm-)"
        );
        assert_eq!(
            config.collectors.network_config.exclude_pattern,
            "^(lo|veth)"
        );
    }

    #[test]
    fn test_parse_toml_config() {
        let toml_str = r#"
[server]
listen = "127.0.0.1:9102"
metrics_path = "/prom"

[collectors]
cpu = true
memory = false
disk = true
network = false
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.listen, "127.0.0.1:9102");
        assert_eq!(config.server.metrics_path, "/prom");
        assert!(config.collectors.cpu);
        assert!(!config.collectors.memory);
        assert!(config.collectors.disk);
        assert!(!config.collectors.network);
    }
}
