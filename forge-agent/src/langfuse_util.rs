//! Helper utilities for Langfuse integration

#[cfg(feature = "langfuse")]
use langfuse_ergonomic::{ClientBuilder, LangfuseClient};

/// Create a Langfuse client from environment variables.
///
/// Required environment variables:
/// - `LANGFUSE_PUBLIC_KEY`: Your Langfuse public key
/// - `LANGFUSE_SECRET_KEY`: Your Langfuse secret key
/// - `LANGFUSE_BASE_URL`: (Optional) Langfuse base URL, defaults to cloud.langfuse.com
///
/// Returns `None` if the feature is disabled or keys are not configured.
#[cfg(feature = "langfuse")]
pub fn create_langfuse_client() -> Option<LangfuseClient> {
    match ClientBuilder::from_env() {
        Ok(builder) => match builder.build() {
            Ok(client) => {
                tracing::info!("[Langfuse] Client initialized successfully");
                Some(client)
            }
            Err(e) => {
                tracing::warn!("[Langfuse] Failed to build client: {}", e);
                None
            }
        },
        Err(e) => {
            tracing::debug!("[Langfuse] Not configured (set LANGFUSE_PUBLIC_KEY and LANGFUSE_SECRET_KEY): {}", e);
            None
        }
    }
}

#[cfg(not(feature = "langfuse"))]
pub fn create_langfuse_client() -> Option<()> {
    None
}

/// Check if Langfuse is enabled and configured
pub fn is_langfuse_enabled() -> bool {
    #[cfg(feature = "langfuse")]
    {
        std::env::var("LANGFUSE_PUBLIC_KEY").is_ok() 
            && std::env::var("LANGFUSE_SECRET_KEY").is_ok()
    }
    
    #[cfg(not(feature = "langfuse"))]
    {
        false
    }
}
