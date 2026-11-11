//! Public-seam tests for `template_instructions(setup: &CourseSetup) -> String`.

use tutordocs_engine::template_instructions;
use tutordocs_store::{list_course_templates, template_instructions_for_id, CourseSetup};

#[test]
fn each_builtin_template_reaches_the_prompt_instructions_and_custom_uses_only_the_guide() {
    for template in list_course_templates() {
        let setup = CourseSetup {
            template: template.id.clone(),
            level: "some".to_string(),
            hours_per_week: Some(8),
            language: "en".to_string(),
            guide: "Include a case study on Raft.".to_string(),
        };
        let prompt_block = template_instructions(&setup);
        let body = template_instructions_for_id(&template.id).unwrap();

        if template.id == "custom" {
            assert!(
                body.is_empty(),
                "custom template has no preset instructions"
            );
            for other in ["interview", "exam", "overview"] {
                let other_body = template_instructions_for_id(other).unwrap();
                assert!(
                    !prompt_block.contains(other_body),
                    "custom must not include {other} instructions"
                );
            }
        } else {
            assert!(!body.is_empty(), "{} must have instructions", template.id);
            assert!(
                prompt_block.contains(body),
                "{} instructions missing from prompt block: {prompt_block}",
                template.id
            );
        }

        assert!(
            prompt_block.contains("Include a case study on Raft."),
            "user guide must appear in prompt block: {prompt_block}"
        );
        assert!(prompt_block.contains("some"), "{prompt_block}");
        assert!(prompt_block.contains("8"), "{prompt_block}");
        assert!(prompt_block.contains("en"), "{prompt_block}");
    }
}
