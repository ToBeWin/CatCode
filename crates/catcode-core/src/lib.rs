//! # catcode-core
//!
//! Core types and traits for the CatCode AI coding agent.
//!
//! This crate defines the foundational abstractions shared across all other
//! CatCode crates:
//!
//! - **Provider trait** — LLM provider interface (chat, streaming, token counting)
//! - **Tool trait** — Tool definition with schema, execution, and lifecycle hooks
//! - **Middleware trait** — Interceptor chain for tool execution
//! - **Types** — Chat requests/responses, token usage, roles, content blocks
//! - **Memory** — Facts, archive entries, and session memory types
//! - **Config** — Shared configuration structures
//! - **Error** — Error types for provider, tool, and middleware failures

pub mod config;
pub mod error;
pub mod memory;
pub mod middleware;
pub mod provider;
pub mod tool;
pub mod types;

pub use config::*;
pub use error::*;
pub use memory::*;
pub use middleware::*;
pub use provider::*;
pub use tool::*;
pub use types::*;
