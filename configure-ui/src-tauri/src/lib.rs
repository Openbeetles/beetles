use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }))
            .setup(|app| {
                app.handle()
                    .plugin(tauri_plugin_window_state::Builder::default().build())?;
                Ok(())
            });
    }

    builder
        .run(tauri::generate_context!())
        .expect("error while running Beetle Configure UI desktop shell");
}
