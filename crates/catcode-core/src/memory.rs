use serde::{Deserialize, Serialize};
use std::fmt;

// === MemoryType ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// [`MemoryType`]
pub enum MemoryType {
    /// [`User`].
    User,
    /// [`Feedback`].
    Feedback,
    /// [`Project`].
    Project,
    /// [`Reference`].
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
/// [`MemoryEntry`]
pub struct MemoryEntry {
    pub name: String,
    pub description: String,
    pub memory_type: MemoryType,
    pub content: String,
}

// === FactCategory ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// [`FactCategory`]
pub enum FactCategory {
    /// [`Preference`].
    Preference,
    /// [`Knowledge`].
    Knowledge,
    /// [`Context`].
    Context,
    /// [`Behavior`].
    Behavior,
    /// [`Goal`].
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
/// [`ArchiveFact`]
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
        assert_eq!(entry.name, "deepseek_default");
        assert_eq!(entry.description, "Use DeepSeek as default provider");
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

    #[test]
    fn test_memory_type_serialization() {
        let json = serde_json::to_string(&MemoryType::User).unwrap();
        assert_eq!(json, "\"user\"");
        let deserialized: MemoryType = serde_json::from_str("\"feedback\"").unwrap();
        assert_eq!(deserialized, MemoryType::Feedback);
    }

    #[test]
    fn test_fact_category_serialization() {
        let json = serde_json::to_string(&FactCategory::Goal).unwrap();
        assert_eq!(json, "\"goal\"");
        let categories: Vec<FactCategory> =
            serde_json::from_str(r#"["preference","knowledge","context","behavior","goal"]"#)
                .unwrap();
        assert_eq!(categories.len(), 5);
    }

    #[test]
    fn test_fact_category_all_variants_display() {
        assert_eq!(FactCategory::Context.to_string(), "context");
        assert_eq!(FactCategory::Behavior.to_string(), "behavior");
        assert_eq!(FactCategory::Goal.to_string(), "goal");
    }

    #[test]
    fn test_memory_type_all_variants() {
        assert_eq!(format!("{:?}", MemoryType::User), "User");
        assert_eq!(format!("{:?}", MemoryType::Feedback), "Feedback");
        assert_eq!(format!("{:?}", MemoryType::Project), "Project");
        assert_eq!(format!("{:?}", MemoryType::Reference), "Reference");
    }

    #[test]
    fn test_archive_fact_field_access() {
        let fact = ArchiveFact::new("user prefers dark mode", FactCategory::Preference, 0.9);
        assert!(fact.content.contains("dark mode"));
        assert_eq!(fact.category, FactCategory::Preference);
        assert!((fact.confidence - 0.9).abs() < f32::EPSILON);
        assert_eq!(fact.source, "manual");
        assert!(!fact.id.is_empty());
    }

    #[test]
    fn test_memory_entry_serialization_roundtrip() {
        let entry = MemoryEntry {
            name: "test".to_string(),
            description: "desc".to_string(),
            memory_type: MemoryType::Project,
            content: "content".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: MemoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.memory_type, MemoryType::Project);
    }

    #[test]
    fn test_archive_fact_serialization_roundtrip() {
        let fact = ArchiveFact::new("remember this", FactCategory::Knowledge, 0.8);
        let json = serde_json::to_string(&fact).unwrap();
        let deserialized: ArchiveFact = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.content, "remember this");
        assert_eq!(deserialized.category, FactCategory::Knowledge);
        assert!((deserialized.confidence - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_archive_fact_valid_confidence() {
        let fact = ArchiveFact::new("valid", FactCategory::Goal, 0.5);
        assert!((fact.confidence - 0.5).abs() < f32::EPSILON);
    }
}
