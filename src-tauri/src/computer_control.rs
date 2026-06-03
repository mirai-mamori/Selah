use serde_json::{json, Value};
use tauri::Manager;

#[derive(Debug, Clone, Copy)]
struct ScreenRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn owner_window_label(target: Option<&str>) -> Option<String> {
    target.map(crate::webview_toolbar::browser_window_label_from_target)
}

fn window_screen_rect(app: &tauri::AppHandle, target: Option<&str>) -> Result<ScreenRect, String> {
    let label = owner_window_label(target).unwrap_or_else(|| "main".to_string());
    let window = app
        .get_window(&label)
        .or_else(|| target.and_then(|t| app.get_window(t)))
        .ok_or_else(|| format!("window not found: {}", label))?;
    let pos = window
        .outer_position()
        .map_err(|e| format!("window position failed: {}", e))?;
    let size = window
        .outer_size()
        .map_err(|e| format!("window size failed: {}", e))?;
    Ok(ScreenRect {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    })
}

fn focus_target(app: &tauri::AppHandle, target: Option<&str>) {
    let Some(target) = target else {
        return;
    };
    let label = owner_window_label(Some(target)).unwrap_or_else(|| target.to_string());
    if let Some(window) = app.get_window(&label) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if let Some(webview) = app.get_webview(target) {
        let _ = webview.set_focus();
    }
}

fn resolve_point(
    app: &tauri::AppHandle,
    target: Option<&str>,
    x: f64,
    y: f64,
    coordinate_space: Option<&str>,
) -> Result<(f64, f64), String> {
    match coordinate_space.unwrap_or("screenshot") {
        "screen" => Ok((x, y)),
        "webview" | "viewport" => {
            let origin = webview_screen_origin(app, target)?;
            Ok((origin.0 + x, origin.1 + y))
        }
        "screenshot" | "target" | "" => {
            let rect = window_screen_rect(app, target)?;
            Ok((rect.x as f64 + x, rect.y as f64 + y))
        }
        other => Err(format!("unknown coordinate_space: {}", other)),
    }
}

fn webview_screen_origin(
    app: &tauri::AppHandle,
    target: Option<&str>,
) -> Result<(f64, f64), String> {
    let target = target
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .ok_or_else(|| "webview coordinate_space requires a target".to_string())?;
    let webview = app
        .get_webview(target)
        .ok_or_else(|| format!("webview not found: {}", target))?;
    let wv_pos = webview
        .position()
        .map_err(|e| format!("webview position failed: {}", e))?;
    let owner_label = owner_window_label(Some(target)).unwrap_or_else(|| target.to_string());
    if owner_label == target {
        return Ok((wv_pos.x as f64, wv_pos.y as f64));
    }
    let owner = app
        .get_window(&owner_label)
        .ok_or_else(|| format!("window not found: {}", owner_label))?;
    let owner_pos = owner
        .outer_position()
        .map_err(|e| format!("window position failed: {}", e))?;
    if target.ends_with("-ct") {
        return Ok((owner_pos.x as f64, owner_pos.y as f64));
    }
    let owner_size = owner
        .outer_size()
        .map_err(|e| format!("window size failed: {}", e))?;
    if wv_pos.x >= owner_pos.x
        && wv_pos.y >= owner_pos.y
        && wv_pos.x <= owner_pos.x.saturating_add(owner_size.width as i32)
        && wv_pos.y <= owner_pos.y.saturating_add(owner_size.height as i32)
    {
        return Ok((wv_pos.x as f64, wv_pos.y as f64));
    }
    Ok((
        owner_pos.x as f64 + wv_pos.x as f64,
        owner_pos.y as f64 + wv_pos.y as f64,
    ))
}

pub async fn screenshot(app: &tauri::AppHandle, target: Option<&str>) -> Result<Value, String> {
    let rect = window_screen_rect(app, target)?;
    screenshot_rect(rect, target).await
}

#[cfg(target_os = "macos")]
async fn screenshot_rect(rect: ScreenRect, target: Option<&str>) -> Result<Value, String> {
    use base64::Engine;
    use std::process::Command;

    let path = std::env::temp_dir().join(format!(
        "selah-computer-screenshot-{}-{}.png",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));
    let region = format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height);
    let output = Command::new("screencapture")
        .args(["-x", "-t", "png", "-R", &region])
        .arg(&path)
        .output()
        .map_err(|e| format!("screencapture failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "screencapture exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("read screenshot failed: {}", e))?;
    let _ = std::fs::remove_file(&path);
    Ok(json!({
        "target": target.unwrap_or(""),
        "coordinate_space": "screenshot",
        "screen_rect": {
            "x": rect.x,
            "y": rect.y,
            "width": rect.width,
            "height": rect.height,
        },
        "image": {
            "mime": "image/png",
            "data_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
        },
    }))
}

#[cfg(not(target_os = "macos"))]
async fn screenshot_rect(_rect: ScreenRect, _target: Option<&str>) -> Result<Value, String> {
    Err("computer_screenshot is not implemented on this platform yet".into())
}

pub async fn mouse_click(
    app: &tauri::AppHandle,
    target: Option<&str>,
    x: f64,
    y: f64,
    coordinate_space: Option<&str>,
) -> Result<Value, String> {
    focus_target(app, target);
    let (sx, sy) = resolve_point(app, target, x, y, coordinate_space)?;
    platform_mouse_click(sx, sy)?;
    Ok(json!({
        "status": "ok",
        "target": target.unwrap_or(""),
        "screen_x": sx,
        "screen_y": sy,
    }))
}

pub async fn mouse_drag(
    app: &tauri::AppHandle,
    target: Option<&str>,
    from_x: f64,
    from_y: f64,
    to_x: f64,
    to_y: f64,
    steps: u64,
    coordinate_space: Option<&str>,
) -> Result<Value, String> {
    focus_target(app, target);
    let (sx0, sy0) = resolve_point(app, target, from_x, from_y, coordinate_space)?;
    let (sx1, sy1) = resolve_point(app, target, to_x, to_y, coordinate_space)?;
    platform_mouse_drag(sx0, sy0, sx1, sy1, steps.clamp(2, 32))?;
    Ok(json!({
        "status": "ok",
        "target": target.unwrap_or(""),
        "from": { "screen_x": sx0, "screen_y": sy0 },
        "to": { "screen_x": sx1, "screen_y": sy1 },
    }))
}

pub async fn scroll(
    app: &tauri::AppHandle,
    target: Option<&str>,
    delta_y: i32,
    x: Option<f64>,
    y: Option<f64>,
    coordinate_space: Option<&str>,
) -> Result<Value, String> {
    focus_target(app, target);
    if target.is_some() || x.is_some() || y.is_some() {
        let (sx, sy) = if let (Some(x), Some(y)) = (x, y) {
            resolve_point(app, target, x, y, coordinate_space)?
        } else {
            let rect = window_screen_rect(app, target)?;
            (
                rect.x as f64 + rect.width as f64 / 2.0,
                rect.y as f64 + rect.height as f64 / 2.0,
            )
        };
        platform_mouse_move(sx, sy)?;
    }
    platform_scroll(delta_y)?;
    Ok(json!({
        "status": "ok",
        "target": target.unwrap_or(""),
        "delta_y": delta_y,
    }))
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::thread;
    use std::time::Duration;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    const K_CG_HID_EVENT_TAP: u32 = 0;
    const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
    const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
    const K_CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
    const K_CG_MOUSE_BUTTON_LEFT: u32 = 0;
    const K_CG_SCROLL_EVENT_UNIT_PIXEL: u32 = 0;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateMouseEvent(
            source: *mut c_void,
            mouse_type: u32,
            mouse_cursor_position: CGPoint,
            mouse_button: u32,
        ) -> *mut c_void;
        fn CGEventCreateScrollWheelEvent(
            source: *mut c_void,
            units: u32,
            wheel_count: u32,
            wheel1: i32,
        ) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
        fn CGPreflightPostEventAccess() -> bool;
        fn CFRelease(cf: *const c_void);
    }

    fn ensure_post_event_access() -> Result<(), String> {
        let allowed = unsafe { CGPreflightPostEventAccess() };
        if allowed {
            Ok(())
        } else {
            Err("macOS is blocking synthetic input events. Grant Selah Accessibility/Input Monitoring permission, then retry.".into())
        }
    }

    fn post_mouse(kind: u32, x: f64, y: f64) -> Result<(), String> {
        ensure_post_event_access()?;
        let event = unsafe {
            CGEventCreateMouseEvent(
                std::ptr::null_mut(),
                kind,
                CGPoint { x, y },
                K_CG_MOUSE_BUTTON_LEFT,
            )
        };
        if event.is_null() {
            return Err("CGEventCreateMouseEvent returned null".into());
        }
        unsafe {
            CGEventPost(K_CG_HID_EVENT_TAP, event);
            CFRelease(event.cast());
        }
        Ok(())
    }

    pub fn click(x: f64, y: f64) -> Result<(), String> {
        post_mouse(K_CG_EVENT_MOUSE_MOVED, x, y)?;
        thread::sleep(Duration::from_millis(30));
        post_mouse(K_CG_EVENT_LEFT_MOUSE_DOWN, x, y)?;
        thread::sleep(Duration::from_millis(45));
        post_mouse(K_CG_EVENT_LEFT_MOUSE_UP, x, y)
    }

    pub fn move_to(x: f64, y: f64) -> Result<(), String> {
        post_mouse(K_CG_EVENT_MOUSE_MOVED, x, y)
    }

    pub fn drag(x0: f64, y0: f64, x1: f64, y1: f64, steps: u64) -> Result<(), String> {
        post_mouse(K_CG_EVENT_MOUSE_MOVED, x0, y0)?;
        thread::sleep(Duration::from_millis(30));
        post_mouse(K_CG_EVENT_LEFT_MOUSE_DOWN, x0, y0)?;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let x = x0 + (x1 - x0) * t;
            let y = y0 + (y1 - y0) * t;
            post_mouse(K_CG_EVENT_LEFT_MOUSE_DRAGGED, x, y)?;
            thread::sleep(Duration::from_millis(16));
        }
        post_mouse(K_CG_EVENT_LEFT_MOUSE_UP, x1, y1)
    }

    pub fn scroll(delta_y: i32) -> Result<(), String> {
        ensure_post_event_access()?;
        let event = unsafe {
            CGEventCreateScrollWheelEvent(
                std::ptr::null_mut(),
                K_CG_SCROLL_EVENT_UNIT_PIXEL,
                1,
                delta_y,
            )
        };
        if event.is_null() {
            return Err("CGEventCreateScrollWheelEvent returned null".into());
        }
        unsafe {
            CGEventPost(K_CG_HID_EVENT_TAP, event);
            CFRelease(event.cast());
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn platform_mouse_click(x: f64, y: f64) -> Result<(), String> {
    macos::click(x, y)
}

#[cfg(not(target_os = "macos"))]
fn platform_mouse_click(_x: f64, _y: f64) -> Result<(), String> {
    Err("computer_mouse_click is not implemented on this platform yet".into())
}

#[cfg(target_os = "macos")]
fn platform_mouse_drag(x0: f64, y0: f64, x1: f64, y1: f64, steps: u64) -> Result<(), String> {
    macos::drag(x0, y0, x1, y1, steps)
}

#[cfg(not(target_os = "macos"))]
fn platform_mouse_drag(_x0: f64, _y0: f64, _x1: f64, _y1: f64, _steps: u64) -> Result<(), String> {
    Err("computer_mouse_drag is not implemented on this platform yet".into())
}

#[cfg(target_os = "macos")]
fn platform_mouse_move(x: f64, y: f64) -> Result<(), String> {
    macos::move_to(x, y)
}

#[cfg(not(target_os = "macos"))]
fn platform_mouse_move(_x: f64, _y: f64) -> Result<(), String> {
    Err("computer_mouse_move is not implemented on this platform yet".into())
}

#[cfg(target_os = "macos")]
fn platform_scroll(delta_y: i32) -> Result<(), String> {
    macos::scroll(delta_y)
}

#[cfg(not(target_os = "macos"))]
fn platform_scroll(_delta_y: i32) -> Result<(), String> {
    Err("computer_scroll is not implemented on this platform yet".into())
}
