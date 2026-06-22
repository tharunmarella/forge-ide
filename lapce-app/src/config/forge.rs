use serde::{Deserialize, Serialize};
use structdesc::FieldNames;

#[derive(FieldNames, Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ForgeConfig {
    #[field_names(desc = "Base URL for the forge-search API")]
    pub search_url: String,
    #[field_names(desc = "Default AI provider when not using forge-search auth")]
    pub default_provider: String,
    #[field_names(desc = "Default AI model when not using forge-search auth")]
    pub default_model: String,
}

impl ForgeConfig {
    /// Resolved forge-search API base URL (env overrides config; placeholders ignored).
    pub fn resolved_search_url(&self) -> String {
        let cfg = self.search_url.trim();
        forge_agent::forge_search::resolve_base_url(if cfg.is_empty() {
            None
        } else {
            Some(cfg)
        })
    }
}
