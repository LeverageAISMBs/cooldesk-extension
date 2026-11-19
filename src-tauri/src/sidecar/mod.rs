// Modified in the LeverageAISMBs/cooldesk-extension fork (2026-11): registers cli_auth and cli_invoke. See FORK-CHANGES.md.
// Sidecar Server Module
// Rust replacement for Node.js sidecar

pub mod data;
pub mod storage;
pub mod sync;
pub mod server;
pub mod handlers;
pub mod llm;
#[cfg(feature = "llm")]
pub mod llm_v2;
pub mod feedback;
pub mod llm_v3;
pub mod sampler;
pub mod sites;
pub mod cooldesk;
pub mod local_servers;
pub mod mcp;
pub mod browser_history;
pub mod cli_auth;
pub mod cli_invoke;

pub use server::start_server;
