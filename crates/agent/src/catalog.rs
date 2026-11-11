//! A driver's model and effort choices, parsed from the text its CLI prints.
//! Nothing here is a fixed list: a help text with no list yields an empty one.

use serde::Serialize;

use crate::agy::parse_models;
use crate::types::{Effort, ModelId};

/// One model the user can pick. A null id is the CLI's own default. Empty
/// `efforts` means the CLI did not say which levels this model accepts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CatalogModel {
    pub id: Option<String>,
    pub label: String,
    pub efforts: Vec<Effort>,
}

/// What one driver reported. `efforts` is the driver-wide list from `--help`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriverCatalog {
    pub models: Vec<CatalogModel>,
    pub efforts: Vec<Effort>,
    pub accepts_custom_model: bool,
}

/// Build the catalog for `id` from its `--help` text and, for agy, `agy models`.
///
/// Claude's help names aliases and one effort list, and no per-model efforts,
/// so every Claude model carries that same list. A typed full name is accepted
/// only for Claude, whose help says a model may be given by its full name.
/// Qoder's `--reasoning-effort` is read when `--effort` names no list.
pub fn driver_catalog(id: &str, help: &str, agy_models: Option<&str>) -> DriverCatalog {
    let mut efforts = flag_levels(help, "--effort");
    if efforts.is_empty() {
        efforts = flag_levels(help, "--reasoning-effort");
    }
    let accepts_custom_model = id == "claude";
    let mut models = vec![CatalogModel {
        id: None,
        label: "CLI default".to_string(),
        efforts: efforts.clone(),
    }];
    if id == "agy" {
        for listed in parse_models(agy_models.unwrap_or(""), &efforts) {
            models.push(CatalogModel {
                id: Some(listed.id),
                label: listed.label,
                efforts: listed.efforts,
            });
        }
    } else {
        for alias in model_aliases(help) {
            models.push(CatalogModel {
                id: Some(alias.clone()),
                label: alias,
                efforts: efforts.clone(),
            });
        }
    }
    DriverCatalog {
        models,
        efforts,
        accepts_custom_model,
    }
}

/// Levels named in parentheses on `flag`'s help block, in the text's order.
/// A block with no such list, or no such flag, is empty.
fn flag_levels(help: &str, flag: &str) -> Vec<Effort> {
    let Some(block) = option_block(help, flag) else {
        return Vec::new();
    };
    let mut rest = block.as_str();
    while let Some(start) = rest.find('(') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find(')') else {
            break;
        };
        let inside = &rest[..end];
        rest = &rest[end + 1..];
        if let Some(levels) = level_group(inside) {
            return levels;
        }
    }
    Vec::new()
}

fn level_group(inside: &str) -> Option<Vec<Effort>> {
    let parts: Vec<&str> = inside
        .split(['|', ','])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let mut levels = Vec::with_capacity(parts.len());
    for part in parts {
        let word = part.strip_prefix("or ").unwrap_or(part).trim();
        levels.push(Effort::parse(word).ok()?);
    }
    Some(levels)
}

/// Quoted model ids in the `--model` help block, in the order printed.
fn model_aliases(help: &str) -> Vec<String> {
    let Some(block) = option_block(help, "--model") else {
        return Vec::new();
    };
    let mut aliases = Vec::new();
    let mut rest = block.as_str();
    while let Some(start) = rest.find(['\'', '"']) {
        let quote = rest[start..].chars().next().unwrap_or('\'');
        let after = &rest[start + quote.len_utf8()..];
        let Some(end) = after.find(quote) else {
            break;
        };
        let word = &after[..end];
        if ModelId::parse(word).is_ok() && !aliases.iter().any(|have| have == word) {
            aliases.push(word.to_string());
        }
        rest = &after[end + quote.len_utf8()..];
    }
    aliases
}

fn option_block(help: &str, flag: &str) -> Option<String> {
    let lines: Vec<&str> = help.lines().collect();
    let start = lines.iter().position(|line| is_option_line(line, flag))?;
    let mut block = vec![lines[start].to_string()];
    for line in &lines[start + 1..] {
        if is_any_option_line(line) {
            break;
        }
        block.push((*line).to_string());
    }
    Some(block.join("\n"))
}

fn is_option_line(line: &str, flag: &str) -> bool {
    let line = line.trim_start();
    line.starts_with('-') && option_tokens(line).any(|token| token == flag)
}

fn is_any_option_line(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with('-') && option_tokens(line).next().is_some()
}

fn option_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split([',', ' ', '='])
        .take_while(|token| token.is_empty() || token.starts_with('-'))
        .filter(|token| token.starts_with('-'))
}
