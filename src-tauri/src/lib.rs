//! TutorDocs shell. One reader window that lists the library and reads chapters
//! (`reader`), writes courses and chapters through the agent (`generate`), and
//! the sandboxed visual contract (`visual`) and the artifacts it serves
//! (`artifacts`).

mod agents;
mod annotations;
mod artifacts;
pub mod ctl;
mod generate;
mod jobs;
mod pdf_sources;
mod quiz;
mod reader;
mod sources;
mod trash;
mod visual;

pub use visual::{validate, visual_sandbox_attribute, Manifest};

pub fn run() {
    let root = reader::default_library_root();
    let state = reader::ReaderState::open(&root);
    let agents = agents::AgentsState::installed(&root);
    #[cfg_attr(not(feature = "e2e-control"), allow(unused_mut))]
    let mut context = tauri::generate_context!();
    // A test build keeps no web view storage, so a replay never reads or
    // writes the WebKit data the user's own TutorDocs window shares.
    #[cfg(feature = "e2e-control")]
    for window in context.config_mut().app.windows.iter_mut() {
        window.incognito = true;
    }
    let app = reader::install(tauri::Builder::default(), state)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(generate::GenerationState::new(root, agents.clone()))
        .register_uri_scheme_protocol("visual", artifacts::serve)
        .manage(agents)
        .on_page_load(|_webview, payload| {
            #[cfg(feature = "e2e-control")]
            eprintln!("tutordocs-ctl page {:?} {}", payload.event(), payload.url());
            #[cfg(not(feature = "e2e-control"))]
            let _ = payload;
        })
        .setup(|app| {
            use tauri::{Emitter, Manager};
            let handle = app.handle().clone();
            app.state::<jobs::JobRegistry>()
                .attach_emit(std::sync::Arc::new(move |name, payload| {
                    let _ = handle.emit(name, payload);
                }));
            #[cfg(feature = "e2e-control")]
            {
                crate::ctl::start_for_app(app.handle().clone()).map_err(std::io::Error::other)?;
                // Wide enough that the study panel is in the grid (it hides below 1080px).
                if let Some(window) = app.get_webview_window("reader") {
                    let url = window
                        .url()
                        .map(|u| u.to_string())
                        .unwrap_or_else(|e| e.to_string());
                    eprintln!("tutordocs-ctl webview url {url}");
                    let _ = window
                        .set_size(tauri::Size::Logical(tauri::LogicalSize::new(1280.0, 900.0)));
                } else {
                    eprintln!("tutordocs-ctl webview missing at setup");
                }
            }
            Ok(())
        })
        .build(context)
        .expect("error while building tauri application");
    app.run(|_app_handle, _event| {
        #[cfg(feature = "e2e-control")]
        if matches!(_event, tauri::RunEvent::Exit) {
            crate::ctl::cleanup_for_app(_app_handle);
        }
    });
}
