//! Writes a generated two-page lecture PDF for the real-app check (ticket 09).
//! Page 1 is plain text. Page 2 draws text through a half-scale Form XObject,
//! the case ingest flags. `revised` writes a second edition of page 1 with a
//! new opening line and without the pump sentence, to exercise re-anchoring.
//! Usage: demo_pdf <out.pdf> [revised]

use tutordocs_ingest::fixture::{pdf, Form, Page};

const PAGE_ONE: &str = "BT /F1 24 Tf 72 720 Td (Cell membranes) Tj \
    /F1 12 Tf 0 -36 Td (The plasma membrane separates the cell from its surroundings.) Tj \
    0 -18 Td (It is a lipid bilayer with proteins embedded across it.) Tj \
    0 -18 Td (Small nonpolar molecules diffuse through it directly.) Tj \
    0 -18 Td (Ions and large molecules need channels or carriers.) Tj \
    0 -36 Td (Active transport spends ATP to move solutes against a gradient.) Tj \
    0 -18 Td (The sodium potassium pump is the classic example.) Tj ET";

const PAGE_ONE_REVISED: &str = "BT /F1 24 Tf 72 720 Td (Cell membranes) Tj \
    /F1 12 Tf 0 -36 Td (Every cell is wrapped in a membrane.) Tj \
    0 -18 Td (The plasma membrane separates the cell from its surroundings.) Tj \
    0 -18 Td (It is a lipid bilayer with proteins embedded across it.) Tj \
    0 -36 Td (Active transport spends ATP to move solutes against a gradient.) Tj ET";

const FORM_TEXT: &str = "BT /F1 24 Tf 72 720 Td (Text drawn through a form) Tj \
    0 -36 Td (lands at the wrong place and size.) Tj ET";

const PAGE_TWO_TEXT: &str =
    "BT /F1 12 Tf 72 300 Td (This line is ordinary page text and lands correctly.) Tj ET";

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: demo_pdf <out.pdf> [revised]");
    let revised = std::env::args().nth(2).as_deref() == Some("revised");
    let forms = [Form {
        name: "Fm1",
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        content: FORM_TEXT,
    }];
    let page_two = format!("q 0.5 0 0 0.5 0 0 cm /Fm1 Do Q {PAGE_TWO_TEXT}");
    let bytes = pdf(&[
        Page::text(if revised { PAGE_ONE_REVISED } else { PAGE_ONE }),
        Page {
            content: &page_two,
            forms: &forms,
        },
    ]);
    std::fs::write(&out, bytes).expect("write the PDF");
}
