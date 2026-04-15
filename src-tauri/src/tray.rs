use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager, Runtime,
};

const MAIN_WINDOW_LABEL: &str = "main";
const MENU_SHOW_WINDOW: &str = "show-window";
const MENU_LOCK: &str = "lock";
const MENU_QUIT: &str = "quit";

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Err(error) = window.show() {
            log::error!("failed to show main window: {error}");
        }

        if let Err(error) = window.set_focus() {
            log::error!("failed to focus main window: {error}");
        }
    } else {
        log::warn!("main window '{MAIN_WINDOW_LABEL}' not found");
    }
}

pub fn setup_tray(app: &App) -> tauri::Result<()> {
    let show_window = MenuItemBuilder::with_id(MENU_SHOW_WINDOW, "Show Window").build(app)?;
    let lock = MenuItemBuilder::with_id(MENU_LOCK, "Lock")
        .enabled(false)
        .build(app)?;
    let quit = MenuItemBuilder::with_id(MENU_QUIT, "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&show_window)
        .item(&lock)
        .separator()
        .item(&quit)
        .build()?;

    TrayIconBuilder::new()
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("app icon should be set"),
        )
        .tooltip("bw-agent")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW_WINDOW => show_main_window(app),
            MENU_LOCK => log::info!("tray lock clicked (placeholder)"),
            MENU_QUIT => std::process::exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event: TrayIconEvent| match event {
            TrayIconEvent::DoubleClick { .. } => show_main_window(tray.app_handle()),
            #[cfg(target_os = "windows")]
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        })
        .build(app)?;

    Ok(())
}
