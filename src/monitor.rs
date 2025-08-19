use crate::config::{Protocol, SystemConfig};
use anyhow::Result;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, ToSocketAddrs, IpAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::{sleep, timeout};
use surge_ping::{Client, Config, PingIdentifier, PingSequence};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub id: Uuid,
    pub config: SystemConfig,
    pub is_online: bool,
    pub last_check: DateTime<Utc>,
    pub last_online: Option<DateTime<Utc>>,
    pub last_offline: Option<DateTime<Utc>>,
    pub response_time_ms: Option<u64>,
    pub uptime_percentage: f64,
    pub total_checks: u64,
    pub successful_checks: u64,
    pub error_message: Option<String>,
}

impl SystemStatus {
    pub fn new(config: SystemConfig) -> Self {
        Self {
            id: Uuid::new_v4(),
            config,
            is_online: false,
            last_check: Utc::now(),
            last_online: None,
            last_offline: None,
            response_time_ms: None,
            uptime_percentage: 0.0,
            total_checks: 0,
            successful_checks: 0,
            error_message: None,
        }
    }

    pub fn update_status(&mut self, is_online: bool, response_time: Option<u64>, error: Option<String>) {
        let now = Utc::now();
        
        self.last_check = now;
        self.total_checks += 1;
        self.error_message = error;
        self.response_time_ms = response_time;

        if is_online {
            self.successful_checks += 1;
            self.last_online = Some(now);
            if !self.is_online {
                log::info!("{} is now ONLINE", self.config.name);
            }
        } else if self.is_online {
            self.last_offline = Some(now);
            log::warn!("{} is now OFFLINE", self.config.name);
        }

        self.is_online = is_online;
        self.uptime_percentage = if self.total_checks > 0 {
            (self.successful_checks as f64 / self.total_checks as f64) * 100.0
        } else {
            0.0
        };
    }
}

pub struct MonitorManager {
    systems: Arc<DashMap<Uuid, SystemStatus>>,
    monitoring_tasks: DashMap<Uuid, tokio::task::JoinHandle<()>>,
}

impl MonitorManager {
    pub fn new() -> Self {
        Self {
            systems: Arc::new(DashMap::new()),
            monitoring_tasks: DashMap::new(),
        }
    }

    pub async fn add_system(&mut self, config: SystemConfig) -> Result<Uuid> {
        let status = SystemStatus::new(config);
        let id = status.id;
        
        self.systems.insert(id, status);
        self.start_monitoring_task(id).await?;
        
        Ok(id)
    }

    pub fn remove_system(&mut self, id: Uuid) {
        self.systems.remove(&id);
        if let Some((_, task)) = self.monitoring_tasks.remove(&id) {
            task.abort();
        }
    }

    pub fn get_systems(&self) -> Vec<SystemStatus> {
        self.systems.iter().map(|entry| entry.value().clone()).collect()
    }

    pub fn get_system(&self, id: Uuid) -> Option<SystemStatus> {
        self.systems.get(&id).map(|entry| entry.value().clone())
    }

    async fn start_monitoring_task(&self, id: Uuid) -> Result<()> {
        let systems = Arc::clone(&self.systems);
        
        let task = tokio::spawn(async move {
            loop {
                if let Some(mut system_ref) = systems.get_mut(&id) {
                    let config = system_ref.config.clone();
                    
                    if config.enabled {
                        let (is_online, response_time, error) = 
                            Self::check_system_status(&config).await;
                        
                        system_ref.update_status(is_online, response_time, error);
                    }
                } else {
                    // System was removed, exit task
                    break;
                }
                
                sleep(Duration::from_secs(30)).await; // Default check interval
            }
        });

        self.monitoring_tasks.insert(id, task);
        Ok(())
    }

    async fn check_system_status(config: &SystemConfig) -> (bool, Option<u64>, Option<String>) {
        let start_time = std::time::Instant::now();
        
        let result = match config.protocol {
            Protocol::Ping => Self::ping_check(&config.host).await,
            Protocol::Tcp => Self::tcp_check(&config.host, config.port.unwrap_or(80)).await,
            Protocol::Udp => Self::udp_check(&config.host, config.port.unwrap_or(53)).await,
        };

        let response_time = start_time.elapsed().as_millis() as u64;

        match result {
            Ok(()) => (true, Some(response_time), None),
            Err(e) => (false, None, Some(e.to_string())),
        }
    }

    async fn ping_check(host: &str) -> Result<()> {
        // Try using system ping command as fallback
        let output = tokio::process::Command::new("ping")
            .args(&["-c", "1", "-W", "5", host])
            .output()
            .await?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("Ping failed: {}", stderr))
        }
    }

    async fn tcp_check(host: &str, port: u16) -> Result<()> {
        let addr = format!("{}:{}", host, port);
        let socket_addr: SocketAddr = addr.to_socket_addrs()?.next()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve address"))?;
        
        timeout(
            Duration::from_secs(5),
            TcpStream::connect(socket_addr)
        ).await??;
        
        Ok(())
    }

    async fn udp_check(host: &str, port: u16) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let addr = format!("{}:{}", host, port);
        
        // Send a simple UDP packet
        timeout(
            Duration::from_secs(5),
            socket.send_to(b"ping", &addr)
        ).await??;
        
        Ok(())
    }
}

impl Default for MonitorManager {
    fn default() -> Self {
        Self::new()
    }
}