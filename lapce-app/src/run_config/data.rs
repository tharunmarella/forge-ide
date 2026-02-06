//! Run Configuration data management

use std::rc::Rc;

use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use lapce_rpc::{
    dap_types::RunDebugConfig,
    proxy::{DetectedRunConfig, ProxyResponse},
};

use crate::window_tab::CommonData;

/// Data for managing run configurations
#[derive(Clone)]
pub struct RunConfigData {
    pub scope: Scope,
    /// Currently selected configuration name
    pub selected: RwSignal<Option<String>>,
    /// Auto-detected configurations from project files
    pub detected_configs: RwSignal<Vec<DetectedRunConfig>>,
    /// User-defined configurations from .lapce/run.toml
    pub user_configs: RwSignal<Vec<RunDebugConfig>>,
    /// Whether configs are currently loading
    pub loading: RwSignal<bool>,
    /// Error message if any
    pub error: RwSignal<Option<String>>,
    /// Whether the run config dropdown is visible
    pub dropdown_visible: RwSignal<bool>,
}

impl RunConfigData {
    pub fn new(scope: Scope) -> Self {
        Self {
            scope,
            selected: scope.create_rw_signal(None),
            detected_configs: scope.create_rw_signal(Vec::new()),
            user_configs: scope.create_rw_signal(Vec::new()),
            loading: scope.create_rw_signal(false),
            error: scope.create_rw_signal(None),
            dropdown_visible: scope.create_rw_signal(false),
        }
    }
    
    /// Get all configurations (detected + user)
    pub fn all_configs(&self) -> Vec<RunConfigItem> {
        let mut items = Vec::new();
        
        // Add detected configs
        for config in self.detected_configs.get() {
            items.push(RunConfigItem {
                name: config.name.clone(),
                config_type: config.config_type.clone(),
                source: ConfigSource::Detected,
                command: config.command.clone(),
                args: config.args.clone(),
                cwd: config.cwd.clone(),
            });
        }
        
        // Add user configs
        for config in self.user_configs.get() {
            items.push(RunConfigItem {
                name: config.name.clone(),
                config_type: config.ty.clone().unwrap_or_else(|| "custom".to_string()),
                source: ConfigSource::User,
                command: config.program.clone(),
                args: config.args.clone().unwrap_or_default(),
                cwd: config.cwd.clone(),
            });
        }
        
        items
    }
    
    /// Get the currently selected configuration
    pub fn get_selected_config(&self) -> Option<RunConfigItem> {
        let selected_name = self.selected.get()?;
        self.all_configs().into_iter().find(|c| c.name == selected_name)
    }
    
    /// Select a configuration by name
    pub fn select(&self, name: Option<String>) {
        self.selected.set(name);
    }
    
    /// Toggle dropdown visibility
    pub fn toggle_dropdown(&self) {
        self.dropdown_visible.update(|v| *v = !*v);
    }
    
    /// Close dropdown
    pub fn close_dropdown(&self) {
        self.dropdown_visible.set(false);
    }
}

/// A unified run configuration item (can be detected or user-defined)
#[derive(Clone, Debug)]
pub struct RunConfigItem {
    pub name: String,
    pub config_type: String,
    pub source: ConfigSource,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

impl RunConfigItem {
    /// Convert to RunDebugConfig for execution
    pub fn to_run_debug_config(&self) -> RunDebugConfig {
        RunDebugConfig {
            ty: Some(self.config_type.clone()),
            name: self.name.clone(),
            program: self.command.clone(),
            args: Some(self.args.clone()),
            cwd: self.cwd.clone(),
            env: None,
            prelaunch: None,
            debug_command: None,
            dap_id: Default::default(),
            tracing_output: false,
            config_source: Default::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigSource {
    Detected,
    User,
}

/// Fetch run configurations from the backend
pub fn fetch_run_configs(
    scope: Scope,
    common: Rc<CommonData>,
    data: RunConfigData,
) {
    use floem::ext_event::create_ext_action;
    
    tracing::info!("fetch_run_configs: Starting");
    
    data.loading.set(true);
    data.error.set(None);
    
    tracing::info!("fetch_run_configs: Creating ext_action");
    let send = create_ext_action(scope, move |result: Result<ProxyResponse, _>| {
        tracing::info!("fetch_run_configs: Callback received");
        data.loading.set(false);
        match result {
            Ok(ProxyResponse::RunConfigsResponse { detected, user }) => {
                tracing::info!("fetch_run_configs: Got {} detected, {} user configs", detected.len(), user.len());
                data.detected_configs.set(detected);
                data.user_configs.set(user);
                
                // Auto-select first config if none selected
                if data.selected.get().is_none() {
                    let all = data.all_configs();
                    if let Some(first) = all.first() {
                        data.selected.set(Some(first.name.clone()));
                    }
                }
                tracing::info!("fetch_run_configs: Done processing configs");
            }
            Err(e) => {
                tracing::error!("fetch_run_configs: Error: {:?}", e);
                data.error.set(Some(format!("Failed to load configs: {:?}", e)));
            }
            _ => {
                tracing::warn!("fetch_run_configs: Unexpected response type");
            }
        }
    });
    
    tracing::info!("fetch_run_configs: Calling proxy.get_run_configs");
    common.proxy.get_run_configs(send);
    tracing::info!("fetch_run_configs: RPC call initiated");
}
