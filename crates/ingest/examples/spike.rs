//! Spike 3 harness (ticket 03). Reads a PDF in place and prints aggregate
//! extraction statistics. Optional second argument dumps word anchors as TSV
//! for an independent cross-check. The input file is never modified.

use std::io::Write;
use std::time::Instant;
use tutordocs_ingest::pdf::extract_pdf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: spike <pdf-path> [dump.tsv]");
        std::process::exit(2);
    }
    let bytes = std::fs::read(&args[1]).expect("read pdf");
    let t0 = Instant::now();
    let pages = extract_pdf(&bytes).expect("extract");
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let mut words = 0usize;
    let mut quote_chars = 0usize;
    let mut thai_chars = 0usize;
    let mut control_chars = 0usize;
    let mut in_bounds = 0usize;
    let mut zero_area = 0usize;
    let mut empty_pages = 0usize;
    let mut text_lines = 0usize;
    let mut collapsed = 0usize;
    let mut long_words = 0usize;
    let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
    let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
    for p in &pages {
        if p.words.is_empty() {
            empty_pages += 1;
        }
        words += p.words.len();
        text_lines += p.text.lines().count();
        for w in &p.words {
            quote_chars += w.quote.chars().count();
            for c in w.quote.chars() {
                if ('\u{0E00}'..='\u{0E7F}').contains(&c) {
                    thai_chars += 1;
                }
                if c.is_control() {
                    control_chars += 1;
                }
            }
            let r = w.normalized_rect;
            if w.quote.chars().count() >= 3 {
                long_words += 1;
                if r.h > 0.0 && r.w / r.h < 0.1 {
                    collapsed += 1;
                }
            }
            if r.w <= 0.0 || r.h <= 0.0 {
                zero_area += 1;
            }
            if r.x >= -0.01 && r.y >= -0.01 && r.x + r.w <= 1.01 && r.y + r.h <= 1.01 {
                in_bounds += 1;
            }
            min_x = min_x.min(r.x);
            min_y = min_y.min(r.y);
            max_x = max_x.max(r.x + r.w);
            max_y = max_y.max(r.y + r.h);
        }
    }
    println!("pages={}", pages.len());
    println!("words={}", words);
    println!("quote_chars={}", quote_chars);
    println!("thai_chars={}", thai_chars);
    println!("control_chars={}", control_chars);
    println!("empty_pages={}", empty_pages);
    println!("elapsed_ms={:.1}", elapsed_ms);
    println!("rects_in_bounds={}", in_bounds);
    println!("zero_area_rects={}", zero_area);
    println!("text_lines={}", text_lines);
    println!(
        "collapsed_words={}/{} (words of 3+ chars with w/h < 0.1)",
        collapsed, long_words
    );
    if words > 0 {
        println!(
            "rect_extent x[{:.4},{:.4}] y[{:.4},{:.4}]",
            min_x, max_x, min_y, max_y
        );
    }

    if let Some(dump_path) = args.get(2) {
        let mut f = std::fs::File::create(dump_path).expect("create dump");
        for p in &pages {
            for w in &p.words {
                let r = w.normalized_rect;
                writeln!(
                    f,
                    "{}\t{}\t{:.6}\t{:.6}\t{:.6}\t{:.6}",
                    w.page,
                    w.quote.replace(['\t', '\n', '\r'], " "),
                    r.x,
                    r.y,
                    r.w,
                    r.h
                )
                .expect("write dump");
            }
        }
    }
}
