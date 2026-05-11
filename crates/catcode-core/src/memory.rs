use serde::{Deserialize, Serialize};
use std::fmt;

// === MemoryType ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
}

impl fmt::Display for MemoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Feedback => write!(f, "feedback"),
            Self::Project => write!(f, "project"),
            Self::Reference => write!(f, "reference"),
        }
    }
}

// === MemoryEntry ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub name: String,
    pub description: String,
    pub memory_type: MemoryType,
    pub content: String,
}

// === FactCategory ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FactCategory {
    Preference,
    Knowledge,
    Context,
    Behavior,
    Goal,
}

impl fmt::Display for FactCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preference => write!(f, "preference"),
            Self::Knowledge => write!(f, "knowledge"),
            Self::Context => write!(f, "context"),
            Self::Behavior => write!(f, "behavior"),
            Self::Goal => write!(f, "goal"),
        }
    }
}

// === ArchiveFact ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveFact {
    pub id: String,
    pub content: String,
    pub category: FactCategory,
    pub confidence: f32,
    pub source: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ArchiveFact {
    pub fn new(content: impl Into<String>, category: FactCategory, confidence: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.into(),
            category,
            confidence: confidence.clamp(0.0, 1.0),
            source: "manual".to_string(),
            created_at: chrono::Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_type_display() {
        assert_eq!(MemoryType::User.to_string(), "user");
        assert_eq!(MemoryType::Feedback.to_string(), "feedback");
        assert_eq!(MemoryType::Project.to_string(), "project");
        assert_eq!(MemoryType::Reference.to_string(), "reference");
    }

    #[test]
    fn test_memory_entry_creation() {
        let entry = MemoryEntry {
            name: "deepseek_default".to_string(),
            description: "Use DeepSeek as default provider".to_string(),
            memory_type: MemoryType::Feedback,
            content: "Always use DeepSeek first.".to_string(),
        };
        assert_eq!(entry.memory_type, MemoryType::Feedback);
    }

    #[test]
    fn test_archive_fact_confidence_clamp() {
        let fact = ArchiveFact::new("test", FactCategory::Preference, 1.5);
        assert!(fact.confidence <= 1.0);

        let fact2 = ArchiveFact::new("test", FactCategory::Preference, -0.5);
        assert!(fact2.confidence >= 0.0);
    }

    #[test]
    fn test_fact_category_display() {
        assert_eq!(FactCategory::Preference.to_string(), "preference");
        assert_eq!(FactCategory::Knowledge.to_string(), "knowledge");
    }
}
