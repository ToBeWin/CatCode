//! catcode-tools: Built-in tools for the CatCode AI agent.
//!
//! Each tool implements the `catcode_core::Tool` trait. The `ToolRegistry`
//! manages tool discovery, dispatch, and LLM schema generation.

pub mod bash;
pub mod glob;
pub mod list_dir;
pub mod read_file;
pub mod search;
pub mod write_file;

mod registry;

pub use bash::BashTool;
pub use glob::GlobTool;
pub use list_dir::ListDirTool;
pub use read_file::ReadFileTool;
pub use registry::ToolRegistry;
pub use search::SearchFilesTool;
pub use write_file::WriteFileTool;
