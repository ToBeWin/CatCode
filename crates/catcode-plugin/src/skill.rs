use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A Skill is a TOML-defined configuration that provides prompt templates,
/// tool preferences, context rules, and hooks for a specific domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub skill: SkillMetadata,
    #[serde(default)]
    pub rules: SkillRules,
    #[serde(default)]
    pub prompts: SkillPrompts,
    #[serde(default)]
    pub context: SkillContext,
    #[serde(default)]
    pub hooks: SkillHooks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillRules {
    /// Tools that should always be run after changes (e.g., ["cargo check"])
    #[serde(default)]
    pub always_run: Vec<String>,
    /// Tools the skill prefers to use
    #[serde(default)]
    pub prefer_tools: Vec<String>,
    /// Tools the skill advises against
    #[serde(default)]
    pub avoid_tools: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillPrompts {
    /// Additional system prompt suffix for this skill
    #[serde(default)]
    pub system_suffix: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillContext {
    /// Files that should always be included in context
    #[serde(default)]
    pub always_include_files: Vec<String>,
    /// Patterns to ignore when building context
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillHooks {
    /// Commands to run before commit
    #[serde(default)]
    pub before_commit: Option<String>,
    /// Commands to run after file write
    #[serde(default)]
    pub after_write: Option<String>,
}

/// Errors during skill loading or validation.
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("IO error reading skill {path}: {source}")]
    IoError {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("TOML parse error in {path}: {source}")]
    ParseError {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("Invalid skill '{name}': {reason}")]
    Invalid { name: String, reason: String },

    #[error("Skill not found: {0}")]
    NotFound(String),
}

/// Load a skill from a TOML file.
pub fn load_skill(path: &Path) -> Result<Skill, SkillError> {
    let content = std::fs::read_to_string(path).map_err(|e| SkillError::IoError {
        path: path.to_path_buf(),
        source: e,
    })?;

    let skill: Skill = toml::from_str(&content).map_err(|e| SkillError::ParseError {
        path: path.to_path_buf(),
        source: e,
    })?;

    validate_skill(&skill)?;
    Ok(skill)
}

/// Validate a skill's configuration.
fn validate_skill(skill: &Skill) -> Result<(), SkillError> {
    if skill.skill.name.is_empty() {
        return Err(SkillError::Invalid {
            name: "<unnamed>".to_string(),
            reason: "skill name cannot be empty".to_string(),
        });
    }
    if skill.skill.version.is_empty() {
        return Err(SkillError::Invalid {
            name: skill.skill.name.clone(),
            reason: "skill version cannot be empty".to_string(),
        });
    }
    Ok(())
}

/// Load all skills from a directory.
pub fn load_skills_from_dir(dir: &Path) -> Result<Vec<Skill>, SkillError> {
    let mut skills = Vec::new();

    if !dir.exists() {
        return Ok(skills);
    }

    let entries = std::fs::read_dir(dir).map_err(|e| SkillError::IoError {
        path: dir.to_path_buf(),
        source: e,
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            match load_skill(&path) {
                Ok(skill) => skills.push(skill),
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "Failed to load skill, skipping");
                }
            }
        }
    }

    Ok(skills)
}

/// Registry of loaded skills.
pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Register a skill.
    pub fn register(&mut self, skill: Skill) {
        self.skills.insert(skill.skill.name.clone(), skill);
    }

    /// Get a skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// List all registered skill names.
    pub fn list_names(&self) -> Vec<String> {
        self.skills.keys().cloned().collect()
    }

    /// Get all registered skills.
    pub fn all(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Load and register all skills from a directory.
    pub fn load_dir(&mut self, dir: &Path) -> Result<usize, SkillError> {
        let skills = load_skills_from_dir(dir)?;
        let count = skills.len();
        for skill in skills {
            self.register(skill);
        }
        Ok(count)
    }

    /// Build the combined system prompt suffix from all loaded skills.
    pub fn combined_system_suffix(&self) -> String {
        self.skills
            .values()
            .filter(|s| !s.prompts.system_suffix.is_empty())
            .map(|s| s.prompts.system_suffix.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Collect all "always include" files from loaded skills.
    pub fn all_always_include_files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .skills
            .values()
            .flat_map(|s| s.context.always_include_files.clone())
            .collect();
        files.sort();
        files.dedup();
        files
    }

    /// Collect all "always run" commands from loaded skills.
    pub fn all_always_run(&self) -> Vec<String> {
        self.skills
            .values()
            .flat_map(|s| s.rules.always_run.clone())
            .collect()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_skill_toml() -> &'static str {
        r#"
[skill]
name = "rust"
version = "1.0.0"
description = "Rust development skill"

[rules]
always_run = ["cargo check", "cargo clippy"]
prefer_tools = ["code_analysis", "patch_file"]
avoid_tools = ["bash"]

[prompts]
system_suffix = "You are working on a Rust project. Use anyhow::Result."

[context]
always_include_files = ["Cargo.toml", "src/lib.rs"]
ignore_patterns = ["target/", "*.lock"]

[hooks]
before_commit = "cargo test"
after_write = "cargo fmt"
"#
    }

    #[test]
    fn test_parse_skill_toml() {
        let skill: Skill = toml::from_str(sample_skill_toml()).unwrap();
        assert_eq!(skill.skill.name, "rust");
        assert_eq!(skill.skill.version, "1.0.0");
        assert_eq!(skill.rules.always_run.len(), 2);
        assert_eq!(skill.rules.prefer_tools.len(), 2);
        assert_eq!(skill.rules.avoid_tools.len(), 1);
        assert!(skill.prompts.system_suffix.contains("Rust"));
        assert_eq!(skill.context.always_include_files.len(), 2);
        assert_eq!(skill.context.ignore_patterns.len(), 2);
        assert!(skill.hooks.before_commit.is_some());
        assert!(skill.hooks.after_write.is_some());
    }

    #[test]
    fn test_validate_skill_valid() {
        let skill: Skill = toml::from_str(sample_skill_toml()).unwrap();
        assert!(validate_skill(&skill).is_ok());
    }

    #[test]
    fn test_validate_skill_empty_name() {
        let skill: Skill = toml::from_str(
            r#"
[skill]
name = ""
version = "1.0.0"
description = "test"
"#,
        )
        .unwrap();
        let result = validate_skill(&skill);
        assert!(result.is_err());
        match result.unwrap_err() {
            SkillError::Invalid { name, .. } => assert_eq!(name, "<unnamed>"),
            _ => panic!("Expected Invalid error"),
        }
    }

    #[test]
    fn test_validate_skill_empty_version() {
        let skill: Skill = toml::from_str(
            r#"
[skill]
name = "test"
version = ""
description = "test"
"#,
        )
        .unwrap();
        let result = validate_skill(&skill);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_skill_from_file() {
        let dir = std::env::temp_dir().join("catcode_test_skills");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_skill.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(sample_skill_toml().as_bytes()).unwrap();

        let skill = load_skill(&path).unwrap();
        assert_eq!(skill.skill.name, "rust");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_load_skill_invalid_toml() {
        let dir = std::env::temp_dir().join("catcode_test_skills");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("bad_skill.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"not valid toml [[[[").unwrap();

        let result = load_skill(&path);
        assert!(result.is_err());
        match result.unwrap_err() {
            SkillError::ParseError { .. } => {}
            _ => panic!("Expected ParseError"),
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_skill_registry() {
        let mut registry = SkillRegistry::new();
        let skill: Skill = toml::from_str(sample_skill_toml()).unwrap();
        registry.register(skill);

        assert!(registry.get("rust").is_some());
        assert!(registry.get("python").is_none());
        assert_eq!(registry.list_names().len(), 1);
    }

    #[test]
    fn test_skill_registry_combined_system_suffix() {
        let mut registry = SkillRegistry::new();
        let skill: Skill = toml::from_str(sample_skill_toml()).unwrap();
        registry.register(skill);

        let suffix = registry.combined_system_suffix();
        assert!(suffix.contains("Rust project"));
    }

    #[test]
    fn test_skill_registry_all_always_include_files() {
        let mut registry = SkillRegistry::new();
        let skill: Skill = toml::from_str(sample_skill_toml()).unwrap();
        registry.register(skill);

        let files = registry.all_always_include_files();
        assert!(files.contains(&"Cargo.toml".to_string()));
        assert!(files.contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn test_skill_registry_all_always_run() {
        let mut registry = SkillRegistry::new();
        let skill: Skill = toml::from_str(sample_skill_toml()).unwrap();
        registry.register(skill);

        let cmds = registry.all_always_run();
        assert!(cmds.contains(&"cargo check".to_string()));
        assert!(cmds.contains(&"cargo clippy".to_string()));
    }

    #[test]
    fn test_skill_minimal() {
        let toml_str = r#"
[skill]
name = "minimal"
version = "0.1.0"
description = "Minimal skill"
"#;
        let skill: Skill = toml::from_str(toml_str).unwrap();
        assert_eq!(skill.skill.name, "minimal");
        assert!(skill.rules.always_run.is_empty());
        assert!(skill.prompts.system_suffix.is_empty());
    }

    #[test]
    fn test_load_skills_from_dir() {
        let dir = std::env::temp_dir().join("catcode_test_skills_dir");
        let _ = std::fs::create_dir_all(&dir);

        // Create two skill files
        let path1 = dir.join("skill1.toml");
        let path2 = dir.join("skill2.toml");
        let mut f1 = std::fs::File::create(&path1).unwrap();
        f1.write_all(
            br#"
[skill]
name = "skill1"
version = "1.0.0"
description = "First skill"
"#,
        )
        .unwrap();
        let mut f2 = std::fs::File::create(&path2).unwrap();
        f2.write_all(
            br#"
[skill]
name = "skill2"
version = "1.0.0"
description = "Second skill"
"#,
        )
        .unwrap();

        // Also create a non-toml file that should be ignored
        let path3 = dir.join("readme.txt");
        let mut f3 = std::fs::File::create(&path3).unwrap();
        f3.write_all(b"not a skill").unwrap();

        let skills = load_skills_from_dir(&dir).unwrap();
        assert_eq!(skills.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_skills_from_nonexistent_dir() {
        let dir = Path::new("/tmp/catcode_nonexistent_skills_dir_12345");
        let skills = load_skills_from_dir(dir).unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn test_skill_registry_load_dir() {
        let dir = std::env::temp_dir().join("catcode_test_registry_dir");
        let _ = std::fs::create_dir_all(&dir);

        let path = dir.join("my_skill.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(
            br#"
[skill]
name = "my_skill"
version = "1.0.0"
description = "Test"

[prompts]
system_suffix = "Hello from skill"
"#,
        )
        .unwrap();

        let mut registry = SkillRegistry::new();
        let count = registry.load_dir(&dir).unwrap();
        assert_eq!(count, 1);
        assert!(registry.get("my_skill").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
