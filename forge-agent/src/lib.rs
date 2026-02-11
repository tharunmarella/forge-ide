pub mod bridge;
pub mod bridge_standalone;
pub mod output_masking;
pub mod tools;
pub mod tracing_hook;
pub mod langfuse_hook;
pub mod langfuse_util;
pub mod forge_search;
pub mod project_memory;

// Re-export key types
pub use bridge::ProxyBridge;
pub use bridge_standalone::StandaloneBridge;

pub use tracing_hook::TracingHook;
pub use langfuse_hook::LangfuseHook;
pub use langfuse_util::{create_langfuse_client, is_langfuse_enabled};

// Re-export forge_search client
pub use forge_search::{client as forge_search_client, ForgeSearchClient};
