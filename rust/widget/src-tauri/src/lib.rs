pub fn run() {
    let _ = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!());
    let _ = limits_watch_rs::port::native::store();
}
