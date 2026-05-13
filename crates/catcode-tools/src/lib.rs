//! # catcode-tools
//!
//! Built-in tools for the CatCode AI coding agent.
//!
//! Each tool implements the [`catcode_core::Tool`] trait. The [`ToolRegistry`]
//! manages tool discovery, dispatch, and LLM schema generation.
//!
//! ## Available tools
//!
//! - **File ops**: [`ReadFileTool`], [`WriteFileTool`], [`PatchFileTool`], [`DeleteFileTool`], [`ListDirTool`]
//! - **Search**: [`GlobTool`], [`SearchFilesTool`], [`WebFetchTool`]
//! - **Code analysis**: [`CodeAnalysisTool`]
//! - **Git**: [`GitCommitTool`], [`GitDiffTool`], [`GitStatusTool`]
//! - **Shell**: [`BashTool`]

pub mod bash;
pub mod code_analysis;
pub mod delete_file;
pub mod git_commit;
pub mod git_diff;
pub mod git_status;
pub mod glob;
pub mod list_dir;
pub mod patch_file;
pub mod read_file;
pub mod search;
pub mod web_fetch;
pub mod write_file;

mod registry;

pub use bash::{BashProgress, BashTool};
pub use code_analysis::CodeAnalysisTool;
pub use delete_file::DeleteFileTool;
pub use git_commit::GitCommitTool;
pub use git_diff::GitDiffTool;
pub use git_status::GitStatusTool;
pub use glob::GlobTool;
pub use list_dir::ListDirTool;
pub use patch_file::PatchFileTool;
pub use read_file::ReadFileTool;
pub use registry::ToolRegistry;
pub use search::SearchFilesTool;
pub use web_fetch::WebFetchTool;
pub use write_file::WriteFileTool;
