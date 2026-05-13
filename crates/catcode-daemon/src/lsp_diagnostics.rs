use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: PathBuf,
    pub line: u64,
    pub column: u64,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticFile {
    pub file: PathBuf,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticAttachment {
    pub summary: String,
    pub details: Vec<String>,
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
struct DiagnosticKey {
    line: u64,
    column: u64,
    message: String,
}

pub struct DiagnosticRegistry {
    max_per_file: usize,
    max_total: usize,
    delivered_cache: LruCache<PathBuf, HashSet<DiagnosticKey>>,
}

impl DiagnosticRegistry {
    pub fn new() -> Self {
        Self {
            max_per_file: 10,
            max_total: 30,
            delivered_cache: LruCache::new(NonZeroUsize::new(100).unwrap()),
        }
    }

    pub fn register(&mut self, file: &Path, incoming: Vec<Diagnostic>) -> Vec<Diagnostic> {
        if !self.delivered_cache.contains(file) {
            self.delivered_cache.put(file.to_path_buf(), HashSet::new());
        }
        let delivered = self
            .delivered_cache
            .get_mut(file)
            .expect("just inserted");

        let mut new_diags = Vec::new();
        for d in incoming {
            let key = DiagnosticKey {
                line: d.line,
                column: d.column,
                message: d.message.clone(),
            };
            if delivered.insert(key) {
                new_diags.push(d);
            }
        }

        new_diags.truncate(self.max_per_file);
        new_diags
    }

    pub fn clear_file(&mut self, file: &Path) {
        self.delivered_cache.pop(file);
    }

    pub fn build_attachment(&self, files_with_diags: &[DiagnosticFile]) -> Option<DiagnosticAttachment> {
        let mut all: Vec<&Diagnostic> = Vec::new();
        for df in files_with_diags {
            if let Some(delivered_set) = self.delivered_cache.peek(&df.file) {
                for d in &df.diagnostics {
                    let key = DiagnosticKey {
                        line: d.line,
                        column: d.column,
                        message: d.message.clone(),
                    };
                    if delivered_set.contains(&key) {
                        all.push(d);
                    }
                }
            }
        }

        all.sort_by_key(|d| match d.severity {
            DiagnosticSeverity::Error => 0,
            DiagnosticSeverity::Warning => 1,
            _ => 2,
        });
        all.truncate(self.max_total);

        if all.is_empty() {
            return None;
        }

        let errors = all
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count();
        let warnings = all
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count();
        let summary = format!("LSP diagnostics: {} errors, {} warnings", errors, warnings);

        let details: Vec<String> = all
            .iter()
            .map(|d| {
                format!(
                    "{}:{}:{} [{:?}] {}",
                    d.file.display(),
                    d.line,
                    d.column,
                    d.severity,
                    d.message
                )
            })
            .collect();

        Some(DiagnosticAttachment { summary, details })
    }
}

pub struct LspWatcher {
    registry: Arc<Mutex<DiagnosticRegistry>>,
    watch_paths: Vec<PathBuf>,
}

impl LspWatcher {
    pub fn new(registry: Arc<Mutex<DiagnosticRegistry>>) -> Self {
        Self {
            registry,
            watch_paths: Vec::new(),
        }
    }

    pub fn add_watch(&mut self, path: PathBuf) {
        self.watch_paths.push(path);
    }

    pub fn poll(&self) -> Vec<DiagnosticFile> {
        let mut results = Vec::new();
        for path in &self.watch_paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(diags) = serde_json::from_str::<Vec<Diagnostic>>(&content) {
                    let mut registry = self.registry.blocking_lock();
                    let new = registry.register(path, diags);
                    if !new.is_empty() {
                        results.push(DiagnosticFile {
                            file: path.clone(),
                            diagnostics: new,
                        });
                    }
                }
            }
        }
        results
    }

    pub fn clear_file(&self, path: &Path) {
        let mut registry = self.registry.blocking_lock();
        registry.clear_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(
        file: &str,
        line: u64,
        column: u64,
        severity: DiagnosticSeverity,
        message: &str,
    ) -> Diagnostic {
        Diagnostic {
            file: PathBuf::from(file),
            line,
            column,
            severity,
            message: message.to_string(),
            code: None,
        }
    }

    #[test]
    fn test_registry_dedup_same_file() {
        let mut registry = DiagnosticRegistry::new();
        let file = Path::new("src/main.rs");

        let diags = vec![
            diag("src/main.rs", 1, 1, DiagnosticSeverity::Error, "first error"),
            diag("src/main.rs", 1, 1, DiagnosticSeverity::Error, "first error"),
            diag("src/main.rs", 2, 1, DiagnosticSeverity::Warning, "second warning"),
        ];

        let new = registry.register(file, diags);
        assert_eq!(new.len(), 2);
    }

    #[test]
    fn test_registry_limits_per_file() {
        let mut registry = DiagnosticRegistry::new();
        registry.max_per_file = 3;
        let file = Path::new("src/main.rs");

        let diags: Vec<Diagnostic> = (0..10)
            .map(|i| diag("src/main.rs", i, 1, DiagnosticSeverity::Error, &format!("error {}", i)))
            .collect();

        let new = registry.register(file, diags);
        assert_eq!(new.len(), 3);
    }

    #[test]
    fn test_clear_file_after_edit() {
        let mut registry = DiagnosticRegistry::new();
        let file = Path::new("src/main.rs");

        let diags = vec![diag("src/main.rs", 1, 1, DiagnosticSeverity::Error, "error")];

        let new = registry.register(file, diags.clone());
        assert_eq!(new.len(), 1);

        let new = registry.register(file, diags.clone());
        assert_eq!(new.len(), 0);

        registry.clear_file(file);
        let new = registry.register(file, diags);
        assert_eq!(new.len(), 1);
    }

    #[test]
    fn test_diagnostic_attachment_prioritizes_errors() {
        let mut registry = DiagnosticRegistry::new();
        let file = Path::new("src/main.rs");

        registry.register(
            file,
            vec![
                diag("src/main.rs", 1, 1, DiagnosticSeverity::Warning, "warning"),
                diag("src/main.rs", 2, 1, DiagnosticSeverity::Error, "error"),
                diag("src/main.rs", 3, 1, DiagnosticSeverity::Info, "info"),
            ],
        );

        let df = DiagnosticFile {
            file: PathBuf::from("src/main.rs"),
            diagnostics: vec![
                diag("src/main.rs", 1, 1, DiagnosticSeverity::Warning, "warning"),
                diag("src/main.rs", 2, 1, DiagnosticSeverity::Error, "error"),
                diag("src/main.rs", 3, 1, DiagnosticSeverity::Info, "info"),
            ],
        };

        let attachment = registry.build_attachment(&[df]).unwrap();
        assert!(attachment.summary.contains("1 errors"));
        assert!(attachment.summary.contains("1 warnings"));
        assert_eq!(attachment.details.len(), 3);
        assert!(attachment.details[0].contains("Error"));
    }

    #[test]
    fn test_empty_registry_returns_none() {
        let registry = DiagnosticRegistry::new();
        let attachment = registry.build_attachment(&[]);
        assert!(attachment.is_none());
    }

    #[test]
    fn test_registry_limits_total() {
        let mut registry = DiagnosticRegistry::new();
        registry.max_total = 2;

        for i in 0..5 {
            let file = PathBuf::from(format!("src/file{}.rs", i));
            registry.register(
                &file,
                vec![diag(
                    file.to_str().unwrap(),
                    1,
                    1,
                    DiagnosticSeverity::Error,
                    &format!("error {}", i),
                )],
            );
        }

        let dfs: Vec<DiagnosticFile> = (0..5)
            .map(|i| {
                let file = PathBuf::from(format!("src/file{}.rs", i));
                DiagnosticFile {
                    file: file.clone(),
                    diagnostics: vec![diag(
                        file.to_str().unwrap(),
                        1,
                        1,
                        DiagnosticSeverity::Error,
                        &format!("error {}", i),
                    )],
                }
            })
            .collect();

        let attachment = registry.build_attachment(&dfs).unwrap();
        assert!(attachment.details.len() <= 2);
    }
}
