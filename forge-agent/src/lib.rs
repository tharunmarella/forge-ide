pub mod bridge;
pub mod bridge_standalone;
pub mod loop_detection;
pub mod output_masking;
pub mod tools;
pub mod forge_search;
pub mod project_memory;

// Re-export key types
pub use bridge::ProxyBridge;
pub use bridge_standalone::StandaloneBridge;
pub use loop_detection::LoopDetector;

// Re-export forge_search client and SSE types
pub use forge_search::{
    client as forge_search_client, 
    ForgeSearchClient, 
    SseEvent, 
    SsePlanStep,
    ToolCallInfo,
};
