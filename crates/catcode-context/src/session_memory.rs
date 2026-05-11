use anyhow::{Context, Result};
use catcode_core::memory::{MemoryEntry, MemoryType};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// File-based session memory system (Claude Code style).
///
/// Memory entries are stored as individual markdown files with YAML frontmatter
/// under a `.catcode/memory/` directory. An index file (`MEMORY.md`) is maintained
/// as a quick-reference listing of all memory entries.
///
/// # File Format
///
/// Each memory entry is stored as a markdown file:
///
/// ```markdown
/// ---
/// name: entry-name
/// description: one-line description
/// type: user|feedback|project|reference
/// ---
///
/// Content here.
/// ```
///
/// # Example
///
/// ```no_run
/// use catcode_context::SessionMemory;
/// use catcode_core::memory::{MemoryEntry, MemoryType};
/// use std::path::PathBuf;
///
/// # fn example() -> anyhow::Result<()> {
/// let memory = SessionMemory::new(PathBuf::from(".catcode/memory"));
/// memory.init()?;
///
/// let entry = MemoryEntry {
///     name: "deepseek-preference".to_string(),
///     description: "Use DeepSeek as default".to_string(),
///     memory_type: MemoryType::Feedback,
///     content: "Always prefer DeepSeek for code tasks.".to_string(),
/// };
/// memory.save_memory(&entry)?;
///
/// let index = memory.get_index_content()?;
/// println!("{index}");
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct SessionMemory {
    /// Root directory for memory files (e.g. `.catcode/memory/`).
    pub memory_dir: PathBuf,
    /// Path to the index file (`MEMORY.md`).
    pub index_path: PathBuf,
    /// Maximum number of lines in the index file.
    pub max_index_lines: usize,
}

impl SessionMemory {
    /// Create a new session memory instance.
    ///
    /// The index path is automatically set to `<memory_dir>/MEMORY.md`.
    pub fn new(memory_dir: PathBuf) -> Self {
        let index_path = memory_dir.join("MEMORY.md");
        Self {
            memory_dir,
            index_path,
            max_index_lines: 200,
        }
    }

    /// Initialize the memory directory and index file.
    ///
    /// Creates the directory structure and an empty `MEMORY.md` if they
    /// do not already exist. This is idempotent — calling it multiple
    /// times is safe.
    pub fn init(&self) -> Result<()> {
        if !self.memory_dir.exists() {
            fs::create_dir_all(&self.memory_dir).with_context(|| {
                format!(
                    "Failed to create memory directory: {}",
                    self.memory_dir.display()
                )
            })?;
            info!(dir = %self.memory_dir.display(), "Created memory directory");
        }
        if !self.index_path.exists() {
            fs::write(
                &self.index_path,
                "# Memory Index\n\nNo memories recorded yet.\n",
            )
            .with_context(|| {
                format!("Failed to create index file: {}", self.index_path.display())
            })?;
            info!(path = %self.index_path.display(), "Created memory index file");
        }
        Ok(())
    }

    /// Save a memory entry as a markdown file with frontmatter.
    ///
    /// The filename is derived from the entry name (sanitized for filesystem
    /// safety). If a file with the same name already exist, it is overwritten.
    pub fn save_memory(&self, entry: &MemoryEntry) -> Result<()> {
        let filename = sanitize_filename(&entry.name);
        let filepath = self.memory_dir.join(format!("{filename}.md"));

        let content = format!(
            "---\nname: {}\ndescription: {}\ntype: {}\n---\n\n{}\n",
            entry.name, entry.description, entry.memory_type, entry.content
        );

        fs::write(&filepath, &content)
            .with_context(|| format!("Failed to write memory file: {}", filepath.display()))?;

        debug!(name = %entry.name, path = %filepath.display(), "Saved memory entry");

        // Rebuild the index after saving
        self.rebuild_index()?;

        Ok(())
    }

    /// Load all memory entries from the memory directory.
    ///
    /// Reads all `.md` files (except `MEMORY.md` itself), parses their
    /// frontmatter, and returns the entries. Files that fail to parse
    /// are skipped with a warning.
    pub fn load_all(&self) -> Result<Vec<MemoryEntry>> {
        let mut entries = Vec::new();

        if !self.memory_dir.exists() {
            return Ok(entries);
        }

        let dir_entries = fs::read_dir(&self.memory_dir).with_context(|| {
            format!(
                "Failed to read memory directory: {}",
                self.memory_dir.display()
            )
        })?;

        for dir_entry in dir_entries {
            let dir_entry = dir_entry?;
            let path = dir_entry.path();

            // Only process .md files, skip MEMORY.md index
            if path.extension().is_some_and(|e| e == "md")
                && path.file_name().is_some_and(|n| n != "MEMORY.md")
            {
                match parse_memory_file(&path) {
                    Ok(entry) => entries.push(entry),
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "Failed to parse memory file");
                    }
                }
            }
        }

        debug!(count = entries.len(), "Loaded memory entries");
        Ok(entries)
    }

    /// Load memory entries filtered by type.
    ///
    /// Equivalent to `load_all()` followed by filtering, but clearer
    /// in intent.
    pub fn load_by_type(&self, memory_type: MemoryType) -> Result<Vec<MemoryEntry>> {
        let all = self.load_all()?;
        Ok(all
            .into_iter()
            .filter(|e| e.memory_type == memory_type)
            .collect())
    }

    /// Rebuild the `MEMORY.md` index from all memory files.
    ///
    /// Scans all memory files and regenerates the index. This is called
    /// automatically after `save_memory()`.
    pub fn rebuild_index(&self) -> Result<()> {
        let entries = self.load_all().unwrap_or_default();

        let mut lines: Vec<String> = vec!["# Memory Index".to_string(), String::new()];

        if entries.is_empty() {
            lines.push("No memories recorded yet.".to_string());
        } else {
            // Group by type
            for memory_type in &[
                MemoryType::User,
                MemoryType::Feedback,
                MemoryType::Project,
                MemoryType::Reference,
            ] {
                let typed: Vec<&MemoryEntry> = entries
                    .iter()
                    .filter(|e| e.memory_type == *memory_type)
                    .collect();

                if !typed.is_empty() {
                    lines.push(format!("## {}", capitalize(&memory_type.to_string())));
                    lines.push(String::new());
                    for entry in &typed {
                        lines.push(format!("- **{}**: {}", entry.name, entry.description));
                    }
                    lines.push(String::new());
                }
            }
        }

        // Enforce max line limit (reserve one line for the truncation marker)
        if lines.len() > self.max_index_lines {
            lines.truncate(self.max_index_lines.saturating_sub(1));
            lines.push("... (truncated)".to_string());
        }

        fs::write(&self.index_path, lines.join("\n")).with_context(|| {
            format!("Failed to write index file: {}", self.index_path.display())
        })?;

        debug!("Rebuilt memory index");
        Ok(())
    }

    /// Read the current `MEMORY.md` content.
    ///
    /// Returns the raw content of the index file, suitable for injection
    /// into the system prompt or context stack.
    pub fn get_index_content(&self) -> Result<String> {
        if !self.index_path.exists() {
            return Ok(String::new());
        }
        fs::read_to_string(&self.index_path)
            .with_context(|| format!("Failed to read index file: {}", self.index_path.display()))
    }
}

/// Parse a memory markdown file with YAML frontmatter.
fn parse_memory_file(path: &Path) -> Result<MemoryEntry> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    // Split on frontmatter delimiters
    let (name, description, memory_type, content) =
        if let Some(after_first) = raw.strip_prefix("---") {
            if let Some(end) = after_first.find("---") {
                let frontmatter = &after_first[..end];
                let body = after_first[end + 3..].trim().to_string();

                let mut name = String::new();
                let mut description = String::new();
                let mut memory_type = MemoryType::Project;

                for line in frontmatter.lines() {
                    let line = line.trim();
                    if let Some(val) = line.strip_prefix("name:") {
                        name = val.trim().to_string();
                    } else if let Some(val) = line.strip_prefix("description:") {
                        description = val.trim().to_string();
                    } else if let Some(val) = line.strip_prefix("type:") {
                        memory_type = parse_memory_type(val.trim());
                    }
                }

                (name, description, memory_type, body)
            } else {
                // Malformed frontmatter — treat as plain content
                (
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    String::new(),
                    MemoryType::Project,
                    raw,
                )
            }
        } else {
            // No frontmatter
            (
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                String::new(),
                MemoryType::Project,
                raw,
            )
        };

    Ok(MemoryEntry {
        name,
        description,
        memory_type,
        content,
    })
}

/// Parse a memory type string, falling back to `Project` for unknowns.
fn parse_memory_type(s: &str) -> MemoryType {
    match s.to_lowercase().as_str() {
        "user" => MemoryType::User,
        "feedback" => MemoryType::Feedback,
        "project" => MemoryType::Project,
        "reference" => MemoryType::Reference,
        _ => {
            warn!(value = s, "Unknown memory type, defaulting to Project");
            MemoryType::Project
        }
    }
}

/// Capitalize the first character of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

/// Sanitize a name for use as a filename.
///
/// Replaces non-alphanumeric characters (except `-` and `_`) with `-`,
/// collapses consecutive dashes, and trims leading/trailing dashes.
fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive dashes
    let mut result = String::new();
    let mut prev_dash = false;
    for c in sanitized.chars() {
        if c == '-' {
            if !prev_dash {
                result.push(c);
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }

    result.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_memory() -> (SessionMemory, TempDir) {
        let tmp = TempDir::new().unwrap();
        let memory = SessionMemory::new(tmp.path().join("memory"));
        (memory, tmp)
    }

    #[test]
    fn test_new_session_memory() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("memory");
        let memory = SessionMemory::new(dir.clone());
        assert_eq!(memory.memory_dir, dir);
        assert_eq!(memory.index_path, dir.join("MEMORY.md"));
        assert_eq!(memory.max_index_lines, 200);
    }

    #[test]
    fn test_init_creates_directory_and_index() {
        let (memory, _tmp) = make_memory();
        assert!(!memory.memory_dir.exists());

        memory.init().unwrap();

        assert!(memory.memory_dir.exists());
        assert!(memory.index_path.exists());

        let content = memory.get_index_content().unwrap();
        assert!(content.contains("Memory Index"));
    }

    #[test]
    fn test_init_idempotent() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();
        memory.init().unwrap(); // Should not fail
        assert!(memory.index_path.exists());
    }

    #[test]
    fn test_save_and_load_memory() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();

        let entry = MemoryEntry {
            name: "test-entry".to_string(),
            description: "A test entry".to_string(),
            memory_type: MemoryType::Feedback,
            content: "This is the content.".to_string(),
        };

        memory.save_memory(&entry).unwrap();

        let loaded = memory.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "test-entry");
        assert_eq!(loaded[0].description, "A test entry");
        assert_eq!(loaded[0].memory_type, MemoryType::Feedback);
        assert_eq!(loaded[0].content, "This is the content.");
    }

    #[test]
    fn test_save_multiple_and_load() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();

        memory
            .save_memory(&MemoryEntry {
                name: "entry-1".to_string(),
                description: "First".to_string(),
                memory_type: MemoryType::User,
                content: "Content 1".to_string(),
            })
            .unwrap();
        memory
            .save_memory(&MemoryEntry {
                name: "entry-2".to_string(),
                description: "Second".to_string(),
                memory_type: MemoryType::Project,
                content: "Content 2".to_string(),
            })
            .unwrap();

        let loaded = memory.load_all().unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn test_load_by_type() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();

        memory
            .save_memory(&MemoryEntry {
                name: "user-pref".to_string(),
                description: "User pref".to_string(),
                memory_type: MemoryType::User,
                content: "Be concise".to_string(),
            })
            .unwrap();
        memory
            .save_memory(&MemoryEntry {
                name: "project-rule".to_string(),
                description: "Project rule".to_string(),
                memory_type: MemoryType::Project,
                content: "Use Rust idioms".to_string(),
            })
            .unwrap();
        memory
            .save_memory(&MemoryEntry {
                name: "feedback-note".to_string(),
                description: "Feedback".to_string(),
                memory_type: MemoryType::Feedback,
                content: "Good job".to_string(),
            })
            .unwrap();

        let users = memory.load_by_type(MemoryType::User).unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name, "user-pref");

        let projects = memory.load_by_type(MemoryType::Project).unwrap();
        assert_eq!(projects.len(), 1);

        let refs = memory.load_by_type(MemoryType::Reference).unwrap();
        assert_eq!(refs.len(), 0);
    }

    #[test]
    fn test_load_empty_directory() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();

        let loaded = memory.load_all().unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_load_by_type_empty() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();

        let loaded = memory.load_by_type(MemoryType::User).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_overwrite_existing_memory() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();

        memory
            .save_memory(&MemoryEntry {
                name: "entry".to_string(),
                description: "Original".to_string(),
                memory_type: MemoryType::User,
                content: "Original content".to_string(),
            })
            .unwrap();

        memory
            .save_memory(&MemoryEntry {
                name: "entry".to_string(),
                description: "Updated".to_string(),
                memory_type: MemoryType::User,
                content: "Updated content".to_string(),
            })
            .unwrap();

        let loaded = memory.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].description, "Updated");
        assert_eq!(loaded[0].content, "Updated content");
    }

    #[test]
    fn test_index_content_after_save() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();

        memory
            .save_memory(&MemoryEntry {
                name: "my-pref".to_string(),
                description: "Be concise".to_string(),
                memory_type: MemoryType::Feedback,
                content: "Always be concise.".to_string(),
            })
            .unwrap();

        let index = memory.get_index_content().unwrap();
        assert!(index.contains("my-pref"));
        assert!(index.contains("Be concise"));
        assert!(index.contains("Feedback"));
    }

    #[test]
    fn test_rebuild_index_groups_by_type() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();

        memory
            .save_memory(&MemoryEntry {
                name: "a".to_string(),
                description: "User entry".to_string(),
                memory_type: MemoryType::User,
                content: "c".to_string(),
            })
            .unwrap();
        memory
            .save_memory(&MemoryEntry {
                name: "b".to_string(),
                description: "Project entry".to_string(),
                memory_type: MemoryType::Project,
                content: "c".to_string(),
            })
            .unwrap();
        memory
            .save_memory(&MemoryEntry {
                name: "c".to_string(),
                description: "Another user entry".to_string(),
                memory_type: MemoryType::User,
                content: "c".to_string(),
            })
            .unwrap();

        let index = memory.get_index_content().unwrap();
        assert!(index.contains("## User"));
        assert!(index.contains("## Project"));
        // Should not have empty sections
        assert!(!index.contains("## Reference"));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("hello-world"), "hello-world");
        assert_eq!(sanitize_filename("hello_world"), "hello_world");
        assert_eq!(sanitize_filename("hello world!"), "hello-world");
        assert_eq!(sanitize_filename("foo/bar"), "foo-bar");
        assert_eq!(sanitize_filename("a---b"), "a-b");
        assert_eq!(sanitize_filename("--trim--"), "trim");
    }

    #[test]
    fn test_parse_memory_file_with_frontmatter() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.md");
        let content = "---\nname: my-entry\ndescription: test desc\ntype: feedback\n---\n\nBody content here.\n";
        fs::write(&path, content).unwrap();

        let entry = parse_memory_file(&path).unwrap();
        assert_eq!(entry.name, "my-entry");
        assert_eq!(entry.description, "test desc");
        assert_eq!(entry.memory_type, MemoryType::Feedback);
        assert!(entry.content.contains("Body content here."));
    }

    #[test]
    fn test_parse_memory_file_no_frontmatter() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("plain.md");
        fs::write(&path, "Just plain content.").unwrap();

        let entry = parse_memory_file(&path).unwrap();
        assert_eq!(entry.name, "plain");
        assert_eq!(entry.content, "Just plain content.");
        assert_eq!(entry.memory_type, MemoryType::Project); // default
    }

    #[test]
    fn test_parse_memory_type_unknown() {
        assert_eq!(parse_memory_type("user"), MemoryType::User);
        assert_eq!(parse_memory_type("feedback"), MemoryType::Feedback);
        assert_eq!(parse_memory_type("project"), MemoryType::Project);
        assert_eq!(parse_memory_type("reference"), MemoryType::Reference);
        assert_eq!(parse_memory_type("unknown"), MemoryType::Project); // fallback
    }

    #[test]
    fn test_max_index_lines_respected() {
        let tmp = TempDir::new().unwrap();
        let mut memory = SessionMemory::new(tmp.path().join("memory"));
        memory.max_index_lines = 5;
        memory.init().unwrap();

        for i in 0..20 {
            memory
                .save_memory(&MemoryEntry {
                    name: format!("entry-{i}"),
                    description: format!("Description {i}"),
                    memory_type: MemoryType::User,
                    content: format!("Content {i}"),
                })
                .unwrap();
        }

        let index = memory.get_index_content().unwrap();
        let line_count = index.lines().count();
        assert!(line_count <= 5);
    }

    #[test]
    fn test_get_index_content_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let memory = SessionMemory::new(tmp.path().join("nonexistent"));
        // Should not fail, returns empty string
        let content = memory.get_index_content().unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn test_save_memory_sanitizes_name() {
        let (memory, _tmp) = make_memory();
        memory.init().unwrap();

        memory
            .save_memory(&MemoryEntry {
                name: "my weird/name!".to_string(),
                description: "Test".to_string(),
                memory_type: MemoryType::User,
                content: "Body".to_string(),
            })
            .unwrap();

        // File should exist with sanitized name
        let expected_path = memory.memory_dir.join("my-weird-name.md");
        assert!(expected_path.exists());

        // Should be loadable
        let loaded = memory.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "my weird/name!");
    }
}
