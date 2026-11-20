#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri shell. Every command forwards to `stegosaur_desktop_core::Core`; heavy ones run off the UI thread.

use std::sync::Arc;
use stegosaur_desktop_core::dto::*;
use stegosaur_desktop_core::Core;
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, State};

struct AppState(Arc<Core>);

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn defaults(state: State<AppState>) -> Defaults {
    state.0.defaults()
}

#[tauri::command]
fn roots(state: State<AppState>) -> Vec<String> {
    state.0.roots()
}

#[tauri::command]
async fn list_dir(state: State<'_, AppState>, path: String) -> Result<ListingDto, String> {
    let core = state.0.clone();
    blocking(move || core.list(&path)).await
}

#[tauri::command]
async fn preview(state: State<'_, AppState>, req: PreviewReq) -> Result<PreviewDto, String> {
    let core = state.0.clone();
    blocking(move || core.preview(req)).await
}

#[tauri::command]
async fn views(state: State<'_, AppState>, req: ViewsReq) -> Result<Response, String> {
    let core = state.0.clone();
    blocking(move || core.views(req)).await.map(Response::new)
}

#[tauri::command]
fn start_job(app: AppHandle, state: State<AppState>, job: JobDto) -> Result<u64, String> {
    state.0.start_job(job, move |event| {
        let _ = app.emit("job", event);
    })
}

#[tauri::command]
fn text_estimate(state: State<AppState>, req: TextEstimateReq) -> Result<TextEstimateDto, String> {
    state.0.text_estimate(req)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState(Arc::new(Core::new())))
        .invoke_handler(tauri::generate_handler![defaults, roots, list_dir, preview, views, start_job, text_estimate])
        .run(tauri::generate_context!())
        .expect("error while running Stegosaur");
}
