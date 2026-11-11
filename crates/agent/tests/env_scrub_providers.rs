//! Each non-Claude driver scrubs its own provider's metering variables and
//! still scrubs Claude's, so a stray Anthropic key reaches no spawned agent.

use std::collections::HashMap;

use tutordocs_agent::env::{
    child_env_with, scrub_env_with, AGY_METERING_ENV_VARS, METERING_ENV_VARS,
    QODER_METERING_ENV_VARS,
};

fn dummies(names: &[&str]) -> HashMap<String, String> {
    names
        .iter()
        .map(|k| ((*k).to_string(), "dummy-value".to_string()))
        .collect()
}

#[test]
fn qoder_scrub_covers_the_credential_and_endpoint_switches_it_reads() {
    // Independent source of truth: the variable the Qoder docs say overrides
    // the saved login, plus credential and endpoint names read from the
    // qodercli 1.1.85 binary. Not derived from QODER_METERING_ENV_VARS.
    let read_by_the_cli = [
        "QODER_PERSONAL_ACCESS_TOKEN",
        "QODER_PAT",
        "QODER_SDK_ACCESS_TOKEN",
        "QODER_JOB_TOKEN",
        "QODER_SERVICE_ACCOUNT_KEY",
        "QODER_SDK_CUSTOM_BASE_URL_BYOK",
        "QODER_PROD_BASE_URL",
        "QODER_VPC_ENDPOINT",
    ];
    let out = scrub_env_with(dummies(&read_by_the_cli), QODER_METERING_ENV_VARS);
    let survivors: Vec<_> = read_by_the_cli
        .iter()
        .filter(|k| out.contains_key(**k))
        .collect();
    assert!(survivors.is_empty(), "survived the scrub: {survivors:?}");
}

#[test]
fn qoder_scrub_keeps_the_login_location_and_model_choice() {
    let mut input = dummies(&["QODER_CONFIG_DIR", "QODER_MODEL", "HOME", "PATH"]);
    input.insert("QODER_PERSONAL_ACCESS_TOKEN".into(), "dummy".into());
    let out = scrub_env_with(input, QODER_METERING_ENV_VARS);
    for kept in ["QODER_CONFIG_DIR", "QODER_MODEL", "HOME", "PATH"] {
        assert!(out.contains_key(kept), "{kept} was scrubbed");
    }
    assert!(!out.contains_key("QODER_PERSONAL_ACCESS_TOKEN"));
}

#[test]
fn agy_scrub_covers_the_api_key_adc_vertex_and_gateway_switches_it_reads() {
    // Names read from the agy 1.5.1 binary: the direct Gemini API key pair,
    // the ADC switch, the Vertex and enterprise switches and the LLM gateway.
    let read_by_the_cli = [
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "AGY_ADC_AUTH",
        "GOOGLE_GENAI_USE_VERTEXAI",
        "GOOGLE_GENAI_USE_ENTERPRISE",
        "GOOGLE_VERTEX_BASE_URL",
        "AGY_LLM_GATEWAY_API_KEY",
        "AGY_LLM_GATEWAY_URL",
        "AGY_LLM_GATEWAY_PROXY_URL",
        "AGY_LLM_GATEWAY_HEADERS",
    ];
    let out = scrub_env_with(dummies(&read_by_the_cli), AGY_METERING_ENV_VARS);
    let survivors: Vec<_> = read_by_the_cli
        .iter()
        .filter(|k| out.contains_key(**k))
        .collect();
    assert!(survivors.is_empty(), "survived the scrub: {survivors:?}");
}

#[test]
fn agy_scrub_keeps_ordinary_environment() {
    let out = scrub_env_with(dummies(&["HOME", "PATH", "TERM"]), AGY_METERING_ENV_VARS);
    assert_eq!(out.len(), 3);
}

#[test]
fn provider_scrubs_extend_the_claude_scrub() {
    for extra in [QODER_METERING_ENV_VARS, AGY_METERING_ENV_VARS] {
        let out = scrub_env_with(dummies(METERING_ENV_VARS), extra);
        assert!(out.is_empty(), "a Claude metering var survived: {out:?}");
    }
}

#[test]
fn child_env_with_drops_the_provider_var_even_when_the_parent_sets_it() {
    // A fabricated dummy literal, not a credential; never printed.
    std::env::set_var("GEMINI_API_KEY", "dummy-not-a-key");
    let out = child_env_with(AGY_METERING_ENV_VARS);
    std::env::remove_var("GEMINI_API_KEY");
    assert!(!out.contains_key("GEMINI_API_KEY"));
}
