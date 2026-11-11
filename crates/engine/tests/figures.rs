//! The figure guide's example is a static SVG the reader can show.

use roxmltree::Document;

const GUIDE: &str = include_str!("../assets/figures.md");

#[test]
fn the_guide_example_svg_parses_and_stays_static_in_both_schemes() {
    let svg = example_svg(GUIDE);
    let doc = Document::parse(svg).expect("the example svg is well-formed xml");
    let root = doc.root_element();
    assert_eq!(root.tag_name().name(), "svg");
    let view_box = root.attribute("viewBox").unwrap_or("");
    assert!(!view_box.is_empty(), "the example svg has a viewBox");

    let mut css = String::new();
    for node in doc.descendants() {
        let name = node.tag_name().name();
        assert_ne!(name, "script", "the example svg has no script");
        if name == "style" {
            for child in node.children() {
                if let Some(text) = child.text() {
                    css.push_str(text);
                }
            }
        }
        for attr in node.attributes() {
            let key = attr.name();
            if key == "href" || key.ends_with(":href") {
                assert!(
                    !is_external_href(attr.value()),
                    "external href {:?} on <{name}>",
                    attr.value()
                );
            }
        }
    }
    assert!(
        css.contains("@media (prefers-color-scheme: dark)"),
        "the example svg has a dark-scheme media block, got {css:?}"
    );
}

/// The one complete drawing in the guide. Prose does not contain an svg tag.
fn example_svg(guide: &str) -> &str {
    let start = guide.find("<svg ").expect("one example svg");
    let close = guide[start..]
        .find("</svg>")
        .expect("the example svg closes");
    &guide[start..start + close + "</svg>".len()]
}

fn is_external_href(value: &str) -> bool {
    let value = value.trim();
    !(value.is_empty() || value.starts_with('#'))
}
