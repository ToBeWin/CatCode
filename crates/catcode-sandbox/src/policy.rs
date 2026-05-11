use std::path::PathBuf;

/// Policy governing sandbox execution.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    /// Paths the tool is allowed to access.
    pub allowed_paths: Vec<PathBuf>,
    /// Paths explicitly denied (takes precedence over allowed).
    pub denied_paths: Vec<PathBuf>,
    /// Network access policy.
    pub network: NetworkPolicy,
    /// Memory limit in MB (0 = unlimited).
    pub memory_limit_mb: u64,
    /// CPU limit as percentage (0.0 = unlimited).
    pub cpu_limit_percent: f32,
    /// Execution timeout in seconds (0 = unlimited).
    pub timeout_secs: u64,
    /// Maximum output size in bytes.
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkPolicy {
    /// No network access.
    Deny,
    /// Full network access.
    Allow,
    /// Only access to these domains/hosts.
    Whitelist(Vec<String>),
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            allowed_paths: Vec::new(),
            denied_paths: Vec::new(),
            network: NetworkPolicy::Deny,
            memory_limit_mb: 512,
            cpu_limit_percent: 50.0,
            timeout_secs: 300,
            max_output_bytes: 1024 * 1024, // 1MB
        }
    }
}

impl SandboxPolicy {
    /// Check if a path is allowed by this policy.
    pub fn is_path_allowed(&self, path: &std::path::Path) -> bool {
        // Explicitly denied paths take precedence
        for denied in &self.denied_paths {
            if path.starts_with(denied) {
                return false;
            }
        }

        // If no allowed paths specified, everything is allowed (except denied)
        if self.allowed_paths.is_empty() {
            return true;
        }

        // Check if path is under any allowed path
        self.allowed_paths
            .iter()
            .any(|allowed| path.starts_with(allowed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = SandboxPolicy::default();
        assert_eq!(policy.network, NetworkPolicy::Deny);
        assert_eq!(policy.memory_limit_mb, 512);
        assert_eq!(policy.timeout_secs, 300);
    }

    #[test]
    fn test_path_allowed_no_restrictions() {
        let policy = SandboxPolicy::default();
        assert!(policy.is_path_allowed(PathBuf::from("/any/path").as_path()));
    }

    #[test]
    fn test_path_denied_overrides() {
        let policy = SandboxPolicy {
            allowed_paths: vec![PathBuf::from("/tmp")],
            denied_paths: vec![PathBuf::from("/tmp/secret")],
            ..Default::default()
        };
        assert!(policy.is_path_allowed(PathBuf::from("/tmp/safe").as_path()));
        assert!(!policy.is_path_allowed(PathBuf::from("/tmp/secret/key").as_path()));
    }

    #[test]
    fn test_path_whitelist() {
        let policy = SandboxPolicy {
            allowed_paths: vec![PathBuf::from("/project"), PathBuf::from("/tmp")],
            denied_paths: Vec::new(),
            ..Default::default()
        };
        assert!(policy.is_path_allowed(PathBuf::from("/project/src/main.rs").as_path()));
        assert!(policy.is_path_allowed(PathBuf::from("/tmp/test").as_path()));
        assert!(!policy.is_path_allowed(PathBuf::from("/etc/passwd").as_path()));
    }
}
