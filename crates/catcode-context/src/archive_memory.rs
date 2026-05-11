use catcode_core::memory::{ArchiveFact, FactCategory};
use tracing::debug;

/// In-memory structured fact store for long-term knowledge.
///
/// `ArchiveMemory` stores discrete facts about the user, project, and
/// environment. Each fact has a confidence score and category. Facts
/// below the confidence threshold are pruned on maintenance cycles.
///
/// This is a Phase 1 in-memory implementation. SQLite-backed persistence
/// will be added in a future phase.
///
/// # Example
///
/// ```
/// use catcode_context::ArchiveMemory;
/// use catcode_core::memory::{ArchiveFact, FactCategory};
///
/// let mut memory = ArchiveMemory::new(100, 0.7);
/// memory.add_fact(ArchiveFact::new(
///     "User prefers Rust over Python",
///     FactCategory::Preference,
///     0.9,
/// )).unwrap();
///
/// let facts = memory.get_facts(Some(FactCategory::Preference));
/// assert_eq!(facts.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct ArchiveMemory {
    /// Maximum number of facts to retain.
    pub max_facts: usize,
    /// Minimum confidence threshold for keeping facts.
    pub confidence_threshold: f32,
    /// Internal fact store.
    facts: Vec<ArchiveFact>,
}

impl ArchiveMemory {
    /// Create a new archive memory store.
    ///
    /// # Arguments
    /// * `max_facts` — maximum number of facts to keep (oldest removed when exceeded)
    /// * `confidence_threshold` — facts below this score are pruned on `prune()`
    pub fn new(max_facts: usize, confidence_threshold: f32) -> Self {
        Self {
            max_facts,
            confidence_threshold: confidence_threshold.clamp(0.0, 1.0),
            facts: Vec::new(),
        }
    }

    /// Add a fact to the archive.
    ///
    /// Returns an error if the fact would exceed `max_facts` after pruning
    /// is not possible (i.e. all existing facts have higher confidence).
    /// In practice, `add_fact` always succeeds — the caller should call
    /// `prune()` periodically to enforce limits.
    pub fn add_fact(&mut self, fact: ArchiveFact) -> anyhow::Result<()> {
        debug!(
            category = %fact.category,
            confidence = fact.confidence,
            "Adding archive fact"
        );
        self.facts.push(fact);
        Ok(())
    }

    /// Retrieve facts, optionally filtered by category.
    ///
    /// If `category` is `None`, all facts are returned.
    pub fn get_facts(&self, category: Option<FactCategory>) -> Vec<&ArchiveFact> {
        match category {
            Some(cat) => self.facts.iter().filter(|f| f.category == cat).collect(),
            None => self.facts.iter().collect(),
        }
    }

    /// Remove facts below the confidence threshold and enforce `max_facts`.
    ///
    /// Pruning happens in two phases:
    /// 1. Remove all facts with confidence below `confidence_threshold`
    /// 2. If still over `max_facts`, remove lowest-confidence facts first
    pub fn prune(&mut self) {
        let before = self.facts.len();

        // Phase 1: remove below threshold
        self.facts
            .retain(|f| f.confidence >= self.confidence_threshold);

        // Phase 2: enforce max_facts by removing lowest-confidence entries
        if self.facts.len() > self.max_facts {
            // Sort by confidence ascending so lowest are first
            self.facts.sort_by(|a, b| {
                a.confidence
                    .partial_cmp(&b.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let excess = self.facts.len() - self.max_facts;
            self.facts.drain(0..excess);
        }

        let removed = before - self.facts.len();
        if removed > 0 {
            debug!(
                removed,
                remaining = self.facts.len(),
                "Pruned archive facts"
            );
        }
    }

    /// Get the current number of stored facts.
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Check if the archive is empty.
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_archive() -> ArchiveMemory {
        ArchiveMemory::new(100, 0.7)
    }

    #[test]
    fn test_new_archive() {
        let archive = make_archive();
        assert_eq!(archive.max_facts, 100);
        assert!((archive.confidence_threshold - 0.7).abs() < 0.01);
        assert!(archive.is_empty());
        assert_eq!(archive.len(), 0);
    }

    #[test]
    fn test_add_and_get_fact() {
        let mut archive = make_archive();
        let fact = ArchiveFact::new("User likes Rust", FactCategory::Preference, 0.9);
        archive.add_fact(fact).unwrap();

        let facts = archive.get_facts(None);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "User likes Rust");
        assert_eq!(facts[0].category, FactCategory::Preference);
        assert!((facts[0].confidence - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_get_facts_by_category() {
        let mut archive = make_archive();

        archive
            .add_fact(ArchiveFact::new("pref1", FactCategory::Preference, 0.9))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("know1", FactCategory::Knowledge, 0.8))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("pref2", FactCategory::Preference, 0.85))
            .unwrap();

        let prefs = archive.get_facts(Some(FactCategory::Preference));
        assert_eq!(prefs.len(), 2);

        let knows = archive.get_facts(Some(FactCategory::Knowledge));
        assert_eq!(knows.len(), 1);

        let ctx = archive.get_facts(Some(FactCategory::Context));
        assert_eq!(ctx.len(), 0);
    }

    #[test]
    fn test_prune_below_threshold() {
        let mut archive = make_archive(); // threshold 0.7

        archive
            .add_fact(ArchiveFact::new("high", FactCategory::Knowledge, 0.9))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("medium", FactCategory::Knowledge, 0.7))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("low", FactCategory::Knowledge, 0.5))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("very_low", FactCategory::Knowledge, 0.3))
            .unwrap();

        archive.prune();

        let facts = archive.get_facts(None);
        assert_eq!(facts.len(), 2);
        // "low" and "very_low" should be removed
        let contents: Vec<&str> = facts.iter().map(|f| f.content.as_str()).collect();
        assert!(contents.contains(&"high"));
        assert!(contents.contains(&"medium"));
    }

    #[test]
    fn test_prune_exceeds_max_facts() {
        let mut archive = ArchiveMemory::new(3, 0.5);

        archive
            .add_fact(ArchiveFact::new("a", FactCategory::Knowledge, 0.6))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("b", FactCategory::Knowledge, 0.7))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("c", FactCategory::Knowledge, 0.8))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("d", FactCategory::Knowledge, 0.9))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("e", FactCategory::Knowledge, 0.95))
            .unwrap();

        archive.prune();

        // Should keep only top 3 by confidence
        assert_eq!(archive.len(), 3);
        let facts = archive.get_facts(None);
        let contents: Vec<&str> = facts.iter().map(|f| f.content.as_str()).collect();
        assert!(contents.contains(&"c"));
        assert!(contents.contains(&"d"));
        assert!(contents.contains(&"e"));
        // "a" and "b" should be removed (lowest confidence)
        assert!(!contents.contains(&"a"));
        assert!(!contents.contains(&"b"));
    }

    #[test]
    fn test_prune_empty() {
        let mut archive = make_archive();
        archive.prune(); // Should not panic
        assert!(archive.is_empty());
    }

    #[test]
    fn test_prune_all_below_threshold() {
        let mut archive = make_archive();

        archive
            .add_fact(ArchiveFact::new("low1", FactCategory::Knowledge, 0.3))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("low2", FactCategory::Knowledge, 0.5))
            .unwrap();

        archive.prune();
        assert!(archive.is_empty());
    }

    #[test]
    fn test_prune_exact_threshold_kept() {
        let mut archive = make_archive(); // threshold 0.7

        archive
            .add_fact(ArchiveFact::new("exact", FactCategory::Knowledge, 0.7))
            .unwrap();

        archive.prune();
        assert_eq!(archive.len(), 1);
    }

    #[test]
    fn test_len_and_is_empty() {
        let mut archive = make_archive();
        assert!(archive.is_empty());
        assert_eq!(archive.len(), 0);

        archive
            .add_fact(ArchiveFact::new("fact", FactCategory::Knowledge, 0.9))
            .unwrap();
        assert!(!archive.is_empty());
        assert_eq!(archive.len(), 1);
    }

    #[test]
    fn test_confidence_threshold_clamped() {
        let archive = ArchiveMemory::new(100, 1.5);
        assert!((archive.confidence_threshold - 1.0).abs() < 0.001);

        let archive = ArchiveMemory::new(100, -0.5);
        assert!((archive.confidence_threshold).abs() < 0.001);
    }

    #[test]
    fn test_add_multiple_facts() {
        let mut archive = make_archive();

        for i in 0..50 {
            archive
                .add_fact(ArchiveFact::new(
                    format!("fact-{i}"),
                    FactCategory::Knowledge,
                    0.8,
                ))
                .unwrap();
        }

        assert_eq!(archive.len(), 50);

        archive.prune();
        assert_eq!(archive.len(), 50); // All above threshold, under max
    }

    #[test]
    fn test_prune_mixed_confidence_with_max() {
        let mut archive = ArchiveMemory::new(2, 0.5);

        archive
            .add_fact(ArchiveFact::new("low", FactCategory::Knowledge, 0.5))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("medium", FactCategory::Knowledge, 0.7))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("high", FactCategory::Knowledge, 0.9))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new("highest", FactCategory::Knowledge, 1.0))
            .unwrap();

        archive.prune();

        // All above threshold, but max_facts=2 so only top 2 survive
        assert_eq!(archive.len(), 2);
        let facts = archive.get_facts(None);
        let contents: Vec<&str> = facts.iter().map(|f| f.content.as_str()).collect();
        assert!(contents.contains(&"high"));
        assert!(contents.contains(&"highest"));
    }
}
