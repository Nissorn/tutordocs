//! The boundary that keeps a model id from becoming a flag, and the closed
//! effort vocabulary both CLIs accept.

use tutordocs_agent::{Effort, ModelId};

#[test]
fn a_model_that_starts_with_a_dash_is_rejected() {
    let err = ModelId::parse("--opus").unwrap_err();
    assert_eq!(err, "model must not start with '-'");
    assert_eq!(
        ModelId::parse("-").unwrap_err(),
        "model must not start with '-'"
    );
}

#[test]
fn an_empty_model_is_rejected() {
    assert_eq!(ModelId::parse("").unwrap_err(), "model is empty");
}

#[test]
fn a_model_id_keeps_letters_digits_and_the_listed_punctuation() {
    let id = ModelId::parse("claude-opus-4.6:high_1").unwrap();
    assert_eq!(id.as_str(), "claude-opus-4.6:high_1");
    assert!(ModelId::parse("sonnet haiku").is_err());
    assert!(ModelId::parse("opus/sonnet").is_err());
    assert!(ModelId::parse("opus;rm").is_err());
}

#[test]
fn an_effort_is_a_lowercase_word_and_a_new_spelling_is_not_rejected() {
    let level = Effort::parse("turbo").unwrap();
    assert_eq!(level.as_str(), "turbo");
    assert_eq!(serde_json::to_string(&level).unwrap(), "\"turbo\"");
    assert_eq!(
        serde_json::from_str::<Effort>("\"max\"").unwrap().as_str(),
        "max"
    );
    assert_eq!(Effort::parse("").unwrap_err(), "effort is empty");
    assert_eq!(
        Effort::parse("-low").unwrap_err(),
        "effort must not start with '-'"
    );
    assert!(Effort::parse("High").is_err());
    assert!(Effort::parse("x-high").is_err());
    assert!(Effort::parse("low1").is_err());
}
