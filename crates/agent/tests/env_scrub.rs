//! The env scrub is the user's core requirement, so it is proven two ways:
//! a pure map test and a test that really sets the var in the parent process.

use std::collections::HashMap;
use tutordocs_agent::env::{child_env, scrub_env, METERING_ENV_VARS};

#[test]
fn scrub_removes_every_metering_var_and_keeps_the_rest() {
    let mut input = HashMap::new();
    for key in METERING_ENV_VARS {
        input.insert((*key).to_string(), "dummy-value".to_string());
    }
    input.insert("PATH".to_string(), "/usr/bin".to_string());
    input.insert("HOME".to_string(), "/Users/test".to_string());

    let out = scrub_env(input);

    for key in METERING_ENV_VARS {
        assert!(!out.contains_key(*key), "{key} survived the scrub");
    }
    assert_eq!(out.get("PATH").map(String::as_str), Some("/usr/bin"));
    assert_eq!(out.get("HOME").map(String::as_str), Some("/Users/test"));
}

#[test]
fn child_env_lacks_the_key_even_when_the_parent_sets_it() {
    // A fabricated dummy literal, not a credential; never printed.
    std::env::set_var("ANTHROPIC_API_KEY", "dummy-not-a-key");
    let out = child_env();
    std::env::remove_var("ANTHROPIC_API_KEY");

    assert!(
        !out.contains_key("ANTHROPIC_API_KEY"),
        "ANTHROPIC_API_KEY reached the child environment"
    );
}

#[test]
fn scrub_covers_every_provider_and_credential_switch_the_cli_reads() {
    // Independent source of truth: the names found in the claude 2.1.319
    // binary that select a provider, supply a credential or redirect the
    // endpoint. Not derived from METERING_ENV_VARS.
    let read_by_the_cli = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_IDENTITY_TOKEN",
        "ANTHROPIC_IDENTITY_TOKEN_FILE",
        "ANTHROPIC_ENVIRONMENT_KEY",
        "ANTHROPIC_AWS_API_KEY",
        "ANTHROPIC_AWS_BASE_URL",
        "ANTHROPIC_AWS_WORKSPACE_ID",
        "ANTHROPIC_BEDROCK_BASE_URL",
        "ANTHROPIC_BEDROCK_MANTLE_BASE_URL",
        "ANTHROPIC_FOUNDRY_API_KEY",
        "ANTHROPIC_FOUNDRY_AUTH_TOKEN",
        "ANTHROPIC_FOUNDRY_BASE_URL",
        "ANTHROPIC_FOUNDRY_RESOURCE",
        "ANTHROPIC_GOOGLE_CLOUD_BASE_URL",
        "ANTHROPIC_VERTEX_BASE_URL",
        "AWS_BEARER_TOKEN_BEDROCK",
        "CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR",
        "CLAUDE_CODE_USE_ANTHROPIC_AWS",
        "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_FOUNDRY",
        "CLAUDE_CODE_USE_GATEWAY",
        "CLAUDE_CODE_USE_MANTLE",
        "CLAUDE_CODE_USE_VERTEX",
    ];
    let input: HashMap<String, String> = read_by_the_cli
        .iter()
        .map(|k| ((*k).to_string(), "dummy-value".to_string()))
        .collect();

    let out = scrub_env(input);

    let survivors: Vec<_> = read_by_the_cli
        .iter()
        .filter(|k| out.contains_key(**k))
        .collect();
    assert!(survivors.is_empty(), "survived the scrub: {survivors:?}");
}

#[test]
fn scrub_keeps_the_subscription_token_and_model_choice() {
    let mut input = HashMap::new();
    input.insert("CLAUDE_CODE_OAUTH_TOKEN".to_string(), "dummy".to_string());
    input.insert("ANTHROPIC_MODEL".to_string(), "dummy-model".to_string());

    let out = scrub_env(input);

    assert!(out.contains_key("CLAUDE_CODE_OAUTH_TOKEN"));
    assert!(out.contains_key("ANTHROPIC_MODEL"));
}
