//! Visual artifacts in the library: `chapters/<chapter>/visuals/<visual>/`.
//!
//! The reader asks `describe_visual` whether an artifact will show, and the
//! `visual:` scheme serves it at `visual://localhost/<chapter>/<visual>/`. Both
//! map two plain ids, the chapter one from the outline, to fixed file names,
//! and both validate on every call, so no request ever names a file path.

use std::io::Write;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use tauri::{Manager, State};
use tutordocs_store::Library;

use crate::reader::{is_plain_component, ReaderState};
use crate::visual::{self, Kind, Manifest, Rejection};

/// Where the host's protocol trace goes when set. Unset, the trace is off.
const TRACE_ENV: &str = "TUTORDOCS_VISUAL_TRACE";

/// An artifact that passed the validator, read from its directory.
struct Artifact {
    manifest: Manifest,
    index_html: String,
    data: Value,
}

/// What the reader needs before it mounts an artifact.
#[derive(Debug, Serialize)]
#[serde(
    tag = "status",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum VisualStatus {
    Accepted {
        title: String,
        kind: Kind,
        estimated_height: u32,
        data: Value,
    },
    Rejected {
        reason: String,
    },
}

/// Read and validate one artifact. The outer error is a bad request (an id
/// that is not plain, a chapter not in the outline); the inner one is an
/// artifact that will not be shown, which the reader turns into its fallback.
fn load(
    library: &Library,
    chapter_id: &str,
    visual_id: &str,
) -> Result<Result<Artifact, Rejection>, String> {
    if !is_plain_component(chapter_id) || !is_plain_component(visual_id) {
        return Err(format!("not a visual: {chapter_id:?}/{visual_id:?}"));
    }
    let chapters = library.chapters().map_err(|e| e.to_string())?;
    if !chapters.iter().any(|chapter| chapter.id == chapter_id) {
        return Err(format!("no chapter {chapter_id:?} in this library"));
    }
    let dir = library
        .root()
        .join("chapters")
        .join(chapter_id)
        .join("visuals")
        .join(visual_id);
    if !dir.is_dir() {
        return Ok(Err(Rejection(format!(
            "there is no visual \"{visual_id}\" in this chapter"
        ))));
    }
    Ok(read_artifact(&dir, visual_id))
}

fn read_artifact(dir: &Path, visual_id: &str) -> Result<Artifact, Rejection> {
    let read = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .map_err(|e| Rejection(format!("cannot read {name}: {e}")))
    };
    let index_html = read("index.html")?;
    let manifest = visual::validate(visual_id, &index_html, &read("manifest.json")?)?;
    let data = match std::fs::read_to_string(dir.join("data.json")) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| Rejection(format!("data.json is not valid JSON: {e}")))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Object(Default::default()),
        Err(e) => return Err(Rejection(format!("cannot read data.json: {e}"))),
    };
    Ok(Artifact {
        manifest,
        index_html,
        data,
    })
}

#[tauri::command]
pub fn describe_visual(
    state: State<'_, ReaderState>,
    chapter_id: String,
    visual_id: String,
) -> Result<VisualStatus, String> {
    state.with(|library| {
        Ok(match load(library, &chapter_id, &visual_id)? {
            Ok(artifact) => VisualStatus::Accepted {
                title: artifact.manifest.title,
                kind: artifact.manifest.kind,
                estimated_height: artifact.manifest.estimated_height,
                data: artifact.data,
            },
            Err(rejection) => VisualStatus::Rejected {
                reason: rejection.0,
            },
        })
    })
}

/// The host forwards every protocol message and its own outcomes here. Today
/// that only feeds the optional trace; quiz and progress persistence will
/// read the same events.
#[tauri::command]
pub fn record_visual_event(
    chapter_id: String,
    visual_id: String,
    event: Value,
) -> Result<(), String> {
    let Some(path) = std::env::var_os(TRACE_ENV) else {
        return Ok(());
    };
    let line = serde_json::json!({
        "chapterId": chapter_id,
        "visualId": visual_id,
        "event": event,
    });
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| writeln!(file, "{line}"))
        .map_err(|e| format!("cannot write the visual trace: {e}"))
}

/// One `visual:` response: status, content type and body.
struct Reply {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

/// A static figure: one plain file name plus an extension from the closed list.
/// The markdown names it `visuals/<name>.<extension>`. The scheme URL names only
/// the chapter and that file, and the `visuals/` directory is a constant here.
struct ChapterImage {
    name: String,
    extension: ImageExtension,
}

#[derive(Clone, Copy)]
enum ImageExtension {
    Svg,
    Png,
    Jpg,
    Jpeg,
    Gif,
    Webp,
}

impl ImageExtension {
    fn parse(extension: &str) -> Option<Self> {
        Some(match extension {
            "svg" => Self::Svg,
            "png" => Self::Png,
            "jpg" => Self::Jpg,
            "jpeg" => Self::Jpeg,
            "gif" => Self::Gif,
            "webp" => Self::Webp,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Jpeg => "jpeg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    /// The type an `<img>` load expects. SVG stays `image/svg+xml`, so the
    /// page never treats the file as a document and its scripts never run.
    fn content_type(self) -> &'static str {
        match self {
            Self::Svg => "image/svg+xml",
            Self::Png => "image/png",
            Self::Jpg | Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }
}

impl ChapterImage {
    fn file_name(&self) -> String {
        format!("{}.{}", self.name, self.extension.as_str())
    }
}

/// Parse one path component into a [`ChapterImage`]. A slash, a `..`, an
/// absolute path or an extension outside the list is `None`, and the caller
/// must not join that string onto a directory.
fn parse_chapter_image(file: &str) -> Option<ChapterImage> {
    if file.contains('/') || file.contains('\\') || !is_plain_component(file) {
        return None;
    }
    let (name, extension) = file.rsplit_once('.')?;
    if !is_plain_component(name) {
        return None;
    }
    let image = ChapterImage {
        name: name.to_string(),
        extension: ImageExtension::parse(extension)?,
    };
    if image.file_name() != file {
        return None;
    }
    Some(image)
}

fn not_a_visual() -> Reply {
    Reply {
        status: 404,
        content_type: "text/plain; charset=utf-8",
        body: b"not a visual".to_vec(),
    }
}

/// The colour scheme the app forces on a static figure. A figure is an
/// `<img>`, so it follows macOS and cannot see the app's own theme setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    /// `theme=light` or `theme=dark` in the request's query. Anything else
    /// leaves the figure's own palette alone, so macOS decides.
    fn from_query(query: Option<&str>) -> Option<Self> {
        query?.split('&').find_map(|pair| match pair {
            "theme=light" => Some(Self::Light),
            "theme=dark" => Some(Self::Dark),
            _ => None,
        })
    }
}

/// A true media condition and one that is never true, so the rewritten query
/// stays valid next to an `and` and inside a `not`.
const ALWAYS: &str = "(min-width: 0px)";
const NEVER: &str = "(min-width: 99999px)";

/// The scheme a `prefers-color-scheme` condition names, and how many bytes of
/// `rest` it takes. `rest` starts just after the opening parenthesis.
fn color_scheme_condition(rest: &str) -> Option<(Scheme, usize)> {
    const FEATURE: &str = "prefers-color-scheme";
    let after_open = rest.trim_start();
    let mut used = rest.len() - after_open.len();
    let name = after_open.get(..FEATURE.len())?;
    if !name.eq_ignore_ascii_case(FEATURE) {
        return None;
    }
    let after_name = after_open[FEATURE.len()..].trim_start();
    used += after_open.len() - after_name.len();
    let after_colon = after_name.strip_prefix(':')?;
    used += after_name.len() - after_colon.len();
    let value = after_colon.trim_start();
    used += after_colon.len() - value.len();
    let (scheme, word) = if value.get(..4)?.eq_ignore_ascii_case("dark") {
        (Scheme::Dark, 4)
    } else if value.get(..5)?.eq_ignore_ascii_case("light") {
        (Scheme::Light, 5)
    } else {
        return None;
    };
    let after_word = value[word..].trim_start();
    used += value.len() - after_word.len();
    after_word.strip_prefix(')')?;
    Some((scheme, used + 1))
}

/// Pin every `prefers-color-scheme` query in an SVG to the forced scheme. The
/// query for the forced scheme becomes always true and the other never true,
/// so the figure draws the palette the app is showing. Nothing else in the
/// file changes, and the SVG is still served as an image, never inlined.
fn force_scheme(svg: &str, forced: Scheme) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find('(') {
        out.push_str(&rest[..at]);
        let inner = &rest[at + 1..];
        match color_scheme_condition(inner) {
            Some((named, used)) => {
                out.push_str(if named == forced { ALWAYS } else { NEVER });
                rest = &inner[used..];
            }
            None => {
                out.push('(');
                rest = inner;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Read `chapters/<chapter>/visuals/<file>` after the chapter id and the file
/// name have each passed [`is_plain_component`]. The file name is one joined
/// component, never a path the request spelled out.
fn serve_figure(
    library: &Library,
    chapter_id: &str,
    image: &ChapterImage,
    scheme: Option<Scheme>,
) -> Reply {
    if !is_plain_component(chapter_id) {
        return not_a_visual();
    }
    let file_name = image.file_name();
    if !is_plain_component(&file_name) {
        return not_a_visual();
    }
    let Ok(chapters) = library.chapters() else {
        return not_a_visual();
    };
    if !chapters.iter().any(|chapter| chapter.id == chapter_id) {
        return not_a_visual();
    }
    let path = library
        .root()
        .join("chapters")
        .join(chapter_id)
        .join("visuals")
        .join(&file_name);
    if path.file_name().and_then(|name| name.to_str()) != Some(file_name.as_str()) {
        return not_a_visual();
    }
    // A symlink could point outside the chapter. Only a regular file is served.
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        return not_a_visual();
    };
    if !meta.file_type().is_file() {
        return not_a_visual();
    }
    let Ok(mut body) = std::fs::read(&path) else {
        return not_a_visual();
    };
    if let (ImageExtension::Svg, Some(forced)) = (image.extension, scheme) {
        if let Ok(text) = std::str::from_utf8(&body) {
            body = force_scheme(text, forced).into_bytes();
        }
    }
    Reply {
        status: 200,
        content_type: image.extension.content_type(),
        body,
    }
}

/// Answer one `visual:` request path.
///
/// `/<chapter>/<file>` with an allowed image extension is that chapter's
/// `visuals/<file>`, served for an `<img>`. An SVG asked for with `?theme=light`
/// or `?theme=dark` has its `prefers-color-scheme` queries pinned to that
/// theme. `/<chapter>/<visual>/` and its
/// `index.html` are still the interactive artifact. Every other path is a 404.
fn respond(library: &Library, path: &str, scheme: Option<Scheme>) -> Reply {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if let [chapter, file] = parts.as_slice() {
        if let Some(image) = parse_chapter_image(file) {
            return serve_figure(library, chapter, &image, scheme);
        }
    }
    let (chapter_id, visual_id) = match parts.as_slice() {
        [chapter, visual] | [chapter, visual, "" | "index.html"] => (*chapter, *visual),
        _ => return not_a_visual(),
    };
    match load(library, chapter_id, visual_id) {
        Ok(Ok(artifact)) => Reply {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: visual::served_document(&artifact.manifest, &artifact.index_html).into_bytes(),
        },
        Ok(Err(rejection)) => Reply {
            status: 403,
            content_type: "text/plain; charset=utf-8",
            body: rejection.0.into_bytes(),
        },
        Err(_) => not_a_visual(),
    }
}

/// The `visual:` scheme handler. No `Access-Control-Allow-Origin`, so one
/// artifact cannot read another's bytes, and a CSP header the document cannot
/// loosen.
pub fn serve<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
    let path = request.uri().path().to_string();
    let scheme = Scheme::from_query(request.uri().query());
    let reply = ctx
        .app_handle()
        .state::<ReaderState>()
        .with(|library| Ok(respond(library, &path, scheme)))
        .unwrap_or_else(|message| Reply {
            status: 503,
            content_type: "text/plain; charset=utf-8",
            body: message.into_bytes(),
        });
    tauri::http::Response::builder()
        .status(reply.status)
        .header("Content-Type", reply.content_type)
        .header("Content-Security-Policy", visual::ARTIFACT_CSP)
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .body(reply.body.into())
        .expect("static headers are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/visual-library");

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn fixture_library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        copy_dir(Path::new(FIXTURE), dir.path());
        let library = Library::open(dir.path()).unwrap();
        (dir, library)
    }

    fn status(library: &Library, visual_id: &str) -> Value {
        let status = match load(library, "shapes", visual_id).unwrap() {
            Ok(a) => VisualStatus::Accepted {
                title: a.manifest.title,
                kind: a.manifest.kind,
                estimated_height: a.manifest.estimated_height,
                data: a.data,
            },
            Err(r) => VisualStatus::Rejected { reason: r.0 },
        };
        serde_json::to_value(status).unwrap()
    }

    /// The wire shapes vitest replays from the same file.
    fn contract() -> Value {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ui/src/fixtures/visual-contract.json"
        )))
        .unwrap()
    }

    #[test]
    fn an_accepted_artifact_describes_itself_with_its_data() {
        let (_dir, library) = fixture_library();
        assert_eq!(
            status(&library, "cube"),
            contract()["describe_visual_accepted"]
        );
    }

    #[test]
    fn a_cdn_artifact_is_described_as_rejected_with_the_url() {
        let (_dir, library) = fixture_library();
        assert_eq!(
            status(&library, "cdn"),
            contract()["describe_visual_rejected"]
        );
    }

    #[test]
    fn a_missing_artifact_is_rejected_not_an_error() {
        let (_dir, library) = fixture_library();
        assert_eq!(status(&library, "sphere")["status"], "rejected");
    }

    #[test]
    fn an_artifact_with_the_failing_manifest_shape_is_described_as_rejected() {
        let (_dir, library) = fixture_library();
        let sim = library
            .root()
            .join("chapters/shapes/visuals/pue-cooling-simulator");
        std::fs::create_dir_all(&sim).unwrap();
        std::fs::write(
            sim.join("index.html"),
            "<!doctype html><html><body><canvas></canvas></body></html>",
        )
        .unwrap();
        std::fs::write(
            sim.join("manifest.json"),
            r#"{
  "id": "pue-cooling-simulator",
  "title": "Hyperscale PUE and Cooling Efficiency Simulator",
  "description": "Interactive thermodynamic and electrical simulation comparing traditional data center infrastructure against Google hyperscale architectures."
}"#,
        )
        .unwrap();

        let res = status(&library, "pue-cooling-simulator");
        assert_eq!(res["status"], "rejected");
        assert!(
            res["reason"]
                .as_str()
                .unwrap()
                .contains("manifest.json is not a valid manifest"),
            "{res}"
        );
    }

    #[test]
    fn ids_that_climb_out_or_name_no_chapter_are_errors() {
        let (_dir, library) = fixture_library();
        assert!(load(&library, "..", "cube").is_err());
        assert!(load(&library, "shapes", "../../course.json").is_err());
        assert!(load(&library, "nowhere", "cube").is_err());
    }

    #[test]
    fn the_scheme_serves_an_accepted_artifact_with_three_inlined() {
        let (_dir, library) = fixture_library();
        for path in ["/shapes/cube/", "/shapes/cube", "/shapes/cube/index.html"] {
            let reply = respond(&library, path, None);
            assert_eq!(reply.status, 200, "{path}");
            let html = String::from_utf8(reply.body).unwrap();
            assert!(html.contains("globalThis.THREE=Object.freeze("), "{path}");
            assert!(!html.contains(r#"from "three""#), "{path}");
        }
    }

    #[test]
    fn the_scheme_refuses_a_rejected_artifact_with_its_reason() {
        let (_dir, library) = fixture_library();
        let reply = respond(&library, "/shapes/cdn/", None);
        assert_eq!(reply.status, 403);
        assert!(String::from_utf8(reply.body)
            .unwrap()
            .contains("cdn.jsdelivr.net"));
    }

    #[test]
    fn the_scheme_never_serves_any_other_file() {
        let (_dir, library) = fixture_library();
        for path in [
            "/shapes/cube/manifest.json",
            "/shapes/cube/data.json",
            "/shapes/index.md",
            "/course.json",
            "/shapes/../../course.json",
            "/%2e%2e/cube/",
            "/",
        ] {
            let reply = respond(&library, path, None);
            assert_ne!(reply.status, 200, "{path}");
            let body = String::from_utf8(reply.body).unwrap();
            assert!(
                !body.contains("Solids") && !body.contains("schemaVersion"),
                "{path}: {body}"
            );
        }
    }

    #[test]
    fn the_scheme_serves_a_chapters_svg_and_png() {
        let (_dir, library) = fixture_library();
        let visuals = library.root().join("chapters/shapes/visuals");
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
        let png = b"\x89PNG\r\n\x1a\nfigure";
        std::fs::write(visuals.join("x.svg"), svg).unwrap();
        std::fs::write(visuals.join("x.png"), png).unwrap();

        let svg_reply = respond(&library, "/shapes/x.svg", None);
        assert_eq!(svg_reply.status, 200);
        assert_eq!(svg_reply.content_type, "image/svg+xml");
        assert_eq!(svg_reply.body, svg);

        let png_reply = respond(&library, "/shapes/x.png", None);
        assert_eq!(png_reply.status, 200);
        assert_eq!(png_reply.content_type, "image/png");
        assert_eq!(png_reply.body, png);
    }

    const THEMED_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><style>
svg { --bg: #f6f5f2; }
@media (prefers-color-scheme: dark) { svg { --bg: #1a1917; } }
</style></svg>"#;

    #[test]
    fn a_forced_scheme_pins_the_figures_color_scheme_queries() {
        let dark = force_scheme(THEMED_SVG, Scheme::Dark);
        assert!(
            dark.contains("@media (min-width: 0px) { svg { --bg: #1a1917; } }"),
            "{dark}"
        );
        let light = force_scheme(THEMED_SVG, Scheme::Light);
        assert!(
            light.contains("@media (min-width: 99999px) { svg { --bg: #1a1917; } }"),
            "{light}"
        );
        assert!(light.contains("svg { --bg: #f6f5f2; }"), "{light}");
    }

    #[test]
    fn the_rewrite_reads_spacing_case_and_both_schemes() {
        let spaced = "@media ( Prefers-Color-Scheme :DARK ) and (min-width: 400px) {}";
        assert_eq!(
            force_scheme(spaced, Scheme::Dark),
            "@media (min-width: 0px) and (min-width: 400px) {}"
        );
        assert_eq!(
            force_scheme("@media (prefers-color-scheme:light){}", Scheme::Dark),
            "@media (min-width: 99999px){}"
        );
        assert_eq!(
            force_scheme("@media not (prefers-color-scheme: dark) {}", Scheme::Light),
            "@media not (min-width: 99999px) {}"
        );
    }

    #[test]
    fn the_rewrite_leaves_every_other_parenthesis_alone() {
        let plain = r#"<svg><text style="fill: rgb(1, 2, 3)">a (b) (prefers-reduced-motion: reduce) (prefers-color-scheme: dim</text></svg>"#;
        assert_eq!(force_scheme(plain, Scheme::Dark), plain);
    }

    #[test]
    fn the_query_names_a_forced_scheme_only_for_light_and_dark() {
        assert_eq!(Scheme::from_query(Some("theme=dark")), Some(Scheme::Dark));
        assert_eq!(
            Scheme::from_query(Some("a=1&theme=light")),
            Some(Scheme::Light)
        );
        assert_eq!(Scheme::from_query(Some("theme=system")), None);
        assert_eq!(Scheme::from_query(Some("theme=")), None);
        assert_eq!(Scheme::from_query(None), None);
    }

    #[test]
    fn the_scheme_serves_a_themed_svg_for_a_forced_theme_and_the_original_otherwise() {
        let (_dir, library) = fixture_library();
        let visuals = library.root().join("chapters/shapes/visuals");
        std::fs::write(visuals.join("t.svg"), THEMED_SVG).unwrap();
        std::fs::write(visuals.join("t.png"), THEMED_SVG).unwrap();

        let plain = respond(&library, "/shapes/t.svg", None);
        assert_eq!(plain.body, THEMED_SVG.as_bytes());

        let dark = respond(&library, "/shapes/t.svg", Some(Scheme::Dark));
        assert_eq!(dark.status, 200);
        assert_eq!(dark.content_type, "image/svg+xml");
        assert!(String::from_utf8(dark.body)
            .unwrap()
            .contains("@media (min-width: 0px)"));

        let png = respond(&library, "/shapes/t.png", Some(Scheme::Dark));
        assert_eq!(png.body, THEMED_SVG.as_bytes(), "only an SVG is rewritten");
    }

    #[test]
    fn each_allowed_figure_extension_names_one_content_type() {
        let cases = [
            ("x.svg", "image/svg+xml"),
            ("x.png", "image/png"),
            ("x.jpg", "image/jpeg"),
            ("x.jpeg", "image/jpeg"),
            ("x.gif", "image/gif"),
            ("x.webp", "image/webp"),
        ];
        for (name, content_type) in cases {
            let image = parse_chapter_image(name).expect(name);
            assert_eq!(image.name, "x", "{name}");
            assert_eq!(image.file_name(), name);
            assert_eq!(image.extension.content_type(), content_type);
        }
    }

    #[test]
    fn the_scheme_refuses_a_figure_that_is_not_one_plain_image() {
        let (_dir, library) = fixture_library();
        let root = library.root();
        let bait = b"BAIT-OUTSIDE-FIGURE";
        let visuals = root.join("chapters/shapes/visuals");
        std::fs::write(root.join("secret.svg"), bait).unwrap();
        std::fs::write(root.join("chapters/shapes/secret.svg"), bait).unwrap();
        std::fs::create_dir_all(visuals.join("nested")).unwrap();
        std::fs::write(visuals.join("nested/x.svg"), bait).unwrap();
        std::fs::write(visuals.join("x.exe"), bait).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("secret.svg"), visuals.join("link.svg")).unwrap();

        for name in [
            "..",
            "a/b.svg",
            "/etc/passwd.svg",
            "x.exe",
            "../secret.svg",
            "nested/x.svg",
            r"a\b.svg",
        ] {
            assert!(parse_chapter_image(name).is_none(), "{name}");
        }

        for path in [
            "/shapes/../secret.svg",
            "/shapes/..",
            "/shapes/a/b.svg",
            "/shapes/nested/x.svg",
            "/shapes//etc/passwd.svg",
            "/etc/passwd.svg",
            "/shapes/x.exe",
            "/shapes/missing.svg",
            "/shapes/visuals/x.svg",
            "/shapes/link.svg",
        ] {
            let reply = respond(&library, path, None);
            assert_ne!(reply.status, 200, "{path}");
            assert!(
                !reply.body.windows(bait.len()).any(|window| window == bait),
                "{path}"
            );
        }
    }
}
