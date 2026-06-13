use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{
    image::Image,
    menu::{Menu, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

const TRAY_MENU_HOME: &str = "tray-home";
const TRAY_MENU_AGENT: &str = "tray-agent";
const TRAY_MENU_TIMETABLE: &str = "tray-timetable";
const TRAY_MENU_TODO: &str = "tray-todo";
const TRAY_MENU_QUIT: &str = "tray-quit";

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    // macOS: monochrome template icon for the menu bar.
    // Windows: use the full-colour app icon for the system tray.
    #[cfg(target_os = "macos")]
    let icon = Image::from_bytes(include_bytes!("../icons/tray-icon@2x.png"))
        .map_err(|e| tauri::Error::AssetNotFound(format!("tray icon: {}", e)))?;
    #[cfg(not(target_os = "macos"))]
    let icon = Image::from_bytes(include_bytes!("../icons/icon.png"))
        .map_err(|e| tauri::Error::AssetNotFound(format!("tray icon: {}", e)))?;

    let home = MenuItemBuilder::with_id(TRAY_MENU_HOME, "ホーム").build(app)?;
    let agent = MenuItemBuilder::with_id(TRAY_MENU_AGENT, "Agent").build(app)?;
    let timetable = MenuItemBuilder::with_id(TRAY_MENU_TIMETABLE, "時間割").build(app)?;
    let todo = MenuItemBuilder::with_id(TRAY_MENU_TODO, "TODO").build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::with_id(TRAY_MENU_QUIT, "終了").build(app)?;
    let menu = Menu::with_items(app, &[&home, &agent, &timetable, &todo, &separator, &quit])?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Selah")
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_MENU_HOME => {
                let _ = show_main_window_with_tab(app, Some("home"));
            }
            TRAY_MENU_AGENT => {
                let _ = show_main_window_with_tab(app, Some("agent"));
            }
            TRAY_MENU_TIMETABLE => {
                let _ = show_main_window_with_tab(app, Some("timetable"));
            }
            TRAY_MENU_TODO => {
                let _ = show_main_window_with_tab(app, Some("todo"));
            }
            TRAY_MENU_QUIT => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } if cfg!(target_os = "windows") => {
                let _ = show_main_window_with_tab(tray.app_handle(), Some("home"));
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

pub(crate) fn show_main_window_with_tab(app: &AppHandle, tab: Option<&str>) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(tab) = tab {
            let _ = app.emit("tray-open-tab", tab);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    show_main_window_with_tab(&app, None)
}

#[tauri::command]
pub fn show_main_agent_window(app: AppHandle) -> Result<(), String> {
    show_main_window_with_tab(&app, Some("agent"))
}

// ============ Tray Status Cycling ============

/// Shared state for cycling tray title text
pub struct TrayStatusState {
    inner: Mutex<(Vec<String>, usize)>,
    running: AtomicBool,
}

impl TrayStatusState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new((Vec::new(), 0)),
            running: AtomicBool::new(false),
        }
    }
}

/// Start the background cycling task (call once at setup)
pub fn start_tray_cycle(app: &AppHandle, state: Arc<TrayStatusState>) {
    if state.running.swap(true, Ordering::SeqCst) {
        return; // already running
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(8));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await; // first tick fires immediately, skip it
        let mut last_text = String::new();
        loop {
            interval.tick().await;
            let Ok(mut guard) = state.inner.lock() else {
                continue; // mutex poisoned, skip this tick
            };
            let (ref items, ref mut idx) = *guard;
            if items.is_empty() {
                // Clear tray title when no items
                if !last_text.is_empty() {
                    last_text.clear();
                    drop(guard);
                    if let Some(tray) = app.tray_by_id("main-tray") {
                        let _ = tray.set_title(None::<&str>);
                    }
                }
                continue;
            }
            // Single item: set once, skip cycling
            if items.len() == 1 {
                let text = format!(" {}", items[0]);
                drop(guard);
                if text != last_text {
                    last_text = text.clone();
                    if let Some(tray) = app.tray_by_id("main-tray") {
                        let _ = tray.set_title(Some(&text));
                    }
                }
                continue;
            }
            *idx %= items.len();
            let text = format!(" {}", items[*idx]);
            *idx = (*idx + 1) % items.len();
            drop(guard);
            if text != last_text {
                last_text = text.clone();
                if let Some(tray) = app.tray_by_id("main-tray") {
                    let _ = tray.set_title(Some(&text));
                }
            }
        }
    });
}

/// Update the cycling status items from the frontend
#[tauri::command]
pub fn set_tray_status_items(
    state: tauri::State<'_, Arc<TrayStatusState>>,
    items: Vec<String>,
) -> Result<(), String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    guard.0 = items;
    guard.1 = 0;
    Ok(())
}
