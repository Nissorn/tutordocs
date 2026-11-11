//! Self-generated PDFs for tests and the real-app check. US Letter pages
//! (612 x 792 pt) with base-14 Helvetica as `F1`, so every glyph advance is a
//! standard AFM width and every expected rect can be computed by hand.

/// A Form XObject a page can draw with `/<name> Do`.
pub struct Form<'a> {
    pub name: &'a str,
    /// The form's `/Matrix`, `[a b c d e f]`.
    pub matrix: [f64; 6],
    pub content: &'a str,
}

/// One page: its content stream and the forms it may draw.
pub struct Page<'a> {
    pub content: &'a str,
    pub forms: &'a [Form<'a>],
}

impl<'a> Page<'a> {
    pub fn text(content: &'a str) -> Self {
        Page {
            content,
            forms: &[],
        }
    }
}

const FONT_ID: usize = 3;

fn stream(dict: &str, content: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// A PDF of `pages`, objects numbered catalog 1, page tree 2, font 3, then
/// each page followed by its content stream and its forms.
pub fn pdf(pages: &[Page]) -> Vec<u8> {
    let mut objects = vec![
        String::new(),
        String::new(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut kids = Vec::new();
    for page in pages {
        let page_id = objects.len() + 1;
        let content_id = page_id + 1;
        let xobjects: String = page
            .forms
            .iter()
            .enumerate()
            .map(|(i, form)| format!("/{} {} 0 R ", form.name, content_id + 1 + i))
            .collect();
        kids.push(format!("{page_id} 0 R"));
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 {FONT_ID} 0 R >> /XObject << {xobjects}>> >> \
             /Contents {content_id} 0 R >>"
        ));
        objects.push(stream("", page.content));
        for form in page.forms {
            let m = form.matrix.map(|v| v.to_string()).join(" ");
            objects.push(stream(
                &format!(
                    "/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Matrix [{m}] \
                     /Resources << /Font << /F1 {FONT_ID} 0 R >> >>"
                ),
                form.content,
            ));
        }
    }
    objects[0] = "<< /Type /Catalog /Pages 2 0 R >>".to_string();
    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        pages.len()
    );
    serialize(&objects)
}

/// Writes `objects` as a PDF numbered from 1, with object 1 as the catalog.
pub fn serialize(objects: &[String]) -> Vec<u8> {
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, obj));
    }
    let xref_pos = out.len();
    out.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
    out.push_str("0000000000 65535 f \n");
    for off in offsets {
        out.push_str(&format!("{:010} 00000 n \n", off));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        xref_pos
    ));
    out.into_bytes()
}
