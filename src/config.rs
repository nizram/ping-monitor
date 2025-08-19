use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub systems: Vec<SystemConfig>,
    pub check_interval_seconds: u64,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub name: String,
    pub host: String,
    pub port: Option<u16>,
    pub protocol: Protocol,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Protocol {
    Ping,
    Tcp,
    Udp,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            systems: Vec::new(),
            check_interval_seconds: 30,
            timeout_seconds: 5,
        }
    }
}

impl Config {
    pub async fn load_or_create(path: &str) -> Result<Self> {
        if Path::new(path).exists() {
            Self::load_from_file(path).await
        } else {
            log::info!("Config file not found, creating default configuration");
            let config = Self::create_default_config();
            config.save_to_file(path).await?;
            Ok(config)
        }
    }

    async fn load_from_file(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path).await?;
        let config: Config = toml::from_str(&content)?;
        log::info!("Loaded configuration with {} systems", config.systems.len());
        Ok(config)
    }

    pub async fn save_to_file(&self, path: &str) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content).await?;
        log::info!("Configuration saved to {}", path);
        Ok(())
    }

    fn create_default_config() -> Self {
        Config {
            systems: vec![
                SystemConfig {
                    name: "Google DNS".to_string(),
                    host: "8.8.8.8".to_string(),
                    port: None,
                    protocol: Protocol::Ping,
                    enabled: true,
                },
                SystemConfig {
                    name: "Cloudflare DNS".to_string(),
                    host: "1.1.1.1".to_string(),
                    port: None,
                    protocol: Protocol::Ping,
                    enabled: true,
                },
                SystemConfig {
                    name: "Local HTTP".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: Some(80),
                    protocol: Protocol::Tcp,
                    enabled: false,
                },
            ],
            check_interval_seconds: 30,
            timeout_seconds: 5,
        }
    }

    pub fn add_system(&mut self, system: SystemConfig) {
        self.systems.push(system);
    }

    pub fn remove_system(&mut self, index: usize) {
        if index < self.systems.len() {
            self.systems.remove(index);
        }
    }

    pub fn update_system(&mut self, index: usize, system: SystemConfig) {
        if index < self.systems.len() {
            self.systems[index] = system;
        }
    }
}

impl SystemConfig {
    pub fn new(name: String, host: String, port: Option<u16>, protocol: Protocol) -> Self {
        Self {
            name,
            host,
            port,
            protocol,
            enabled: true,
        }
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Ping => write!(f, "PING"),
            Protocol::Tcp => write!(f, "TCP"),
            Protocol::Udp => write!(f, "UDP"),
        }
    }
}