//! PDF text extraction with word-level position anchors (ticket 03, Spike 3).
//!
//! Coordinates: user-space PDF points, y axis up, normalized against the page
//! MediaBox so anchors survive zoom and window resize. A rect `(x, y, w, h)`
//! is fractions of the MediaBox width/height, origin at its bottom-left corner.

/// A rectangle normalized to `[0, 1]` fractions of the page MediaBox.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A highlight anchor: page number (1-based), quoted word text, normalized rect.
#[derive(Debug, Clone, PartialEq)]
pub struct WordAnchor {
    pub page: u32,
    pub quote: String,
    pub normalized_rect: NormalizedRect,
}

/// Extracted content of one page: reading-order text plus word anchors.
#[derive(Debug, Clone, PartialEq)]
pub struct PageText {
    pub page: u32,
    /// MediaBox width in PDF points.
    pub width: f64,
    /// MediaBox height in PDF points.
    pub height: f64,
    pub text: String,
    pub words: Vec<WordAnchor>,
    /// Why some anchors on this page may not match the original page.
    pub caveat: Option<PageCaveat>,
}

/// A known way this page's anchors can be wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageCaveat {
    /// Text inside a Form XObject drawn under a non-identity transform.
    /// pdf-extract 0.12.1 restarts the CTM at identity inside every form and
    /// never applies the form's `/Matrix`, so that text lands at the wrong
    /// place and size. An `OutputDev` never sees `Do`, so only the
    /// `pdfium-render` fallback can place it.
    TransformedFormText,
}

/// Extraction failure. Kept dependency-free at the API boundary.
#[derive(Debug)]
pub enum IngestError {
    Pdf(String),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngestError::Pdf(msg) => write!(f, "PDF extraction failed: {}", msg),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<pdf_extract::Error> for IngestError {
    fn from(e: pdf_extract::Error) -> Self {
        IngestError::Pdf(e.to_string())
    }
}

impl From<pdf_extract::OutputError> for IngestError {
    fn from(e: pdf_extract::OutputError) -> Self {
        IngestError::Pdf(e.to_string())
    }
}

/// Extracts per-page text and word anchors from PDF bytes. pdf-extract panics
/// on some real PDFs (missing widths, missing unicode maps) instead of
/// returning an error, so panics are converted at this boundary.
pub fn extract_pdf(bytes: &[u8]) -> Result<Vec<PageText>, IngestError> {
    std::panic::catch_unwind(|| extract_pdf_unguarded(bytes)).unwrap_or_else(|payload| {
        let reason = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        Err(IngestError::Pdf(format!("extractor panicked: {reason}")))
    })
}

fn extract_pdf_unguarded(bytes: &[u8]) -> Result<Vec<PageText>, IngestError> {
    let mut doc = pdf_extract::Document::load_mem(bytes)?;
    normalize_font_widths(&mut doc);
    let mut dev = AnchorDev::new();
    pdf_extract::output_doc(&doc, &mut dev)?;
    let flagged = pages_with_transformed_form_text(&doc);
    for page in &mut dev.pages {
        if flagged.contains(&page.page) {
            page.caveat = Some(PageCaveat::TransformedFormText);
        }
    }
    Ok(dev.pages)
}

/// Nesting depth past which forms are not followed, so a form that draws
/// itself cannot recurse forever.
const MAX_FORM_DEPTH: usize = 8;

type Matrix = [f64; 6];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a` then `b`, in PDF's row-vector convention (`cm` sets CTM = M x CTM).
fn concat(a: &Matrix, b: &Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn is_identity(m: &Matrix) -> bool {
    m.iter().zip(IDENTITY).all(|(v, i)| (v - i).abs() < 1e-9)
}

/// Pages that draw text through a Form XObject under a non-identity
/// transform, which pdf-extract places wrong (see [`PageCaveat`]).
fn pages_with_transformed_form_text(doc: &pdf_extract::Document) -> Vec<u32> {
    doc.get_pages()
        .into_iter()
        .filter(|(_, id)| {
            let Ok(content) = doc.get_page_content(*id) else {
                return false;
            };
            let Ok((inline, inherited)) = doc.get_page_resources(*id) else {
                return false;
            };
            let resources: Vec<&pdf_extract::Dictionary> = inline
                .into_iter()
                .chain(inherited.iter().filter_map(|r| doc.get_dictionary(*r).ok()))
                .collect();
            misplaces_form_text(doc, &content, &resources, 0)
        })
        .map(|(num, _)| num)
        .collect()
}

/// The Form XObject named `name` in the first resource dictionary that has it.
fn form<'a>(
    doc: &'a pdf_extract::Document,
    resources: &[&'a pdf_extract::Dictionary],
    name: &[u8],
) -> Option<&'a pdf_extract::Stream> {
    resources.iter().find_map(|r| {
        let xobjects = doc
            .dereference(r.get(b"XObject").ok()?)
            .ok()?
            .1
            .as_dict()
            .ok()?;
        let stream = doc
            .dereference(xobjects.get(name).ok()?)
            .ok()?
            .1
            .as_stream()
            .ok()?;
        (stream.dict.get(b"Subtype").ok()?.as_name().ok()? == b"Form").then_some(stream)
    })
}

/// A form's content, its `/Matrix`, and the resources its own operators see
/// (its `/Resources`, else the caller's, as older producers rely on).
fn open_form<'a>(
    doc: &'a pdf_extract::Document,
    stream: &'a pdf_extract::Stream,
    parent: &[&'a pdf_extract::Dictionary],
) -> Option<(Vec<u8>, Matrix, Vec<&'a pdf_extract::Dictionary>)> {
    let content = stream.decompressed_content().ok()?;
    let matrix = stream
        .dict
        .get(b"Matrix")
        .ok()
        .and_then(|m| pdf_numbers(doc, m))
        .and_then(|m| <Matrix>::try_from(m).ok())
        .unwrap_or(IDENTITY);
    let own = stream
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|r| doc.dereference(r).ok()?.1.as_dict().ok());
    let resources = own.map_or_else(|| parent.to_vec(), |r| vec![r]);
    Some((content, matrix, resources))
}

/// Calls `visit` with each Form XObject that `content` draws and the
/// transform in force at its `Do`, relative to the start of `content`.
fn each_form<'a>(
    doc: &'a pdf_extract::Document,
    content: &[u8],
    resources: &[&'a pdf_extract::Dictionary],
    mut visit: impl FnMut(&'a pdf_extract::Stream, Matrix) -> bool,
) -> bool {
    let Ok(ops) = pdf_extract::content::Content::decode(content) else {
        return false;
    };
    let (mut ctm, mut saved) = (IDENTITY, Vec::new());
    for op in &ops.operations {
        match op.operator.as_str() {
            "q" => saved.push(ctm),
            "Q" => ctm = saved.pop().unwrap_or(IDENTITY),
            "cm" => {
                let m: Option<Vec<f64>> = op.operands.iter().map(|o| pdf_number(doc, o)).collect();
                if let Some(m) = m.and_then(|m| <Matrix>::try_from(m).ok()) {
                    ctm = concat(&m, &ctm);
                }
            }
            "Do" => {
                let name = op.operands.first().and_then(|n| n.as_name().ok());
                if let Some(stream) = name.and_then(|n| form(doc, resources, n)) {
                    if visit(stream, ctm) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Whether `content` shows any text, directly or through the forms it draws.
fn shows_text(
    doc: &pdf_extract::Document,
    content: &[u8],
    resources: &[&pdf_extract::Dictionary],
    depth: usize,
) -> bool {
    if depth > MAX_FORM_DEPTH {
        return false;
    }
    let direct = pdf_extract::content::Content::decode(content).is_ok_and(|ops| {
        ops.operations
            .iter()
            .any(|op| matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\""))
    });
    direct
        || each_form(doc, content, resources, |stream, _| {
            open_form(doc, stream, resources)
                .is_some_and(|(c, _, r)| shows_text(doc, &c, &r, depth + 1))
        })
}

/// Whether `content` draws text through a form whose `/Matrix` and the
/// transform at its `Do` do not cancel to identity. pdf-extract processes
/// every form from an identity CTM, so the transform is relative to the
/// enclosing stream, and nested forms are checked the same way.
fn misplaces_form_text(
    doc: &pdf_extract::Document,
    content: &[u8],
    resources: &[&pdf_extract::Dictionary],
    depth: usize,
) -> bool {
    if depth > MAX_FORM_DEPTH {
        return false;
    }
    each_form(doc, content, resources, |stream, ctm| {
        let Some((inner, matrix, inner_resources)) = open_form(doc, stream, resources) else {
            return false;
        };
        let moved = !is_identity(&concat(&matrix, &ctm));
        (moved && shows_text(doc, &inner, &inner_resources, depth + 1))
            || misplaces_form_text(doc, &inner, &inner_resources, depth + 1)
    })
}

fn pdf_number(doc: &pdf_extract::Document, o: &pdf_extract::Object) -> Option<f64> {
    match doc.dereference(o).ok()?.1 {
        pdf_extract::Object::Integer(i) => Some(*i as f64),
        pdf_extract::Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

fn pdf_numbers(doc: &pdf_extract::Document, o: &pdf_extract::Object) -> Option<Vec<f64>> {
    let array = doc.dereference(o).ok()?.1.as_array().ok()?;
    array.iter().map(|n| pdf_number(doc, n)).collect()
}

/// `/Widths` of a Type3 font rewritten to the 1/1000 unit pdf-extract
/// assumes, or `None` when the font is not Type3 or already uses that unit.
/// Type3 widths are glyph-space and reach text space through `FontMatrix`, but
/// pdf-extract 0.12.1 divides every width by 1000 and never reads the matrix
/// (`PdfType3Font::get_width`, `show_text`). Slide exports that use
/// `FontMatrix [1 0 0 1 0 0]` therefore advance 1000x too little.
fn rescaled_type3_widths(
    doc: &pdf_extract::Document,
    font: &pdf_extract::Dictionary,
) -> Option<Vec<pdf_extract::Object>> {
    if font.get(b"Subtype").ok()?.as_name().ok()? != b"Type3" {
        return None;
    }
    let matrix = pdf_numbers(doc, font.get(b"FontMatrix").ok()?)?;
    let scale = matrix.first()?.hypot(*matrix.get(1)?) * 1000.0;
    if scale <= 0.0 || (scale - 1.0).abs() < 1e-6 {
        return None;
    }
    let widths = pdf_numbers(doc, font.get(b"Widths").ok()?)?;
    Some(
        widths
            .iter()
            .map(|w| pdf_extract::Object::Real((w * scale) as f32))
            .collect(),
    )
}

/// `/W` of a CID font with every `first last width` range expanded to the
/// `first [width ...]` form, or `None` when it has no range entries.
/// pdf-extract 0.12.1 reads the range form with the wrong operands
/// (`PdfCIDFont::new`), so those CIDs fall back to `/DW`, which is 0 in
/// Cairo-made PDFs: monospace text then advances by nothing.
fn expanded_cid_widths(
    doc: &pdf_extract::Document,
    font: &pdf_extract::Dictionary,
) -> Option<Vec<pdf_extract::Object>> {
    use pdf_extract::Object;
    let subtype = font.get(b"Subtype").ok()?.as_name().ok()?;
    if subtype != b"CIDFontType0" && subtype != b"CIDFontType2" {
        return None;
    }
    let entries = doc
        .dereference(font.get(b"W").ok()?)
        .ok()?
        .1
        .as_array()
        .ok()?;
    let (mut out, mut has_range, mut i) = (Vec::new(), false, 0);
    while i < entries.len() {
        let first = pdf_number(doc, &entries[i])? as i64;
        let next = doc.dereference(entries.get(i + 1)?).ok()?.1;
        out.push(Object::Integer(first));
        if let Ok(list) = next.as_array() {
            out.push(Object::Array(list.clone()));
            i += 2;
        } else {
            let last = pdf_number(doc, next)? as i64;
            let width = pdf_number(doc, entries.get(i + 2)?)? as f32;
            if last < first || last - first >= 0x10000 {
                return None;
            }
            out.push(Object::Array(vec![
                Object::Real(width);
                (last - first + 1) as usize
            ]));
            has_range = true;
            i += 3;
        }
    }
    has_range.then_some(out)
}

/// Rewrites font width tables in memory, never in the source bytes, so that
/// pdf-extract's advance arithmetic matches the PDF spec. The new array is
/// set inline so one shared `/Widths` object used by fonts with different
/// matrices is not scaled twice.
fn normalize_font_widths(doc: &mut pdf_extract::Document) {
    let updates: Vec<_> = doc
        .objects
        .iter()
        .filter_map(|(id, obj)| {
            let font = obj.as_dict().ok()?;
            let fix = rescaled_type3_widths(doc, font)
                .map(|widths| ("Widths", widths))
                .or_else(|| expanded_cid_widths(doc, font).map(|w| ("W", w)))?;
            Some((*id, fix))
        })
        .collect();
    for (id, (key, widths)) in updates {
        if let Ok(pdf_extract::Object::Dictionary(font)) = doc.get_object_mut(id) {
            font.set(key, pdf_extract::Object::Array(widths));
        }
    }
}

/// OutputDev passes no font ascent/descent, so 1.2 em approximates the full
/// glyph box height; good enough for highlight rects, not glyph outlines.
pub const GLYPH_HEIGHT_FACTOR: f64 = 1.2;
/// Baseline jump (in em) that means a new line; same hint as pdf-extract's
/// own PlainTextOutput.
const LINE_JUMP_FACTOR: f64 = 1.5;
/// Leftward move plus this much baseline change (em) means a new line.
const LINE_RETURN_FACTOR: f64 = 0.5;
/// Horizontal gap (em) that means a word boundary when no space glyph exists.
const WORD_GAP_FACTOR: f64 = 0.2;

/// Collects per-glyph positions into word anchors. Word and line breaks come
/// from glyph geometry and whitespace glyphs only. pdf-extract fires
/// `begin_word` per string operand and `end_line` on every `Tm`, `Td`, `TD`
/// and `T*`, so those hooks mark repositioning, not word or line ends. Treating
/// them as boundaries split Thai combining marks (zero advance, each placed by
/// its own positioning operator) into standalone zero-width words.
struct AnchorDev {
    pages: Vec<PageText>,
    page_num: u32,
    media_box: (f64, f64, f64, f64),
    text: String,
    words: Vec<WordAnchor>,
    word_quote: String,
    word_box: Option<(f64, f64, f64, f64)>,
    /// Pen end x and baseline y of the previous glyph on this page.
    last: Option<(f64, f64)>,
}

impl AnchorDev {
    fn new() -> Self {
        AnchorDev {
            pages: Vec::new(),
            page_num: 0,
            media_box: (0.0, 0.0, 0.0, 0.0),
            text: String::new(),
            words: Vec::new(),
            word_quote: String::new(),
            word_box: None,
            last: None,
        }
    }

    fn flush_word(&mut self) {
        if let (false, Some(b)) = (self.word_quote.is_empty(), self.word_box) {
            self.words.push(WordAnchor {
                page: self.page_num,
                quote: std::mem::take(&mut self.word_quote),
                normalized_rect: normalize_rect(b, self.media_box),
            });
        }
        self.word_quote.clear();
        self.word_box = None;
    }

    fn push_space(&mut self) {
        if !self.text.is_empty() && !self.text.ends_with([' ', '\n']) {
            self.text.push(' ');
        }
    }

    fn push_newline(&mut self) {
        let trimmed = self.text.trim_end_matches(' ').len();
        self.text.truncate(trimmed);
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
    }

    fn union_word_box(&mut self, b: (f64, f64, f64, f64)) {
        self.word_box = Some(match self.word_box {
            None => b,
            Some((x0, y0, x1, y1)) => (x0.min(b.0), y0.min(b.1), x1.max(b.2), y1.max(b.3)),
        });
    }
}

/// Axis-aligned bounding box of one glyph: corners in text space span
/// x 0..width*font_size and y -0.2*font_size..1.0*font_size (the 1.2 em
/// approximation split at the baseline), mapped through the trm.
fn glyph_user_box(
    trm: &pdf_extract::Transform,
    width: f64,
    font_size: f64,
) -> (f64, f64, f64, f64) {
    let descent = (GLYPH_HEIGHT_FACTOR - 1.0) * font_size;
    let corners = [
        (0.0, -descent),
        (width * font_size, -descent),
        (width * font_size, font_size),
        (0.0, font_size),
    ];
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (tx, ty) in corners {
        let x = trm.m11 * tx + trm.m21 * ty + trm.m31;
        let y = trm.m12 * tx + trm.m22 * ty + trm.m32;
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    }
    b
}

impl pdf_extract::OutputDev for AnchorDev {
    fn begin_page(
        &mut self,
        page_num: u32,
        media_box: &pdf_extract::MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), pdf_extract::OutputError> {
        self.page_num = page_num;
        self.media_box = (media_box.llx, media_box.lly, media_box.urx, media_box.ury);
        self.text.clear();
        self.words.clear();
        self.word_quote.clear();
        self.word_box = None;
        self.last = None;
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.flush_word();
        self.pages.push(PageText {
            page: self.page_num,
            width: self.media_box.2 - self.media_box.0,
            height: self.media_box.3 - self.media_box.1,
            text: std::mem::take(&mut self.text),
            words: std::mem::take(&mut self.words),
            caveat: None,
        });
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &pdf_extract::Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        ch: &str,
    ) -> Result<(), pdf_extract::OutputError> {
        if ch.trim().is_empty() {
            self.flush_word();
            self.push_space();
            return Ok(());
        }
        // trm excludes font size (pdf-extract keeps it in the font_size
        // parameter), so scales come from the matrix columns.
        let sx = trm.m11.hypot(trm.m12);
        let sy = trm.m21.hypot(trm.m22);
        let fs_y = font_size * sy;
        let (x, y) = (trm.m31, trm.m32);
        if let Some((last_end_x, last_y)) = self.last {
            if fs_y > 0.0 {
                let dy = (y - last_y).abs();
                if dy > LINE_JUMP_FACTOR * fs_y
                    || (x < last_end_x && dy > LINE_RETURN_FACTOR * fs_y)
                {
                    self.flush_word();
                    self.push_newline();
                } else if x > last_end_x + WORD_GAP_FACTOR * fs_y {
                    self.flush_word();
                    self.push_space();
                }
            }
        }
        self.union_word_box(glyph_user_box(trm, width, font_size));
        self.word_quote.push_str(ch);
        self.text.push_str(ch);
        self.last = Some((x + width * font_size * sx, y));
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
}

/// Normalizes a user-space rect `(x0, y0, x1, y1)` against a MediaBox
/// `(llx, lly, urx, ury)`. Degenerate MediaBoxes yield a zero rect.
pub fn normalize_rect(
    rect: (f64, f64, f64, f64),
    media_box: (f64, f64, f64, f64),
) -> NormalizedRect {
    let (x0, y0, x1, y1) = rect;
    let (llx, lly, urx, ury) = media_box;
    let page_w = urx - llx;
    let page_h = ury - lly;
    if page_w <= 0.0 || page_h <= 0.0 {
        return NormalizedRect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
    }
    NormalizedRect {
        x: (x0 - llx) / page_w,
        y: (y0 - lly) / page_h,
        w: (x1 - x0) / page_w,
        h: (y1 - y0) / page_h,
    }
}

#[cfg(test)]
mod tests;
