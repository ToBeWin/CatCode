use catcode_core::OperationLevel;

/// Classifies tool operations by their risk level.
///
/// This extends the basic OperationLevel from catcode-core with
/// argument-aware classification logic.
/// Classifies tool operations by risk level (Safe, Sensitive, Dangerous).
pub struct OperationClassifier;

impl OperationClassifier {
    /// Classify a tool call by name and arguments.
    ///
    /// The base level comes from the tool's own `operation_level()`,
    /// but the classifier can upgrade it based on arguments.
    pub fn classify(tool: &str, args: &serde_json::Value) -> OperationLevel {
        match tool {
            // Safe tools — always safe regardless of args
            "read_file" | "list_dir" | "glob" | "search_files" | "code_analysis" => {
                OperationLevel::Safe
            }

            // Git operations — safe by default
            "git_status" | "git_diff" | "git_log" => OperationLevel::Safe,

            // Sensitive tools
            "write_file" | "patch_file" | "git_commit" => OperationLevel::Sensitive,

            // Dangerous tools — upgrade based on args
            "bash" => Self::classify_bash(args),
            "delete_file" => OperationLevel::Dangerous,
            "web_fetch" => OperationLevel::Dangerous,

            // Unknown tools default to dangerous
            _ => OperationLevel::Dangerous,
        }
    }

    /// Classify a bash command based on its content.
    fn classify_bash(args: &serde_json::Value) -> OperationLevel {
        let cmd = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        // Known safe commands
        if Self::is_safe_command(&cmd) {
            return OperationLevel::Safe;
        }

        // Known sensitive commands
        if Self::is_sensitive_command(&cmd) {
            return OperationLevel::Sensitive;
        }

        // Everything else is dangerous
        OperationLevel::Dangerous
    }

    /// Check if a bash command is read-only / safe.
    fn is_safe_command(cmd: &str) -> bool {
        let safe_prefixes = [
            "ls", "cat", "head", "tail", "wc", "grep", "find", "echo", "pwd", "which", "whoami",
            "date", "env", "printenv", "type", "file", "stat", "du", "df", "uname", "hostname",
            "id", "tree",
        ];

        let first_word = cmd.split_whitespace().next().unwrap_or("");
        safe_prefixes.contains(&first_word)
    }

    /// Check if a bash command is moderately dangerous.
    fn is_sensitive_command(cmd: &str) -> bool {
        let sensitive_prefixes = [
            "cargo",
            "npm",
            "yarn",
            "pip",
            "make",
            "cmake",
            "mvn",
            "gradle",
            "git add",
            "git commit",
            "git push",
            "git checkout",
            "git branch",
            "mkdir",
            "cp",
            "mv",
            "touch",
            "chmod",
            "chown",
            "docker build",
            "docker run",
        ];

        sensitive_prefixes
            .iter()
            .any(|prefix| cmd.starts_with(prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_file_is_safe() {
        assert_eq!(
            OperationClassifier::classify("read_file", &serde_json::json!({"path": "src/main.rs"})),
            OperationLevel::Safe
        );
    }

    #[test]
    fn test_write_file_is_sensitive() {
        assert_eq!(
            OperationClassifier::classify(
                "write_file",
                &serde_json::json!({"path": "src/main.rs", "content": "fn main() {}"})
            ),
            OperationLevel::Sensitive
        );
    }

    #[test]
    fn test_bash_echo_is_safe() {
        assert_eq!(
            OperationClassifier::classify("bash", &serde_json::json!({"command": "echo hello"})),
            OperationLevel::Safe
        );
    }

    #[test]
    fn test_bash_rm_is_dangerous() {
        assert_eq!(
            OperationClassifier::classify(
                "bash",
                &serde_json::json!({"command": "rm -rf /tmp/test"})
            ),
            OperationLevel::Dangerous
        );
    }

    #[test]
    fn test_bash_cargo_is_sensitive() {
        assert_eq!(
            OperationClassifier::classify("bash", &serde_json::json!({"command": "cargo build"})),
            OperationLevel::Sensitive
        );
    }

    #[test]
    fn test_bash_git_commit_is_sensitive() {
        assert_eq!(
            OperationClassifier::classify(
                "bash",
                &serde_json::json!({"command": "git commit -m 'test'"})
            ),
            OperationLevel::Sensitive
        );
    }

    #[test]
    fn test_unknown_tool_is_dangerous() {
        assert_eq!(
            OperationClassifier::classify("custom_tool", &serde_json::json!({})),
            OperationLevel::Dangerous
        );
    }

    #[test]
    fn test_delete_file_is_dangerous() {
        assert_eq!(
            OperationClassifier::classify(
                "delete_file",
                &serde_json::json!({"path": "src/old.rs"})
            ),
            OperationLevel::Dangerous
        );
    }
}
