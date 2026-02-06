//! Run Configuration module
//! 
//! Provides run configuration management with:
//! - Auto-detection of run scripts from project files
//! - User-defined configurations in .lapce/run.toml
//! - Title bar dropdown for quick access
//! - Full configuration editor tab

pub mod data;
pub mod dropdown;
pub mod view;

pub use data::{RunConfigData, RunConfigItem, ConfigSource, fetch_run_configs};
pub use dropdown::run_config_dropdown;
pub use view::run_config_editor_view;
