//! `agy models` text, recorded on 2026-11-09, parsed without running a CLI.
//! The status line was on stderr and is included so it cannot become a model.

use tutordocs_agent::agy::parse_models;
use tutordocs_agent::Effort;

const RECORDED: &str = include_str!("fixtures/agy/models.txt");

fn levels(words: &[&str]) -> Vec<Effort> {
    words
        .iter()
        .map(|word| Effort::parse(word).unwrap())
        .collect()
}

fn words(models: &[tutordocs_agent::agy::ListedModel]) -> Vec<(&str, &str, Vec<&str>)> {
    models
        .iter()
        .map(|model| {
            (
                model.id.as_str(),
                model.label.as_str(),
                model.efforts.iter().map(Effort::as_str).collect(),
            )
        })
        .collect()
}

#[test]
fn recorded_agy_models_group_by_base_id_and_keep_each_models_efforts() {
    let reported = levels(&["low", "medium", "high", "xhigh", "max"]);
    let models = parse_models(RECORDED, &reported);
    assert_eq!(
        words(&models),
        [
            (
                "gemini-4-argon",
                "Gemini 4 Argon",
                vec!["low", "medium", "high"]
            ),
            (
                "gemini-3.8-flash",
                "Gemini 3.8 Flash",
                vec!["low", "medium", "high"]
            ),
            (
                "claude-opus-5-5",
                "Claude Opus 5.5",
                vec!["low", "medium", "high"]
            ),
            (
                "claude-sonnet-5-5",
                "Claude Sonnet 5.5",
                vec!["low", "medium", "high"]
            ),
            ("gpt-oss-120b", "GPT-OSS 120B", vec!["medium"]),
        ]
    );
}

#[test]
fn a_model_line_whose_id_would_be_a_flag_is_dropped() {
    let models = parse_models("--bad\tNope\nok\tOkay\n", &levels(&["low"]));
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "ok");
    assert_eq!(models[0].label, "Okay");
    assert!(models[0].efforts.is_empty());
}

#[test]
fn an_xhigh_suffix_stays_whole_when_xhigh_was_not_reported() {
    let models = parse_models(
        "custom-xhigh\tCustom (Xhigh)\n",
        &levels(&["low", "medium", "high"]),
    );
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "custom-xhigh");
    assert_eq!(models[0].label, "Custom (Xhigh)");
    assert!(models[0].efforts.is_empty());
}

#[test]
fn an_xhigh_suffix_is_stripped_when_the_help_lists_xhigh() {
    let models = parse_models(
        "custom-xhigh\tCustom (Xhigh)\n",
        &levels(&["low", "medium", "high", "xhigh", "max"]),
    );
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "custom");
    assert_eq!(models[0].label, "Custom");
    assert_eq!(models[0].efforts[0].as_str(), "xhigh");
}
