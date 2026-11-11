//! Turn a course's [`CourseSetup`] and chosen template into outline-prompt
//! instructions.

use tutordocs_store::{template_instructions_for_id, CourseSetup};

/// Format the chosen template's instructions and the learner's setup for the
/// outline prompt.
pub fn template_instructions(setup: &CourseSetup) -> String {
    let mut lines = Vec::new();
    lines.push(format!("TEMPLATE: {}", setup.template));
    lines.push(format!("LEVEL: {}", setup.level));
    if let Some(hours) = setup.hours_per_week {
        lines.push(format!("HOURS_PER_WEEK: {hours}"));
    }
    lines.push(format!("LANGUAGE: {}", setup.language));
    if let Some(body) = template_instructions_for_id(&setup.template) {
        if !body.is_empty() {
            lines.push(format!("TEMPLATE_INSTRUCTIONS:\n{body}"));
        }
    }
    let guide = setup.guide.trim();
    if !guide.is_empty() {
        lines.push(format!("USER_GUIDE:\n{guide}"));
    }
    lines.join("\n")
}
