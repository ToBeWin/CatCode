use crate::Message;

/// Token counter that provides accurate token estimates.
///
/// Uses tiktoken-rs (cl100k_base) when the `tokenizer` feature is enabled.
/// Falls back to a character-counting heuristic when disabled.
pub struct Tokenizer {
    #[allow(dead_code)]
    inner: TokenizerImpl,
}

enum TokenizerImpl {
    #[cfg(feature = "tokenizer")]
    Tiktoken(tiktoken_rs::CoreBPE),
    Fallback,
}

impl Tokenizer {
    /// Create a new tokenizer.
    ///
    /// When the `tokenizer` feature is enabled, initializes cl100k_base encoding.
    /// Otherwise uses a simple heuristic (~4 chars per token).
    pub fn new() -> Self {
        #[cfg(feature = "tokenizer")]
        {
            if let Ok(bpe) = tiktoken_rs::cl100k_base() {
                return Self {
                    inner: TokenizerImpl::Tiktoken(bpe),
                };
            }
        }
        Self {
            inner: TokenizerImpl::Fallback,
        }
    }

    /// Count tokens in a text string.
    pub fn count(&self, text: &str) -> usize {
        #[cfg(feature = "tokenizer")]
        {
            if let TokenizerImpl::Tiktoken(bpe) = &self.inner {
                return bpe.encode_with_special_tokens(text).len();
            }
        }
        // Fallback: ~4 chars per token for English text
        (text.len() / 4).max(1)
    }

    /// Count tokens in multiple messages, accounting for format overhead.
    pub fn count_messages(&self, messages: &[Message]) -> usize {
        let mut total = 0;
        for msg in messages {
            total += self.count(&msg.content);
            total += 4; // role + format overhead
            if let Some(ref name) = msg.name {
                total += self.count(name) + 1;
            }
        }
        total
    }
}

impl Default for Tokenizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenizer_creation() {
        let t = Tokenizer::new();
        let count = t.count("Hello, world!");
        assert!(count > 0);
    }

    #[test]
    fn test_empty_string() {
        let t = Tokenizer::new();
        assert_eq!(t.count(""), 1);
    }

    #[test]
    fn test_long_text_scales() {
        let t = Tokenizer::new();
        let short = t.count("short text");
        let long = t.count("this is a much longer text with many more words in it");
        assert!(long >= short, "longer text should have >= tokens");
    }

    #[test]
    fn test_count_messages() {
        let t = Tokenizer::new();
        let msgs = vec![Message::user("hello"), Message::assistant("hi there")];
        let count = t.count_messages(&msgs);
        assert!(count > 0);
    }

    #[test]
    fn test_estimate_consistency() {
        let t = Tokenizer::new();
        let a = t.count("consistent text for testing purposes");
        let b = t.count("consistent text for testing purposes");
        assert_eq!(a, b);
    }

    #[test]
    fn test_empty_messages() {
        let t = Tokenizer::new();
        assert_eq!(t.count_messages(&[]), 0);
    }
}
