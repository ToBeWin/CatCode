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

/// The `bash` module.
pub mod bash;
/// The `code_analysis` module.
pub mod code_analysis;
/// The `delete_file` module.
pub mod delete_file;
/// The `git_commit` module.
pub mod git_commit;
/// The `git_diff` module.
pub mod git_diff;
/// The `git_status` module.
pub mod git_status;
/// The `glob` module.
pub mod glob;
/// The `list_dir` module.
pub mod list_dir;
/// The `patch_file` module.
pub mod patch_file;
/// The `read_file` module.
pub mod read_file;
/// The `search` module.
pub mod search;
/// The `web_fetch` module.
pub mod web_fetch;
/// The `write_file` module.
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
