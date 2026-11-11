//! The learner's own words for a rewrite, parsed once at the command boundary.

/// The most characters one instruction carries. The reader builds it from a
/// preset sentence of about 200 characters and a note of at most 500, so a
/// request from the app is never cut; a longer one came from somewhere else.
pub const MAX_INSTRUCTION_CHARS: usize = 1000;

/// What the learner asked a rewrite to change. Trimmed, never empty, and no
/// longer than [`MAX_INSTRUCTION_CHARS`], so the engine can put it in a prompt
/// without checking it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteInstruction(String);

impl RewriteInstruction {
    /// `None` for no instruction or one that is only whitespace, which keeps
    /// the rewrite prompt exactly as it was before instructions existed.
    /// Anything over the cap is cut at a character boundary.
    pub fn parse(raw: Option<&str>) -> Option<Self> {
        let trimmed = raw?.trim();
        if trimmed.is_empty() {
            return None;
        }
        let capped: String = trimmed.chars().take(MAX_INSTRUCTION_CHARS).collect();
        Some(Self(capped.trim_end().to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(raw: &str) -> Option<String> {
        RewriteInstruction::parse(Some(raw)).map(|i| i.as_str().to_string())
    }

    #[test]
    fn no_instruction_and_a_blank_one_are_both_none() {
        assert_eq!(RewriteInstruction::parse(None), None);
        assert_eq!(parsed(""), None);
        assert_eq!(parsed(" \n\t  "), None);
    }

    #[test]
    fn an_instruction_is_trimmed() {
        assert_eq!(
            parsed("  Explain it more simply.\n"),
            Some("Explain it more simply.".to_string())
        );
    }

    #[test]
    fn an_over_long_instruction_is_capped_at_the_limit() {
        let long = "a".repeat(MAX_INSTRUCTION_CHARS + 500);
        let kept = parsed(&long).unwrap();
        assert_eq!(kept.chars().count(), MAX_INSTRUCTION_CHARS);
        assert!(long.starts_with(&kept));
    }

    #[test]
    fn the_cap_counts_characters_and_never_splits_one() {
        let thai = "ง่าย".repeat(MAX_INSTRUCTION_CHARS);
        let kept = parsed(&thai).unwrap();
        assert_eq!(kept.chars().count(), MAX_INSTRUCTION_CHARS);
        assert!(thai.starts_with(&kept));
    }

    #[test]
    fn an_instruction_exactly_at_the_cap_is_kept_whole() {
        let exact = "b".repeat(MAX_INSTRUCTION_CHARS);
        assert_eq!(parsed(&exact), Some(exact));
    }
}
