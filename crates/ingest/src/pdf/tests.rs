//! Tests for the pdf module. Expected rects are hand-computed from fixture
//! geometry (known positions, base-14 Helvetica AFM widths, MediaBox), never
//! from rerunning the extractor.

use super::*;

const EPS: f64 = 1e-12;
/// Content-stream real operands parse as f32, so a decimal like 87.336 lands
/// about 5e-6 pt off; that is 1e-8 once normalized.
const F32_OPERAND_EPS: f64 = 1e-6;

fn assert_rect_close(got: NormalizedRect, x: f64, y: f64, w: f64, h: f64) {
    assert_rect_within(got, x, y, w, h, EPS);
}

fn assert_rect_within(got: NormalizedRect, x: f64, y: f64, w: f64, h: f64, eps: f64) {
    assert!((got.x - x).abs() < eps, "x: got {} want {}", got.x, x);
    assert!((got.y - y).abs() < eps, "y: got {} want {}", got.y, y);
    assert!((got.w - w).abs() < eps, "w: got {} want {}", got.w, w);
    assert!((got.h - h).abs() < eps, "h: got {} want {}", got.h, h);
}

#[test]
fn normalize_rect_letter_page_hand_computed() {
    // Fixture geometry: US Letter MediaBox 612 x 792 pt. A 24 pt word whose
    // glyph box spans x 72..126.672 and y 715.2..744 (baseline 720, descent
    // 0.2 em = 4.8 pt, ascent 1.0 em = 24 pt).
    //   x = 72 / 612      = 0.117647058823529
    //   y = 715.2 / 792   = 0.903030303030303
    //   w = 54.672 / 612  = 0.089333333333333
    //   h = 28.8 / 792    = 0.036363636363636
    let got = normalize_rect((72.0, 715.2, 126.672, 744.0), (0.0, 0.0, 612.0, 792.0));
    assert_rect_close(
        got,
        0.117647058823529,
        0.903030303030303,
        0.089333333333333,
        0.036363636363636,
    );
}

#[test]
fn normalize_rect_nonzero_origin_media_box_hand_computed() {
    // MediaBox (10, 20, 310, 420): width 300, height 400, origin (10, 20).
    // Rect (40, 118, 65, 130):
    //   x = (40 - 10) / 300 = 0.1
    //   y = (118 - 20) / 400 = 0.245
    //   w = 25 / 300        = 0.083333333333333
    //   h = 12 / 400        = 0.03
    let got = normalize_rect((40.0, 118.0, 65.0, 130.0), (10.0, 20.0, 310.0, 420.0));
    assert_rect_close(got, 0.1, 0.245, 0.083333333333333, 0.03);
}

/// Builds a minimal one-page PDF around `content` (ASCII only). MediaBox is
/// US Letter 612 x 792, font F1 is base-14 Helvetica, so glyph advances are
/// the standard AFM widths and every expected rect can be hand-computed.
fn fixture_pdf(content: &str) -> Vec<u8> {
    fixture_pdf_with_font(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        content,
    )
}

/// Same one-page fixture with a caller-supplied font dictionary for F1.
fn fixture_pdf_with_font(font_dict: &str, content: &str) -> Vec<u8> {
    fixture_pdf_with_objects(font_dict, &[], content)
}

/// Fixture with extra indirect objects numbered from 6, for fonts that
/// reference further objects (Type3 glyph procedures).
fn fixture_pdf_with_objects(font_dict: &str, extra: &[&str], content: &str) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            .to_string(),
        font_dict.to_string(),
        format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            content
        ),
    ];
    objects.extend(extra.iter().map(|o| o.to_string()));
    crate::fixture::serialize(&objects)
}

#[test]
fn fixture_pdf_yields_text_and_hand_computed_word_rects() {
    let bytes = fixture_pdf("BT /F1 24 Tf 72 720 Td (Hello world) Tj ET");
    let pages = extract_pdf(&bytes).expect("fixture parses");
    assert_eq!(pages.len(), 1);
    let page = &pages[0];
    assert_eq!(page.page, 1);
    assert_eq!(page.text.trim(), "Hello world");
    assert_eq!(page.words.len(), 2);

    // Hand computation from fixture geometry + base-14 Helvetica AFM widths
    // (units/1000): H 722, e 556, l 222, o 556, space 278, w 722, r 333, d 556.
    // At 24 pt, text origin (72, 720):
    //   Hello advance = 2278/1000*24 = 54.672 pt  -> x 72..126.672
    //   space advance =  278/1000*24 =  6.672 pt
    //   world advance = 2389/1000*24 = 57.336 pt  -> x 133.344..190.68
    //   glyph box y: baseline 720, descent 0.2*24 = 4.8, ascent 1.0*24 = 24
    //                -> y 715.2..744, h 28.8
    // Normalized by MediaBox 612 x 792:
    //   Hello: x 72/612, y 715.2/792, w 54.672/612, h 28.8/792
    //   world: x 133.344/612, w 57.336/612, same y and h
    assert_eq!(page.words[0].page, 1);
    assert_eq!(page.words[0].quote, "Hello");
    assert_rect_close(
        page.words[0].normalized_rect,
        0.117647058823529,
        0.903030303030303,
        0.089333333333333,
        0.036363636363636,
    );
    assert_eq!(page.words[1].quote, "world");
    assert_rect_close(
        page.words[1].normalized_rect,
        0.217882352941176,
        0.903030303030303,
        0.093686274509804,
        0.036363636363636,
    );
}

#[test]
fn fixture_pdf_two_lines_flush_words_and_newline() {
    let bytes = fixture_pdf("BT /F1 12 Tf 72 720 Td (First line) Tj 0 -24 Td (Second) Tj ET");
    let pages = extract_pdf(&bytes).expect("fixture parses");
    assert_eq!(pages.len(), 1);
    let page = &pages[0];
    assert_eq!(page.text.trim(), "First line\nSecond");
    let quotes: Vec<&str> = page.words.iter().map(|w| w.quote.as_str()).collect();
    assert_eq!(quotes, ["First", "line", "Second"]);

    // "Second": Helvetica AFM S 667, e 556, c 500, o 556, n 556, d 556
    //   advance = 3391/1000*12 = 40.692 pt, x0 = 72, baseline = 720-24 = 696
    //   box y 696-2.4 .. 696+12 = 693.6..708, h 14.4
    // Normalized: x 72/612, y 693.6/792, w 40.692/612, h 14.4/792
    let second = &page.words[2];
    assert_rect_close(
        second.normalized_rect,
        0.117647058823529,
        0.875757575757576,
        0.066490196078431,
        0.018181818181818,
    );
}
fn word_quotes(page: &PageText) -> Vec<&str> {
    page.words.iter().map(|w| w.quote.as_str()).collect()
}

#[test]
fn runs_placed_end_to_end_by_separate_tm_operators_stay_one_word() {
    // pdf-extract fires end_line on every Tm/Td/TD/T*, so a word split across
    // positioning operators must not be cut there. "He" ends at
    // 72 + (722+556)/1000*12 = 87.336, where the second Tm puts "llo".
    //   Hello advance = 2278/1000*12 = 27.336 pt -> x 72..99.336
    //   box y: baseline 720, 720-2.4 .. 720+12 = 717.6..732, h 14.4
    let bytes =
        fixture_pdf("BT /F1 12 Tf 1 0 0 1 72 720 Tm (He) Tj 1 0 0 1 87.336 720 Tm (llo) Tj ET");
    let page = &extract_pdf(&bytes).expect("fixture parses")[0];
    assert_eq!(word_quotes(page), ["Hello"]);
    assert_eq!(page.text.trim(), "Hello");
    assert_rect_within(
        page.words[0].normalized_rect,
        72.0 / 612.0,
        717.6 / 792.0,
        27.336 / 612.0,
        14.4 / 792.0,
        F32_OPERAND_EPS,
    );
}

#[test]
fn zero_width_glyph_placed_by_its_own_tm_stays_inside_its_word() {
    // Stand-in for a Thai combining mark: code B has width 0 and sits at the
    // end of the previous glyph, positioned by its own Tm like the marks in
    // the real Thai PDF. A has width 600 (units/1000).
    //   A at x=100 advances 6 pt -> 106; B at 106 advances 0; A again at 106
    //   advances 6 -> 112. One word "ABA", x 100..112, 10 pt font, baseline
    //   500: box y 498..510, h 12.
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                /FirstChar 65 /LastChar 66 /Widths [600 0] >>";
    let bytes = fixture_pdf_with_font(
        font,
        "BT /F1 10 Tf 1 0 0 1 100 500 Tm (A) Tj 1 0 0 1 106 500 Tm (B) Tj \
         1 0 0 1 106 500 Tm (A) Tj ET",
    );
    let page = &extract_pdf(&bytes).expect("fixture parses")[0];
    assert_eq!(word_quotes(page), ["ABA"]);
    assert_rect_close(
        page.words[0].normalized_rect,
        100.0 / 612.0,
        498.0 / 792.0,
        12.0 / 612.0,
        12.0 / 792.0,
    );
}

#[test]
fn run_repositioned_after_a_visible_gap_is_a_new_word() {
    // "Hello" ends at 99.336; the next Tm starts at 105.336, a 6 pt gap
    // (0.5 em at 12 pt, over the 0.2 em word gap) with no space glyph.
    let bytes = fixture_pdf(
        "BT /F1 12 Tf 1 0 0 1 72 720 Tm (Hello) Tj 1 0 0 1 105.336 720 Tm (world) Tj ET",
    );
    let page = &extract_pdf(&bytes).expect("fixture parses")[0];
    assert_eq!(word_quotes(page), ["Hello", "world"]);
    assert_eq!(page.text.trim(), "Hello world");
}

#[test]
fn tj_array_kerning_gap_between_strings_separates_words() {
    // TJ adjustment -333 moves the pen right by 333/1000*12 = 3.996 pt
    // (0.333 em, over the 0.2 em word gap); that is how typeset PDFs spell a
    // word space without a space glyph.
    let bytes = fixture_pdf("BT /F1 12 Tf 72 720 Td [(Hello) -333 (world)] TJ ET");
    let page = &extract_pdf(&bytes).expect("fixture parses")[0];
    assert_eq!(word_quotes(page), ["Hello", "world"]);
}

#[test]
fn tj_array_tight_kerning_keeps_one_word() {
    // TJ adjustment 40 pulls the pen left 0.48 pt (0.04 em): kerning, not a gap.
    let bytes = fixture_pdf("BT /F1 12 Tf 72 720 Td [(Hel) 40 (lo)] TJ ET");
    let page = &extract_pdf(&bytes).expect("fixture parses")[0];
    assert_eq!(word_quotes(page), ["Hello"]);
}

#[test]
fn tight_leading_second_line_still_breaks_by_geometry() {
    // 12 pt text with 14 pt leading (1.17 em, under the 1.5 em jump) returns
    // to the left margin, so the left-return rule must still end the line.
    let bytes = fixture_pdf("BT /F1 12 Tf 72 720 Td (First line) Tj 0 -14 Td (Second) Tj ET");
    let page = &extract_pdf(&bytes).expect("fixture parses")[0];
    assert_eq!(page.text.trim(), "First line\nSecond");
    assert_eq!(word_quotes(page), ["First", "line", "Second"]);
}

/// One-glyph Type3 font (code 65 = A) whose `Widths` entry is in glyph space,
/// so the advance in text space is width * FontMatrix.a.
fn type3_fixture(font_matrix: &str, width: &str) -> Vec<u8> {
    type3_fixture_showing(
        font_matrix,
        width,
        "BT /F1 10 Tf 1 0 0 1 72 720 Tm (AAA) Tj 1 0 0 1 100 720 Tm (AA) Tj ET",
    )
}

fn type3_fixture_showing(font_matrix: &str, width: &str, content: &str) -> Vec<u8> {
    let font = format!(
        "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix {font_matrix} \
         /CharProcs << /A 6 0 R >> \
         /Encoding << /Type /Encoding /Differences [65 /A] >> \
         /FirstChar 65 /LastChar 65 /Widths [{width}] >>"
    );
    fixture_pdf_with_objects(&font, &["<< /Length 0 >>\nstream\n\nendstream"], content)
}

#[test]
fn type3_advances_follow_the_font_matrix_not_a_fixed_thousandth() {
    // Every case means "A is 0.6 em wide", so at 10 pt each glyph advances
    // 6 pt: AAA spans x 72..90, the Tm at 100 starts a new word AA spanning
    // x 100..112. Glyph box y: baseline 720, 718..730, h 12.
    // The unit matrix is what the real Latin slide export in the spike uses.
    let cases = [
        ("unit matrix", "[1 0 0 1 0 0]", "0.6"),
        ("hundredth matrix", "[0.01 0 0 0.01 0 0]", "60"),
        ("conventional thousandth", "[0.001 0 0 0.001 0 0]", "600"),
    ];
    for (label, matrix, width) in cases {
        let page = &extract_pdf(&type3_fixture(matrix, width)).expect("fixture parses")[0];
        assert_eq!(word_quotes(page), ["AAA", "AA"], "{label}");
        assert_rect_within(
            page.words[0].normalized_rect,
            72.0 / 612.0,
            718.0 / 792.0,
            18.0 / 612.0,
            12.0 / 792.0,
            F32_OPERAND_EPS,
        );
        assert_rect_within(
            page.words[1].normalized_rect,
            100.0 / 612.0,
            718.0 / 792.0,
            12.0 / 612.0,
            12.0 / 792.0,
            F32_OPERAND_EPS,
        );
    }
}

#[test]
fn extractor_panic_on_an_odd_pdf_becomes_an_error() {
    // pdf-extract panics ("missing width") when a Type3 font shows a code
    // outside its Widths range; here only code 65 has a width and the text
    // shows B. The real Downloads census hit a different pdf-extract panic
    // ("missing unicode map and encoding") the same way. The ingest boundary
    // returns Result, so a library panic must not escape it.
    let bytes = type3_fixture_showing("[1 0 0 1 0 0]", "0.6", "BT /F1 10 Tf 72 720 Td (B) Tj ET");
    let result = extract_pdf(&bytes);
    assert!(matches!(result, Err(IngestError::Pdf(_))), "got {result:?}");
}

/// Type0 font with an Identity-H CID descendant whose `/W` is supplied by the
/// caller and `/DW 0`, so any CID that `/W` does not reach has zero advance.
/// CIDs 65..=67 map to the letters A..C through a ToUnicode CMap.
fn cid_fixture(w_entries: &str) -> Vec<u8> {
    let cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
                /CMapName /Adobe-Identity-UCS def /CMapType 2 def \
                1 begincodespacerange <0000> <FFFF> endcodespacerange \
                1 beginbfrange <0041> <0043> <0041> endbfrange \
                endcmap CMapName currentdict /CMap defineresource pop end end";
    let cid_font = format!(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Test \
         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
         /FontDescriptor 7 0 R /DW 0 /W [{w_entries}] >>"
    );
    let descriptor = "<< /Type /FontDescriptor /FontName /Test /Flags 4 \
                      /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 1000 \
                      /Descent 0 /CapHeight 1000 /StemV 80 >>";
    let to_unicode = format!("<< /Length {} >>\nstream\n{}\nendstream", cmap.len(), cmap);
    fixture_pdf_with_objects(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Test /Encoding /Identity-H \
         /DescendantFonts [6 0 R] /ToUnicode 8 0 R >>",
        &[&cid_font, descriptor, &to_unicode],
        "BT /F1 10 Tf 72 720 Td <004100420043> Tj ET",
    )
}

#[test]
fn cid_widths_in_range_form_advance_like_array_form() {
    // Every case says CIDs 65..=67 are 600 units wide. At 10 pt that is 6 pt
    // per glyph, so ABC spans x 72..90, baseline 720: y 718..730, h 12.
    // pdf-extract 0.12.1 reads the `first last width` form wrongly and falls
    // back to /DW 0; Cairo-made PDFs use it for monospace fonts.
    let cases = [
        ("array form", "65 [600 600 600]"),
        ("range form", "65 67 600"),
        ("mixed forms", "65 [600] 66 67 600"),
    ];
    for (label, w) in cases {
        let page = &extract_pdf(&cid_fixture(w)).expect("fixture parses")[0];
        assert_eq!(word_quotes(page), ["ABC"], "{label}");
        let got_w = page.words[0].normalized_rect.w;
        assert!(
            (got_w - 18.0 / 612.0).abs() < F32_OPERAND_EPS,
            "{label}: w {got_w}"
        );
        assert_rect_within(
            page.words[0].normalized_rect,
            72.0 / 612.0,
            718.0 / 792.0,
            18.0 / 612.0,
            12.0 / 792.0,
            F32_OPERAND_EPS,
        );
    }
}

#[test]
fn page_reports_its_media_box_size_in_points() {
    let bytes = fixture_pdf("BT /F1 24 Tf 72 720 Td (Hello world) Tj ET");
    let page = &extract_pdf(&bytes).expect("fixture parses")[0];
    assert_eq!((page.width, page.height), (612.0, 792.0));
}

use crate::fixture::{Form, Page};

const HELLO_FORM: &str = "BT /F1 24 Tf 72 720 Td (Hello) Tj ET";

fn caveats(pages: &[Page]) -> Vec<Option<PageCaveat>> {
    extract_pdf(&crate::fixture::pdf(pages))
        .expect("fixture parses")
        .iter()
        .map(|p| p.caveat)
        .collect()
}

#[test]
fn plain_page_text_carries_no_caveat() {
    assert_eq!(caveats(&[Page::text(HELLO_FORM)]), [None]);
}

#[test]
fn form_text_under_a_scaling_cm_is_flagged_on_its_page_only() {
    let forms = [Form {
        name: "Fm1",
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        content: HELLO_FORM,
    }];
    let pages = [
        Page::text(HELLO_FORM),
        Page {
            content: "q 0.5 0 0 0.5 0 0 cm /Fm1 Do Q",
            forms: &forms,
        },
    ];
    assert_eq!(
        caveats(&pages),
        [None, Some(PageCaveat::TransformedFormText)]
    );
}

#[test]
fn form_text_under_its_own_matrix_is_flagged() {
    let forms = [Form {
        name: "Fm1",
        matrix: [0.5, 0.0, 0.0, 0.5, 0.0, 0.0],
        content: HELLO_FORM,
    }];
    let page = Page {
        content: "/Fm1 Do",
        forms: &forms,
    };
    assert_eq!(caveats(&[page]), [Some(PageCaveat::TransformedFormText)]);
}

#[test]
fn form_text_drawn_untransformed_is_trusted_and_lands_where_computed() {
    // A transform that a q/Q pair has already undone must not count.
    let forms = [Form {
        name: "Fm1",
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        content: HELLO_FORM,
    }];
    let page = Page {
        content: "q 2 0 0 2 0 0 cm Q /Fm1 Do",
        forms: &forms,
    };
    let pages = extract_pdf(&crate::fixture::pdf(&[page])).expect("fixture parses");
    assert_eq!(pages[0].caveat, None);
    // Same geometry as the Hello word in the first fixture test.
    assert_eq!(pages[0].words[0].quote, "Hello");
    assert_rect_close(
        pages[0].words[0].normalized_rect,
        0.117647058823529,
        0.903030303030303,
        0.089333333333333,
        0.036363636363636,
    );
}

#[test]
fn a_transformed_form_that_shows_no_text_is_not_flagged() {
    let forms = [Form {
        name: "Fm1",
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        content: "0 0 10 10 re f",
    }];
    let page = Page {
        content: "q 3 0 0 3 0 0 cm /Fm1 Do Q BT /F1 24 Tf 72 720 Td (Hello) Tj ET",
        forms: &forms,
    };
    assert_eq!(caveats(&[page]), [None]);
}

#[test]
fn the_flagged_case_is_real_form_text_ignores_the_transform_at_do() {
    // Drawn at half scale, Hello truly spans x 36..63.336. pdf-extract places
    // it as if untransformed, x 72..126.672, which is why the page is flagged.
    // If this starts failing, upstream fixed the CTM and the caveat can go.
    let forms = [Form {
        name: "Fm1",
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        content: HELLO_FORM,
    }];
    let page = Page {
        content: "q 0.5 0 0 0.5 0 0 cm /Fm1 Do Q",
        forms: &forms,
    };
    let pages = extract_pdf(&crate::fixture::pdf(&[page])).expect("fixture parses");
    assert_rect_close(
        pages[0].words[0].normalized_rect,
        0.117647058823529,
        0.903030303030303,
        0.089333333333333,
        0.036363636363636,
    );
}
