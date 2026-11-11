//! Catalogs built from the help and model listings recorded on 2026-11-09.

use tutordocs_agent::catalog::driver_catalog;
use tutordocs_agent::Effort;

fn spellings(levels: &[Effort]) -> Vec<&str> {
    levels.iter().map(Effort::as_str).collect()
}

#[test]
fn recorded_agy_output_builds_a_catalog_per_model() {
    let help = include_str!("fixtures/agy/help.stderr.txt");
    let models = include_str!("fixtures/agy/models.txt");
    let catalog = driver_catalog("agy", help, Some(models));
    assert_eq!(
        spellings(&catalog.efforts),
        ["low", "medium", "high", "xhigh", "max"]
    );
    assert!(!catalog.accepts_custom_model);
    let flash = catalog
        .models
        .iter()
        .find(|model| model.id.as_deref() == Some("gemini-3.8-flash"))
        .unwrap();
    assert_eq!(spellings(&flash.efforts), ["low", "medium", "high"]);
    let oss = catalog
        .models
        .iter()
        .find(|model| model.id.as_deref() == Some("gpt-oss-120b"))
        .unwrap();
    assert_eq!(spellings(&oss.efforts), ["medium"]);
    assert_eq!(catalog.models[0].id, None);
    assert_eq!(
        spellings(&catalog.models[0].efforts),
        spellings(&catalog.efforts)
    );
}

#[test]
fn recorded_claude_help_names_three_aliases_and_one_effort_list() {
    let help = include_str!("fixtures/claude/help.txt");
    let catalog = driver_catalog("claude", help, None);
    assert!(catalog.accepts_custom_model);
    let ids: Vec<Option<&str>> = catalog
        .models
        .iter()
        .map(|model| model.id.as_deref())
        .collect();
    assert_eq!(ids, [None, Some("fable"), Some("opus"), Some("sonnet")]);
    assert_eq!(
        spellings(&catalog.efforts),
        ["low", "medium", "high", "xhigh", "max"]
    );
    assert!(catalog
        .models
        .iter()
        .all(|model| spellings(&model.efforts) == spellings(&catalog.efforts)));
}

#[test]
fn recorded_qoder_help_reports_no_model_aliases_and_no_effort_list() {
    let help = include_str!("fixtures/qoder/help.txt");
    let catalog = driver_catalog("qoder", help, None);
    assert!(catalog.efforts.is_empty());
    assert_eq!(catalog.models.len(), 1);
    assert_eq!(catalog.models[0].id, None);
    assert!(catalog.models[0].efforts.is_empty());
    assert!(!catalog.accepts_custom_model);
}

#[test]
fn help_that_names_the_flag_without_a_list_yields_an_empty_list() {
    let catalog = driver_catalog("claude", "  --effort <level>  Effort level\n", None);
    assert!(catalog.efforts.is_empty());
    assert_eq!(catalog.models.len(), 1);
}

#[test]
fn an_empty_help_text_does_not_crash_and_lists_only_the_cli_default() {
    let catalog = driver_catalog("agy", "", None);
    assert!(catalog.efforts.is_empty());
    assert_eq!(catalog.models.len(), 1);
    assert_eq!(catalog.models[0].label, "CLI default");
}

#[test]
fn a_reasoning_effort_list_is_used_when_effort_names_none() {
    let help = "  --reasoning-effort <level>  Set it (low|max)\n";
    let catalog = driver_catalog("qoder", help, None);
    assert_eq!(spellings(&catalog.efforts), ["low", "max"]);
}
