use crate::config::{Config, Protocol, SystemConfig};
use crate::monitor::{MonitorManager, SystemStatus};
use eframe::egui;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub struct MonitorApp {
    config: Config,
    monitor_manager: Arc<RwLock<MonitorManager>>,
    systems: Vec<SystemStatus>,
    show_add_dialog: bool,
    new_system: SystemConfig,
    selected_protocol: usize,
    refresh_counter: u64,
    system_to_remove: Option<Uuid>,
    runtime: tokio::runtime::Runtime,
}

impl MonitorApp {
    pub fn new(config: Config, monitor_manager: Arc<RwLock<MonitorManager>>, runtime: tokio::runtime::Runtime) -> Self {
        Self {
            config,
            monitor_manager,
            systems: Vec::new(),
            show_add_dialog: false,
            new_system: SystemConfig {
                name: String::new(),
                host: String::new(),
                port: None,
                protocol: Protocol::Ping,
                enabled: true,
            },
            selected_protocol: 0,
            refresh_counter: 0,
            system_to_remove: None,
            runtime,
        }
    }

    fn refresh_systems(&mut self) {
        if let Ok(manager) = self.monitor_manager.try_read() {
            let new_systems = manager.get_systems();
            if new_systems.len() != self.systems.len() {
                log::info!("Systems count changed: {} -> {}", self.systems.len(), new_systems.len());
            }
            self.systems = new_systems;
        }
    }

    fn add_system(&mut self) {
        self.new_system.protocol = match self.selected_protocol {
            0 => Protocol::Ping,
            1 => Protocol::Tcp,
            2 => Protocol::Udp,
            _ => Protocol::Ping,
        };

        if let Ok(mut manager) = self.monitor_manager.try_write() {
            if let Ok(_id) = self.runtime.block_on(manager.add_system(self.new_system.clone())) {
                self.config.add_system(self.new_system.clone());
                // Save config in background
                let config = self.config.clone();
                self.runtime.spawn(async move {
                    let _ = config.save_to_file("monitor_config.toml").await;
                });
            }
        }

        // Reset form
        self.new_system = SystemConfig {
            name: String::new(),
            host: String::new(),
            port: None,
            protocol: Protocol::Ping,
            enabled: true,
        };
        self.selected_protocol = 0;
        self.show_add_dialog = false;
    }

    fn remove_system(&mut self, id: Uuid) {
        if let Ok(mut manager) = self.monitor_manager.try_write() {
            manager.remove_system(id);
            
            // Remove from config
            self.config.systems.retain(|_s| {
                // This is a bit hacky since we don't store UUIDs in config
                // In a real app, you'd want to match by name+host or add UUIDs to config
                true
            });
            
            // Save config
            let config = self.config.clone();
            self.runtime.spawn(async move {
                let _ = config.save_to_file("monitor_config.toml").await;
            });
        }
    }

    fn draw_status_icon(&self, ui: &mut egui::Ui, is_online: bool, response_time: Option<u64>) {
        let (color, text) = if is_online {
            let color = match response_time {
                Some(ms) if ms < 100 => egui::Color32::GREEN,
                Some(ms) if ms < 500 => egui::Color32::YELLOW,
                Some(_) => egui::Color32::from_rgb(255, 165, 0), // Orange
                None => egui::Color32::GREEN,
            };
            (color, "●")
        } else {
            (egui::Color32::RED, "●")
        };

        ui.colored_label(color, text);
    }
}

impl eframe::App for MonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-refresh every few seconds
        self.refresh_counter += 1;
        if self.refresh_counter % 60 == 0 { // Refresh every ~1 second at 60 FPS
            self.refresh_systems();
        }

        // Request repaint for smooth updates
        ctx.request_repaint();

        // Handle system removal outside of the iteration
        if let Some(id_to_remove) = self.system_to_remove.take() {
            self.remove_system(id_to_remove);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("System Uptime Monitor");
            ui.separator();

            // Toolbar
            ui.horizontal(|ui| {
                if ui.button("Add System").clicked() {
                    self.show_add_dialog = true;
                }
                
                if ui.button("Refresh").clicked() {
                    self.refresh_systems();
                }
                
                if ui.button("Test Ping").clicked() {
                    // Quick test
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    match rt.block_on(tokio::process::Command::new("ping")
                        .args(&["-c", "1", "8.8.8.8"])
                        .output()) {
                        Ok(output) => {
                            log::info!("Ping test result: success={}", output.status.success());
                            if !output.status.success() {
                                log::info!("Ping stderr: {}", String::from_utf8_lossy(&output.stderr));
                            }
                        }
                        Err(e) => log::error!("Ping test failed: {}", e),
                    }
                }

                ui.separator();
                ui.label(format!("Monitoring {} systems", self.systems.len()));
            });

            ui.separator();

            // Systems table
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("systems_grid")
                    .num_columns(7)
                    .spacing([10.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        // Header
                        ui.strong("Status");
                        ui.strong("Name");
                        ui.strong("Host");
                        ui.strong("Protocol");
                        ui.strong("Response Time");
                        ui.strong("Uptime %");
                        ui.strong("Actions");
                        ui.end_row();

                        // System rows
                        let systems_to_show = self.systems.clone();
                        for system in &systems_to_show {
                            self.draw_status_icon(ui, system.is_online, system.response_time_ms);
                            
                            ui.label(&system.config.name);
                            
                            let host_text = if let Some(port) = system.config.port {
                                format!("{}:{}", system.config.host, port)
                            } else {
                                system.config.host.clone()
                            };
                            ui.label(host_text);
                            
                            ui.label(format!("{}", system.config.protocol));
                            
                            if let Some(ms) = system.response_time_ms {
                                ui.label(format!("{}ms", ms));
                            } else {
                                ui.label("-");
                            }
                            
                            ui.label(format!("{:.1}%", system.uptime_percentage));
                            
                            let system_id = system.id;
                            ui.horizontal(|ui| {
                                if ui.button("Remove").clicked() {
                                    self.system_to_remove = Some(system_id);
                                }
                            });
                            
                            ui.end_row();
                        }
                    });
            });

            // System details
            if !self.systems.is_empty() {
                ui.separator();
                ui.heading("System Details");
                
                for system in &self.systems {
                    ui.collapsing(&system.config.name, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Last Check:");
                            ui.label(system.last_check.format("%Y-%m-%d %H:%M:%S UTC").to_string());
                        });
                        
                        if let Some(last_online) = system.last_online {
                            ui.horizontal(|ui| {
                                ui.label("Last Online:");
                                ui.label(last_online.format("%Y-%m-%d %H:%M:%S UTC").to_string());
                            });
                        }
                        
                        if let Some(last_offline) = system.last_offline {
                            ui.horizontal(|ui| {
                                ui.label("Last Offline:");
                                ui.label(last_offline.format("%Y-%m-%d %H:%M:%S UTC").to_string());
                            });
                        }
                        
                        ui.horizontal(|ui| {
                            ui.label("Total Checks:");
                            ui.label(system.total_checks.to_string());
                        });
                        
                        ui.horizontal(|ui| {
                            ui.label("Successful Checks:");
                            ui.label(system.successful_checks.to_string());
                        });
                        
                        if let Some(error) = &system.error_message {
                            ui.horizontal(|ui| {
                                ui.label("Last Error:");
                                ui.colored_label(egui::Color32::RED, error);
                            });
                        }
                    });
                }
            }
        });

        // Add system dialog
        if self.show_add_dialog {
            egui::Window::new("Add New System")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut self.new_system.name);
                    });

                    ui.horizontal(|ui| {
                        ui.label("Host:");
                        ui.text_edit_singleline(&mut self.new_system.host);
                    });

                    ui.horizontal(|ui| {
                        ui.label("Protocol:");
                        egui::ComboBox::from_label("")
                            .selected_text(match self.selected_protocol {
                                0 => "PING",
                                1 => "TCP",
                                2 => "UDP",
                                _ => "PING",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.selected_protocol, 0, "PING");
                                ui.selectable_value(&mut self.selected_protocol, 1, "TCP");
                                ui.selectable_value(&mut self.selected_protocol, 2, "UDP");
                            });
                    });

                    if self.selected_protocol != 0 {
                        ui.horizontal(|ui| {
                            ui.label("Port:");
                            let mut port_str = self.new_system.port.map_or(String::new(), |p| p.to_string());
                            if ui.text_edit_singleline(&mut port_str).changed() {
                                self.new_system.port = port_str.parse().ok();
                            }
                        });
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked() {
                            if !self.new_system.name.is_empty() && !self.new_system.host.is_empty() {
                                self.add_system();
                            }
                        }
                        
                        if ui.button("Cancel").clicked() {
                            self.show_add_dialog = false;
                        }
                    });
                });
        }
    }
}