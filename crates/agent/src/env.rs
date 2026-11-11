//! The keyless guarantee: metering env vars never reach the agent child.
//!
//! This scrub is a TutorDocs addition, not a DeepTutor port. Upstream
//! `claude_code_provider.py` passes no `env` to the SDK, so the child inherits
//! the parent environment and a set `ANTHROPIC_API_KEY` silently meters
//! pay-as-you-go billing (PROPOSAL.md "Agent drivers", CONTEXT.md trap).

use std::collections::HashMap;

/// Variables that switch Claude Code off the subscription login, supply a
/// credential, or route the request through another provider or endpoint.
/// The names are the ones the claude 2.1.319 binary reads. Base-URL variables
/// are included because a proxy can capture or meter a request even without a
/// key. `CLAUDE_CODE_OAUTH_TOKEN` is deliberately absent: it is the
/// subscription credential, not a metering path.
pub const METERING_ENV_VARS: &[&str] = &[
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

/// Variables that make the Qoder CLI authenticate some way other than the
/// saved browser login, or talk to another backend. The Qoder docs say
/// `QODER_PERSONAL_ACCESS_TOKEN` wins over the saved login; the rest are
/// credential and endpoint names read as strings from the qodercli 1.1.85
/// binary. `QODER_CONFIG_DIR` (where the login lives) and `QODER_MODEL` are
/// deliberately absent: they locate or choose, they do not meter.
pub const QODER_METERING_ENV_VARS: &[&str] = &[
    "QODER_PERSONAL_ACCESS_TOKEN",
    "QODER_PAT",
    "QODER_ENV_PAT",
    "QODER_SDK_ACCESS_TOKEN",
    "QODER_JOB_TOKEN",
    "QODER_SERVICE_ACCOUNT_KEY",
    "QODER_SDK_AUTH_PAYLOAD_FILE",
    "QODER_AUTH_STDIN",
    "QODER_SDK_CUSTOM_BASE_URL_BYOK",
    "QODER_PROD_BASE_URL",
    "QODER_DAILY_BASE_URL",
    "QODER_TEST_BASE_URL",
    "QODER_VPC_ENDPOINT",
    "QODER_CONFIG_SERVICE_URL",
];

/// Variables that switch the agy (Antigravity) CLI off the cached Google
/// login: the direct Gemini API key pair, the Application Default Credentials
/// switch, the Vertex and enterprise switches and the LLM gateway family.
/// Read as names from the agy 1.5.1 binary. `GOOGLE_CLOUD_PROJECT` is
/// deliberately absent: it names a project, it does not select a billing path.
pub const AGY_METERING_ENV_VARS: &[&str] = &[
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
    "AGY_LLM_GATEWAY_CA_CERT",
    "AGY_LLM_GATEWAY_CLIENT_CERT",
    "AGY_LLM_GATEWAY_CLIENT_KEY",
    "AGY_LLM_GATEWAY_MODELS",
    "AGY_LLM_GATEWAY_WIRE_PROTOCOL",
    "AGY_LLM_GATEWAY_TIER_FLASH_LITE",
    "AGY_LLM_GATEWAY_TIER_PRO",
];

/// Remove every Claude metering variable from an environment map.
pub fn scrub_env(vars: HashMap<String, String>) -> HashMap<String, String> {
    scrub_env_with(vars, &[])
}

/// Remove the Claude metering variables and `extra` (one provider's list).
/// Every driver scrubs Claude's names too, so a stray Anthropic key reaches
/// no spawned agent even if that agent can be pointed at another backend.
pub fn scrub_env_with(vars: HashMap<String, String>, extra: &[&str]) -> HashMap<String, String> {
    vars.into_iter()
        .filter(|(key, _)| {
            !METERING_ENV_VARS.contains(&key.as_str()) && !extra.contains(&key.as_str())
        })
        .collect()
}

/// The explicit child environment: the parent env minus Claude metering variables.
pub fn child_env() -> HashMap<String, String> {
    child_env_with(&[])
}

/// The explicit child environment for a provider with its own `extra` list.
pub fn child_env_with(extra: &[&str]) -> HashMap<String, String> {
    scrub_env_with(std::env::vars().collect(), extra)
}
