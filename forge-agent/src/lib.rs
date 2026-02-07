pub mod bridge;
pub mod bridge_standalone;
pub mod context_cache;
pub mod edit_fixer;
pub mod forge_agent;
pub mod loop_detection;
pub mod output_masking;
pub mod project_memory;
pub mod rig_tools;
pub mod tracing_hook;

// Legacy modules (kept for reference, will migrate incrementally)
pub mod api;
pub mod config;
pub mod context;
pub mod context7;
pub mod edit_agent;
pub mod repomap;
pub mod session;
pub mod setup;
pub mod tools;

// Re-export key types
pub use bridge::ProxyBridge;
pub use bridge_standalone::StandaloneBridge;
pub use forge_agent::{
    ForgeAgentConfig,
    create_agent_anthropic, create_agent_gemini, create_agent_openai,
    build_enriched_prompt, build_project_tree, detect_primary_language,
};
// rig_tools is already pub mod above

pub use tracing_hook::TracingHook;

// Re-export rig types that the IDE will need
pub use rig;
