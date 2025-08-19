use eframe::egui;
use std::sync::Arc;
use tokio::sync::RwLock;

mod config;
mod monitor;
mod ui;

use config::Config;
use monitor::MonitorManager;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Create the main runtime
    let rt = tokio::runtime::Runtime::new()?;
    
    // Run async setup
    let (config, monitor_manager) = rt.block_on(async {
        // Load or create configuration
        let config = Config::load_or_create("monitor_config.toml").await?;
        
        // Initialize monitor manager
        let monitor_manager = Arc::new(RwLock::new(MonitorManager::new()));
        
        // Start monitoring systems from config
        {
            let mut manager = monitor_manager.write().await;
            for system in &config.systems {
                manager.add_system(system.clone()).await?;
            }
        }
        
        Ok::<_, anyhow::Error>((config, monitor_manager))
    })?;

    // Start the GUI
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 700.0]),
        ..Default::default()
    };

    let app = ui::MonitorApp::new(config, monitor_manager, rt);
    
    let result = eframe::run_native(
        "System Uptime Monitor",
        options,
        Box::new(|_cc| Box::new(app)),
    );
    
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Failed to run GUI: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_creation() {
        let config = Config::default();
        assert_eq!(config.systems.len(), 0);
    }
}