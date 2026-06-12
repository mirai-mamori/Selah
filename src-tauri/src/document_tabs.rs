use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};
use tauri::{Emitter, Manager};

const OWNER_LABEL: &str = "document-tabs";

// Injected into browser tabs so links that target a new window (or window.open)
// reliably open as a managed tab instead of being silently dropped by WKWebView.
const BROWSER_LINK_HANDLER_SCRIPT: &str = r#"(function () {
  function invoke() {
    var t = window.__TAURI__;
    return (t && t.core && t.core.invoke) || (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) || null;
  }
  function openTab(url) {
    var inv = invoke();
    if (!inv || !/^https?:\/\//i.test(String(url || ''))) return false;
    inv('open_external_url', { url: String(url) }).catch(function () {});
    return true;
  }
  document.addEventListener('click', function (e) {
    try {
      if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
      var a = e.target && e.target.closest ? e.target.closest('a[href]') : null;
      if (!a) return;
      var target = (a.getAttribute('target') || '').toLowerCase();
      if (target !== '_blank') return;
      if (openTab(a.href)) {
        e.preventDefault();
        e.stopPropagation();
      }
    } catch (_) {}
  }, true);
  try {
    var nativeOpen = window.open;
    window.open = function (url) {
      if (openTab(url)) return null;
      return nativeOpen ? nativeOpen.apply(window, arguments) : null;
    };
  } catch (_) {}
})();"#;
const TAB_STRIP_LABEL: &str = "document-tabs-strip";
const AGENT_PANEL_LABEL: &str = "document-tabs-agent";
const TAB_STRIP_HEIGHT: f64 = 88.0;
/// Detective is a full-screen game with no per-tab controls, so its strip
/// collapses to just the tab row (no second toolbar row).
const COMPACT_STRIP_HEIGHT: f64 = 46.0;
const AGENT_PANEL_WIDTH: f64 = 360.0;
const SPLIT_DIVIDER_WIDTH: f64 = 12.0;
const MAX_SPLIT_DIVIDERS: usize = 2;
const MIN_SPLIT_RATIO: f64 = 0.2;
const DEFAULT_WIDTH: f64 = 1080.0;
const DEFAULT_HEIGHT: f64 = 760.0;
const OFFSCREEN_X: f64 = 50_000.0;
const MAX_TABS: usize = 32;

static TAB_COUNTER: AtomicU32 = AtomicU32::new(0);
static AGENT_PANEL_OPEN: AtomicBool = AtomicBool::new(false);
// Set right before a programmatic window.close() (e.g. last tab closed) so the
// CloseRequested guard lets it through; otherwise the red close button only hides
// the window, preserving open tabs for the next time it is shown.
static FORCE_CLOSE: AtomicBool = AtomicBool::new(false);
static DOCUMENT_WINDOWS: LazyLock<Mutex<HashMap<String, DocumentWindowState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTabInfo {
    pub id: String,
    pub label: String,
    pub target: String,
    pub title: String,
    pub url: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub active: bool,
    pub loading: bool,
    pub controls: Vec<DocumentTabControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub split_ratios: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reopen: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTabControl {
    pub id: String,
    pub label: String,
    pub action: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub tone: Option<String>,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTabProbeReport {
    pub owner: String,
    pub id: String,
    pub label: String,
    pub target: String,
    pub title: String,
    pub url: String,
    pub kind: String,
    pub href: String,
    pub ready_state: String,
    pub visibility_state: String,
    pub has_content: bool,
    pub content_children: i64,
    pub content_text_length: i64,
    pub content_html_length: i64,
    pub has_loading: bool,
    pub has_error: bool,
    pub has_detail_wrap: bool,
    pub has_page_title: bool,
    pub viewport_width: i64,
    pub viewport_height: i64,
    pub body_width: i64,
    pub body_height: i64,
    pub preview: String,
}

#[derive(Debug, Clone)]
struct ChildPane {
    target: String,
    label: String,
    /// Whether this pane shows a discussion board (掲示板). Only a board pane may
    /// spawn a third (grandchild) pane when you drill into a thread from it.
    is_board: bool,
    child: Option<Box<ChildPane>>,
}

/// Flatten a child-pane chain into (target, label) pairs (pane2, pane3, …).
fn pane_chain(child: &Option<ChildPane>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cur = child.as_ref();
    while let Some(c) = cur {
        out.push((c.target.clone(), c.label.clone()));
        cur = c.child.as_deref();
    }
    out
}

/// Remove the pane with `target` (and any panes below it) from a chain. Returns
/// the (target, label) pairs that were removed, or None if not found.
fn truncate_pane_at(node: &mut Option<ChildPane>, target: &str) -> Option<Vec<(String, String)>> {
    // The pane itself (pane2).
    if node.as_ref().map(|c| c.target == target).unwrap_or(false) {
        let removed = pane_chain(node);
        *node = None;
        return Some(removed);
    }
    // A deeper pane (pane3).
    if let Some(parent) = node.as_mut() {
        let is_deeper = parent
            .child
            .as_ref()
            .map(|b| b.target == target)
            .unwrap_or(false);
        if is_deeper {
            let removed = parent
                .child
                .as_deref()
                .map(|b| {
                    let mut v = vec![(b.target.clone(), b.label.clone())];
                    v.extend(pane_chain(&b.child.as_deref().cloned()));
                    v
                })
                .unwrap_or_default();
            parent.child = None;
            return Some(removed);
        }
    }
    None
}

#[derive(Debug, Clone)]
struct DocumentTab {
    id: String,
    label: String,
    target: String,
    key: Option<String>,
    title: String,
    url: String,
    kind: String,
    controls: Vec<DocumentTabControl>,
    reopen: Option<serde_json::Value>,
    loading: bool,
    child: Option<ChildPane>,
    split_weights: Vec<f64>,
    active_split_drag: Option<usize>,
}

#[derive(Debug, Clone, Default)]
struct DocumentWindowState {
    tabs: Vec<DocumentTab>,
    active: Option<String>,
}

impl DocumentTab {
    fn info(&self, active: bool) -> DocumentTabInfo {
        DocumentTabInfo {
            id: self.id.clone(),
            label: self.label.clone(),
            target: self.target.clone(),
            title: self.title.clone(),
            url: self.url.clone(),
            kind: self.kind.clone(),
            active,
            loading: self.loading,
            controls: self.controls.clone(),
            split_ratios: split_ratios_for_layout(&self.child, &self.split_weights),
            reopen: self.reopen.clone(),
        }
    }
}

fn default_split_weights(columns: usize) -> Vec<f64> {
    if columns <= 1 {
        Vec::new()
    } else {
        vec![1.0 / columns as f64; columns]
    }
}

fn normalize_split_weights(weights: &[f64], columns: usize) -> Vec<f64> {
    if columns <= 1 {
        return Vec::new();
    }
    let cleaned: Vec<f64> = weights
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect();
    if cleaned.len() != columns {
        return default_split_weights(columns);
    }
    let sum: f64 = cleaned.iter().sum();
    if sum <= f64::EPSILON {
        return default_split_weights(columns);
    }
    cleaned.into_iter().map(|value| value / sum).collect()
}

fn clamped_split_weights_with_min(weights: &[f64], columns: usize, min_ratio: f64) -> Vec<f64> {
    if columns <= 1 {
        return Vec::new();
    }
    let base = normalize_split_weights(weights, columns);
    let min_ratio = min_ratio.min(1.0 / columns as f64);
    if min_ratio <= 0.0 {
        return base;
    }

    let mut out = vec![0.0; columns];
    let mut locked = vec![false; columns];
    let mut remaining_total = 1.0;

    loop {
        let unlocked: Vec<usize> = (0..columns).filter(|index| !locked[*index]).collect();
        if unlocked.is_empty() {
            break;
        }
        let unlocked_weight: f64 = unlocked.iter().map(|index| base[*index]).sum();
        let unlocked_count = unlocked.len();
        let mut changed = false;

        for index in unlocked {
            let projected = if unlocked_weight <= f64::EPSILON {
                remaining_total / unlocked_count as f64
            } else {
                base[index] / unlocked_weight * remaining_total
            };
            if projected + 1e-9 < min_ratio {
                out[index] = min_ratio;
                locked[index] = true;
                remaining_total = (remaining_total - min_ratio).max(0.0);
                changed = true;
            }
        }

        if !changed {
            let unlocked: Vec<usize> = (0..columns).filter(|index| !locked[*index]).collect();
            let unlocked_weight: f64 = unlocked.iter().map(|index| base[*index]).sum();
            let unlocked_count = unlocked.len();
            for index in unlocked {
                out[index] = if unlocked_weight <= f64::EPSILON {
                    remaining_total / unlocked_count as f64
                } else {
                    base[index] / unlocked_weight * remaining_total
                };
            }
            break;
        }
    }

    normalize_split_weights(&out, columns)
}

fn clamped_split_weights(weights: &[f64], columns: usize) -> Vec<f64> {
    clamped_split_weights_with_min(weights, columns, MIN_SPLIT_RATIO)
}

fn split_ratios_for_layout(child: &Option<ChildPane>, weights: &[f64]) -> Vec<f64> {
    let columns = 1 + pane_chain(child).len();
    if columns <= 1 {
        Vec::new()
    } else {
        clamped_split_weights(weights, columns)
    }
}

fn split_widths_for_layout(total_width: f64, columns: usize, weights: &[f64]) -> Vec<f64> {
    if columns == 0 {
        return Vec::new();
    }
    if columns == 1 {
        return vec![total_width.max(0.0)];
    }
    let divider_total = SPLIT_DIVIDER_WIDTH * (columns.saturating_sub(1)) as f64;
    let usable_width = (total_width - divider_total).max(120.0);
    clamped_split_weights(weights, columns)
        .into_iter()
        .map(|weight| usable_width * weight)
        .collect()
}

fn split_weights_for_second(weights: &[f64]) -> Vec<f64> {
    match clamped_split_weights(weights, weights.len()).as_slice() {
        [left, right] => vec![*left, *right],
        [left, middle, right] => vec![*left, middle + right],
        _ => default_split_weights(2),
    }
}

fn logical_window_size(window: &tauri::Window) -> Result<(f64, f64), String> {
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    Ok((size.width as f64 / scale, size.height as f64 / scale))
}

fn content_width_for_owner(app: &tauri::AppHandle, owner: &str) -> Result<f64, String> {
    let window = app
        .get_window(owner)
        .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
    let (width, _) = logical_window_size(&window)?;
    let clamped_width = width.max(320.0);
    let panel_width = agent_panel_width(clamped_width);
    Ok((clamped_width - panel_width).max(260.0))
}

fn tab_strip_url(owner: &str) -> String {
    format!(
        "index.html#surface=document-tabs&owner={}",
        urlencoding::encode(owner)
    )
}

fn ensure_tab_strip(
    app: &tauri::AppHandle,
    window: &tauri::Window,
    owner: &str,
) -> Result<(), String> {
    if app.get_webview(TAB_STRIP_LABEL).is_some() {
        return Ok(());
    }
    let strip = tauri::webview::WebviewBuilder::new(
        TAB_STRIP_LABEL,
        tauri::WebviewUrl::App(tab_strip_url(owner).into()),
    )
    .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
    .auto_resize();
    window
        .add_child(
            strip,
            tauri::Position::Logical(tauri::LogicalPosition::new(0.0, 0.0)),
            tauri::Size::Logical(tauri::LogicalSize::new(DEFAULT_WIDTH, TAB_STRIP_HEIGHT)),
        )
        .map(|_| ())
        .map_err(|e| format!("タブバー作成失敗: {}", e))
}

fn emit_tabs_changed(app: &tauri::AppHandle, owner: &str) {
    let tabs = list_tabs_for_owner(owner);
    let payload = serde_json::json!({
        "owner": owner,
        "tabs": tabs,
    });
    let mut labels = vec![TAB_STRIP_LABEL.to_string(), AGENT_PANEL_LABEL.to_string()];
    for tab in list_tabs_for_owner(owner) {
        for index in 0..MAX_SPLIT_DIVIDERS {
            labels.push(split_divider_target(&tab.target, index));
        }
    }
    for label in labels {
        let _ = app.emit_to(
            tauri::EventTarget::AnyLabel { label },
            "document-tabs-changed",
            payload.clone(),
        );
    }
}

fn emit_agent_visibility(app: &tauri::AppHandle, open: bool) {
    let _ = app.emit_to(
        tauri::EventTarget::AnyLabel {
            label: TAB_STRIP_LABEL.to_string(),
        },
        "document-tabs-agent-visibility",
        serde_json::json!({
            "owner": OWNER_LABEL,
            "open": open,
        }),
    );
}

pub fn emit_agent_status(app: &tauri::AppHandle, target: &str, active: bool, action: &str) {
    if !AGENT_PANEL_OPEN.load(Ordering::Relaxed) {
        return;
    }
    let _ = app.emit_to(
        tauri::EventTarget::AnyLabel {
            label: AGENT_PANEL_LABEL.to_string(),
        },
        "browser-agent-status",
        serde_json::json!({
            "target": target,
            "active": active,
            "action": action,
        }),
    );
}

fn list_tabs_for_owner(owner: &str) -> Vec<DocumentTabInfo> {
    let state = DOCUMENT_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(owner)
        .cloned()
        .unwrap_or_default();
    state
        .tabs
        .iter()
        .map(|tab| tab.info(state.active.as_deref() == Some(tab.id.as_str())))
        .collect()
}

fn active_tab_for_owner(owner: &str) -> Option<DocumentTab> {
    let state = DOCUMENT_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(owner)
        .cloned()?;
    let active = state.active.as_deref()?;
    state.tabs.iter().find(|tab| tab.id == active).cloned()
}

/// Targets of every **live** pane that makes up the owner's current view: the
/// active tab's main webview plus its split child panes (pane2, pane3, …), in
/// left-to-right order. One element when there is no split. Used so the agent
/// can read/operate on any pane of the current split view, not just the active
/// one.
pub fn active_view_panes(app: &tauri::AppHandle, owner: &str) -> Vec<String> {
    let Some(tab) = active_tab_for_owner(owner) else {
        return Vec::new();
    };
    std::iter::once(tab.target.clone())
        .chain(
            pane_chain(&tab.child)
                .into_iter()
                .map(|(target, _label)| target),
        )
        .filter(|target| app.get_webview(target).is_some())
        .collect()
}

fn ensure_window(app: &tauri::AppHandle, owner: &str) -> Result<tauri::Window, String> {
    if let Some(window) = app.get_window(owner) {
        if AGENT_PANEL_OPEN.load(Ordering::Relaxed) && app.get_webview(AGENT_PANEL_LABEL).is_none()
        {
            AGENT_PANEL_OPEN.store(false, Ordering::Relaxed);
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        ensure_tab_strip(app, &window, owner)?;
        let _ = resize_current_for_owner(app, owner);
        return Ok(window);
    }

    AGENT_PANEL_OPEN.store(false, Ordering::Relaxed);
    let builder = tauri::window::WindowBuilder::new(app, owner)
        .title("Copilot")
        .inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
        .min_inner_size(640.0, 420.0)
        .resizable(true);

    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);

    // On Windows we drop the native title bar and draw our own min/max/close in
    // the tab strip, matching the main window (see lib.rs set_decorations(false)).
    #[cfg(target_os = "windows")]
    let builder = builder.decorations(false);

    let window = builder
        .build()
        .map_err(|e| format!("タブウィンドウ作成失敗: {}", e))?;

    // Open as an independent window: cascade off the main window so it never
    // spawns exactly stacked on top of it. Falls back to screen-center.
    if let Some(main) = app.get_webview_window("main") {
        if let Ok(pos) = main.outer_position() {
            // 48 logical px cascade, kept constant across DPI.
            let offset = (48.0 * main.scale_factor().unwrap_or(1.0)).round() as i32;
            let _ =
                window.set_position(tauri::PhysicalPosition::new(pos.x + offset, pos.y + offset));
        } else {
            let _ = window.center();
        }
    } else {
        let _ = window.center();
    }

    ensure_tab_strip(app, &window, owner)?;

    DOCUMENT_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(owner.to_string())
        .or_default();

    let app_resize = app.clone();
    let owner_resize = owner.to_string();
    let win_for_scale = window.clone();
    window.on_window_event(move |event| {
        match event {
            tauri::WindowEvent::Resized(phys_size) => {
                let scale = win_for_scale.scale_factor().unwrap_or(1.0);
                resize_document_window(
                    &app_resize,
                    &owner_resize,
                    phys_size.width as f64 / scale,
                    phys_size.height as f64 / scale,
                );
            }
            tauri::WindowEvent::CloseRequested { api, .. } => {
                // User hit the close button: keep the tabs alive by hiding instead
                // of destroying. A programmatic close (last tab) sets FORCE_CLOSE.
                if !FORCE_CLOSE.swap(false, Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = win_for_scale.hide();
                }
            }
            _ => {}
        }
    });

    Ok(window)
}

/// Truly close (destroy) the document-tabs window, bypassing the hide-on-close
/// guard. Used when there are no tabs left to preserve.
fn force_close_window(window: &tauri::Window) {
    FORCE_CLOSE.store(true, Ordering::Relaxed);
    let _ = window.close();
}

fn agent_panel_width(width: f64) -> f64 {
    if AGENT_PANEL_OPEN.load(Ordering::Relaxed) {
        AGENT_PANEL_WIDTH.min((width * 0.42).max(300.0))
    } else {
        0.0
    }
}

fn agent_panel_url(tab: Option<&DocumentTab>) -> String {
    let (target, title, kind) = tab
        .map(|tab| (tab.target.as_str(), tab.title.as_str(), tab.kind.as_str()))
        .unwrap_or((OWNER_LABEL, "エージェント", "agent"));
    format!(
        "index.html#surface=agent-panel&owner={}&target={}&title={}&kind={}",
        urlencoding::encode(OWNER_LABEL),
        urlencoding::encode(target),
        urlencoding::encode(title),
        urlencoding::encode(kind),
    )
}

fn open_agent_panel(app: &tauri::AppHandle) -> Result<(), String> {
    let window = ensure_window(app, OWNER_LABEL)?;
    let active = active_tab_for_owner(OWNER_LABEL);
    if app.get_webview(AGENT_PANEL_LABEL).is_none() {
        let panel = tauri::webview::WebviewBuilder::new(
            AGENT_PANEL_LABEL,
            tauri::WebviewUrl::App(agent_panel_url(active.as_ref()).into()),
        )
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
        .auto_resize();
        let size = window.inner_size().map_err(|e| e.to_string())?;
        let scale = window.scale_factor().unwrap_or(1.0);
        let width = size.width as f64 / scale;
        let height = size.height as f64 / scale;
        let panel_width = AGENT_PANEL_WIDTH.min((width * 0.42).max(300.0));
        window
            .add_child(
                panel,
                tauri::Position::Logical(tauri::LogicalPosition::new(
                    (width - panel_width).max(260.0),
                    TAB_STRIP_HEIGHT,
                )),
                tauri::Size::Logical(tauri::LogicalSize::new(
                    panel_width,
                    (height - TAB_STRIP_HEIGHT).max(120.0),
                )),
            )
            .map_err(|e| format!("Agent パネル作成失敗: {}", e))?;
    }
    AGENT_PANEL_OPEN.store(true, Ordering::Relaxed);
    resize_current_for_owner(app, OWNER_LABEL)?;
    emit_agent_visibility(app, true);
    emit_tabs_changed(app, OWNER_LABEL);
    Ok(())
}

fn close_agent_panel(app: &tauri::AppHandle) -> Result<(), String> {
    AGENT_PANEL_OPEN.store(false, Ordering::Relaxed);
    if let Some(panel) = app.get_webview(AGENT_PANEL_LABEL) {
        let _ = panel.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
            OFFSCREEN_X,
            TAB_STRIP_HEIGHT,
        )));
        let _ = panel.set_size(tauri::Size::Logical(tauri::LogicalSize::new(0.0, 0.0)));
    }
    let has_tabs = !list_tabs_for_owner(OWNER_LABEL).is_empty();
    if has_tabs {
        resize_current_for_owner(app, OWNER_LABEL)?;
    } else if let Some(window) = app.get_window(OWNER_LABEL) {
        force_close_window(&window);
        DOCUMENT_WINDOWS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(OWNER_LABEL);
    }
    emit_agent_visibility(app, false);
    Ok(())
}

pub fn open_agent_workspace(app: &tauri::AppHandle) -> Result<(), String> {
    let window = ensure_window(app, OWNER_LABEL)?;
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    open_agent_panel(app)
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn activation_probe_script(owner: &str, tab: &DocumentTab) -> String {
    format!(
        r#"(function () {{
  try {{
    window.dispatchEvent(new Event('resize'));
    if (typeof window.__selahDetailActivate === 'function') {{
      window.__selahDetailActivate();
    }} else {{
      window.dispatchEvent(new CustomEvent('selah-tab-activated'));
    }}
  }} catch (_) {{}}
  try {{
    setTimeout(function () {{
      try {{
    var content = document.getElementById('content');
    var body = document.body;
    var source = content || body;
    var text = source ? String(source.textContent || '') : '';
    var html = content ? String(content.innerHTML || '') : '';
    var rect = body ? body.getBoundingClientRect() : null;
    var invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
    if (!invoke) return;
    invoke('document_tabs_report_probe', {{
      report: {{
        owner: {owner},
        id: {id},
        label: {label},
        target: {target},
        title: {title},
        url: {url},
        kind: {kind},
        href: String(window.location && window.location.href || ''),
        readyState: String(document.readyState || ''),
        visibilityState: String(document.visibilityState || ''),
        hasContent: !!content,
        contentChildren: content ? content.children.length : -1,
        contentTextLength: text.replace(/\s+/g, '').length,
        contentHtmlLength: html.length,
        hasLoading: !!(content && content.querySelector('.loading')),
        hasError: !!(content && content.querySelector('.error')),
        hasDetailWrap: !!(content && content.querySelector('.detail-wrap')),
        hasPageTitle: !!(content && content.querySelector('.page-title')),
        viewportWidth: Math.round(window.innerWidth || 0),
        viewportHeight: Math.round(window.innerHeight || 0),
        bodyWidth: rect ? Math.round(rect.width) : 0,
        bodyHeight: rect ? Math.round(rect.height) : 0,
        preview: text.replace(/\s+/g, ' ').trim().slice(0, 180)
      }}
    }}).catch(function () {{}});
      }} catch (_) {{}}
    }}, 900);
  }} catch (_) {{}}
}})();"#,
        owner = js_string(owner),
        id = js_string(&tab.id),
        label = js_string(&tab.label),
        target = js_string(&tab.target),
        title = js_string(&tab.title),
        url = js_string(&tab.url),
        kind = js_string(&tab.kind),
    )
}

fn title_report_script(target: &str) -> String {
    format!(
        r#"(function () {{
  function send() {{
    try {{
      var title = String(document.title || '').trim();
      var invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
      if (invoke && title) {{
        invoke('document_tabs_report_title', {{ target: {target}, title: title }}).catch(function () {{}});
      }}
    }} catch (_) {{}}
  }}
  send();
  // Many sites set <title> via JS just after load; re-read shortly after.
  setTimeout(send, 700);
}})();"#,
        target = js_string(target),
    )
}

fn notify_tab_activated(app: &tauri::AppHandle, owner: &str, tab: &DocumentTab) {
    if let Some(webview) = app.get_webview(&tab.target) {
        // Make the freshly activated content webview the key responder so clicks
        // and keyboard input land on the page, not the (now hidden) old tab.
        let _ = webview.set_focus();
        if let Err(err) = webview.eval(&activation_probe_script(owner, tab)) {
            log::warn!(
                "document tab activation probe failed owner={} target={} kind={} err={}",
                owner,
                tab.target,
                tab.kind,
                err
            );
        }
    }
}

/// Cheap read of just the active tab's kind (no full-state clone), so the strip
/// can be resized on every drag tick without waiting on a heavy clone.
fn active_kind_for_owner(owner: &str) -> Option<String> {
    DOCUMENT_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(owner)
        .and_then(|state| {
            let active = state.active.as_deref()?;
            state
                .tabs
                .iter()
                .find(|tab| tab.id == active)
                .map(|tab| tab.kind.clone())
        })
}

fn resize_document_window(app: &tauri::AppHandle, owner: &str, width: f64, height: f64) {
    // Resize the strip FIRST, with the least possible work, so its responsive
    // width @media (max-width: 760px) tracks the window edge closely during a
    // drag. Detective collapses the toolbar row away (shorter strip); only a
    // cheap active-kind read is needed — the heavier full-state clone for the
    // content layout happens afterwards.
    let strip_h = if active_kind_for_owner(owner).as_deref() == Some("detective") {
        COMPACT_STRIP_HEIGHT
    } else {
        TAB_STRIP_HEIGHT
    };
    let width = width.max(320.0);
    let height = height.max(strip_h + 120.0);
    if let Some(strip) = app.get_webview(TAB_STRIP_LABEL) {
        let _ = strip.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
            0.0, 0.0,
        )));
        let _ = strip.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            width, strip_h,
        )));
    }

    let state = DOCUMENT_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(owner)
        .cloned()
        .unwrap_or_default();
    let panel_width = agent_panel_width(width);
    let content_width = (width - panel_width).max(260.0);
    let document_height = (height - strip_h).max(120.0);
    let window = app.get_window(owner);
    let active = state.active.as_deref();
    let content_height = document_height.max(120.0);
    for tab in state.tabs {
        let is_active = active == Some(tab.id.as_str());
        // All split panes attached to this tab (pane2, pane3, …).
        let all_panes = pane_chain(&tab.child);
        // The contiguous panes that actually have a live webview, in order.
        let live_panes: Vec<&(String, String)> = all_panes
            .iter()
            .take_while(|(target, _)| app.get_webview(target).is_some())
            .collect();

        if is_active {
            let columns = 1 + live_panes.len();
            let widths = split_widths_for_layout(content_width, columns, &tab.split_weights);
            if let Some(webview) = app.get_webview(&tab.target) {
                let main_width = widths.first().copied().unwrap_or(content_width);
                let _ = webview.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                    main_width,
                    content_height,
                )));
                let _ = webview.set_position(tauri::Position::Logical(
                    tauri::LogicalPosition::new(0.0, strip_h),
                ));
                let _ = webview.show();
            }
            if let Some(window) = window.as_ref() {
                for index in 0..live_panes.len() {
                    if let Err(err) =
                        ensure_split_divider_webview(app, window, owner, &tab.target, index)
                    {
                        log::warn!(
                            "split divider ensure failed owner={} parent={} index={} err={}",
                            owner,
                            tab.target,
                            index,
                            err
                        );
                    }
                }
            }
            close_split_dividers(app, &tab.target, live_panes.len());

            let mut x = widths.first().copied().unwrap_or(content_width);
            for (index, (target, _)) in live_panes.iter().enumerate() {
                let divider_x = x;
                if let Some(divider) = app.get_webview(&split_divider_target(&tab.target, index)) {
                    let dragging_this_divider = tab.active_split_drag == Some(index);
                    let divider_width = if dragging_this_divider {
                        content_width
                    } else {
                        SPLIT_DIVIDER_WIDTH
                    };
                    let divider_pos_x = if dragging_this_divider {
                        0.0
                    } else {
                        divider_x
                    };
                    let _ = divider.set_position(tauri::Position::Logical(
                        tauri::LogicalPosition::new(divider_pos_x, TAB_STRIP_HEIGHT),
                    ));
                    let _ = divider.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                        divider_width,
                        content_height,
                    )));
                    let _ = divider.show();
                }
                let pane_x = divider_x + SPLIT_DIVIDER_WIDTH;
                if let Some(view) = app.get_webview(target) {
                    let pane_width = widths.get(index + 1).copied().unwrap_or_else(|| {
                        ((content_width - SPLIT_DIVIDER_WIDTH * live_panes.len() as f64)
                            / columns as f64)
                            .max(120.0)
                    });
                    let _ = view.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                        pane_width,
                        content_height,
                    )));
                    let _ = view.set_position(tauri::Position::Logical(
                        tauri::LogicalPosition::new(pane_x, TAB_STRIP_HEIGHT),
                    ));
                    let _ = view.show();
                }
                x = pane_x + widths.get(index + 1).copied().unwrap_or(0.0);
            }
        } else {
            hide_split_dividers(app, &tab.target);
            if let Some(webview) = app.get_webview(&tab.target) {
                let _ = webview.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                    content_width,
                    content_height,
                )));
                let _ = webview.set_position(tauri::Position::Logical(
                    tauri::LogicalPosition::new(OFFSCREEN_X, TAB_STRIP_HEIGHT),
                ));
                let _ = webview.show();
            }
            for (target, _) in &all_panes {
                if let Some(view) = app.get_webview(target) {
                    let _ = view.set_position(tauri::Position::Logical(
                        tauri::LogicalPosition::new(OFFSCREEN_X, TAB_STRIP_HEIGHT),
                    ));
                }
            }
        }
    }

    if panel_width > 0.0 {
        if let Some(panel) = app.get_webview(AGENT_PANEL_LABEL) {
            let _ = panel.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                content_width,
                strip_h,
            )));
            let _ = panel.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                panel_width,
                document_height,
            )));
        }
    }
}

pub fn resize_current_for_owner(app: &tauri::AppHandle, owner: &str) -> Result<(), String> {
    let window = app
        .get_window(owner)
        .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    resize_document_window(
        app,
        owner,
        size.width as f64 / scale,
        size.height as f64 / scale,
    );
    Ok(())
}

fn activate_tab_inner(app: &tauri::AppHandle, owner: &str, tab_id: &str) -> Result<(), String> {
    let active_tab = {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states
            .get_mut(owner)
            .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
        let tab = state
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id || tab.target == tab_id || tab.label == tab_id)
            .cloned()
            .ok_or_else(|| format!("タブが見つかりません: {}", tab_id))?;
        state.active = Some(tab.id.clone());
        tab
    };

    crate::webview_toolbar::set_owner_active_target(
        owner,
        &active_tab.target,
        &active_tab.title,
        &active_tab.kind,
    );
    if let Some(window) = app.get_window(owner) {
        let _ = window.set_title(&active_tab.title);
        let _ = window.set_focus();
    }
    resize_current_for_owner(app, owner)?;
    notify_tab_activated(app, owner, &active_tab);
    emit_tabs_changed(app, owner);
    Ok(())
}

fn open_tab(
    app: &tauri::AppHandle,
    key: Option<String>,
    label: Option<String>,
    target: Option<String>,
    url: tauri::WebviewUrl,
    initial_url: String,
    title: String,
    kind: String,
    init_scripts: &[&str],
    reopen: Option<serde_json::Value>,
) -> Result<DocumentTabInfo, String> {
    let owner = OWNER_LABEL;
    let window = ensure_window(app, owner)?;
    if let Some(key) = key.as_deref() {
        let existing = DOCUMENT_WINDOWS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(owner)
            .and_then(|state| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.key.as_deref() == Some(key))
                    .cloned()
            });
        if let Some(tab) = existing {
            activate_tab_inner(app, owner, &tab.id)?;
            return Ok(tab.info(true));
        }
    }

    let count = DOCUMENT_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(owner)
        .map(|state| state.tabs.len())
        .unwrap_or(0);
    if count >= MAX_TABS {
        return Err(format!("タブは最大 {} 件までです", MAX_TABS));
    }

    let idx = TAB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let label = label.unwrap_or_else(|| format!("document-tab-{}", idx));
    let target = target.unwrap_or_else(|| format!("{}-ct", label));
    if app.get_webview(&target).is_some() {
        return Err(format!("Webview target already exists: {}", target));
    }

    let mut builder = tauri::webview::WebviewBuilder::new(&target, url)
        // Without this, the first click after the window is unfocused (or after a
        // tab switch) is swallowed to focus the webview instead of reaching the
        // page — making some pages feel "uninteractive".
        .accept_first_mouse(true)
        .initialization_script(crate::webview_toolbar::browser_bridge_script())
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled);
    for script in init_scripts {
        builder = builder.initialization_script(*script);
    }
    if kind == "browser" {
        // WKWebView ignores target="_blank" / window.open unless the app opens a
        // new window itself; the native new-window hook is unreliable, so route
        // those clicks to a managed tab from inside the page.
        builder = builder.initialization_script(BROWSER_LINK_HANDLER_SCRIPT);
    }

    let app_for_load = app.clone();
    let owner_for_load = owner.to_string();
    let tab_id_for_load = label.clone();
    let target_for_load = target.clone();
    builder = builder.on_page_load(move |webview, payload| {
        let started = matches!(payload.event(), tauri::webview::PageLoadEvent::Started);
        let finished = matches!(payload.event(), tauri::webview::PageLoadEvent::Finished);
        if !started && !finished {
            return;
        }
        let url = payload.url().to_string();
        crate::webview_toolbar::set_readable_url(&target_for_load, &url);
        let mut should_emit = false;
        if let Ok(mut states) = DOCUMENT_WINDOWS.lock() {
            if let Some(state) = states.get_mut(&owner_for_load) {
                if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id_for_load) {
                    tab.url = url.clone();
                    tab.loading = started;
                    should_emit = true;
                }
            }
        }
        if should_emit {
            emit_tabs_changed(&app_for_load, &owner_for_load);
        }
        // Browser tabs start with a host-name placeholder title; pull the real
        // <title> from the loaded page and reflect it on the tab.
        if finished {
            let _ = webview.eval(&title_report_script(&target_for_load));
        }
    });

    let app_for_popup = app.clone();
    builder = builder.on_new_window(move |popup_url, _features| {
        let app_for_open = app_for_popup.clone();
        let _ = app_for_popup.run_on_main_thread(move || {
            let _ = open_external_tab(&app_for_open, popup_url.to_string(), None);
        });
        tauri::webview::NewWindowResponse::Deny
    });

    window
        .add_child(
            builder,
            tauri::Position::Logical(tauri::LogicalPosition::new(0.0, TAB_STRIP_HEIGHT)),
            tauri::Size::Logical(tauri::LogicalSize::new(
                DEFAULT_WIDTH,
                DEFAULT_HEIGHT - TAB_STRIP_HEIGHT,
            )),
        )
        .map_err(|e| format!("タブ作成失敗: {}", e))?;

    let tab = DocumentTab {
        id: label.clone(),
        label: label.clone(),
        target: target.clone(),
        key,
        title: title.clone(),
        url: initial_url,
        kind: kind.clone(),
        controls: Vec::new(),
        reopen,
        loading: false,
        child: None,
        split_weights: Vec::new(),
        active_split_drag: None,
    };
    {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states.entry(owner.to_string()).or_default();
        state.tabs.push(tab.clone());
        state.active = Some(tab.id.clone());
    }
    crate::webview_toolbar::register_readable_child(app, owner, &label, &target, &title, &kind);
    activate_tab_inner(app, owner, &tab.id)?;
    Ok(tab.info(true))
}

pub fn open_markdown_reader_tab(
    app: &tauri::AppHandle,
    key: String,
    title: String,
    source_path: Option<String>,
) -> Result<DocumentTabInfo, String> {
    let label = key;
    let target = format!("{}-ct", label);
    let url = format!(
        "index.html#surface=markdown-reader&tabLabel={}&ownerLabel={}",
        urlencoding::encode(&target),
        urlencoding::encode(OWNER_LABEL)
    );
    // Only bookmarkable when we know the file path to reopen it from.
    let reopen = source_path
        .map(|path| serde_json::json!({ "type": "reader", "path": path, "title": title }));
    open_tab(
        app,
        Some(label.clone()),
        Some(label),
        Some(target),
        tauri::WebviewUrl::App(url.clone().into()),
        url,
        title,
        "reader".to_string(),
        &[],
        reopen,
    )
}

/// Open (or focus) a tab that loads one of the app's own `#surface=…` pages in
/// the Copilot window. `key` makes it a singleton (re-open focuses the existing
/// tab); `extra_query` appends to the URL hash (e.g. "&course=…").
fn open_app_surface_tab(
    app: &tauri::AppHandle,
    surface: &str,
    kind: &str,
    title: String,
    key: Option<&str>,
    extra_query: &str,
) -> Result<DocumentTabInfo, String> {
    let idx = TAB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let label = format!("document-tab-{}-{}", kind, idx);
    let target = format!("{}-ct", label);
    let url = format!(
        "index.html#surface={}&tabLabel={}&ownerLabel={}{}",
        surface,
        urlencoding::encode(&target),
        urlencoding::encode(OWNER_LABEL),
        extra_query
    );
    open_tab(
        app,
        key.map(str::to_string),
        Some(label),
        Some(target),
        tauri::WebviewUrl::App(url.clone().into()),
        url,
        title,
        kind.to_string(),
        &[],
        None,
    )
}

pub fn open_new_tab(app: &tauri::AppHandle) -> Result<DocumentTabInfo, String> {
    open_app_surface_tab(app, "home", "home", "新しいタブ".to_string(), None, "")
}

fn detail_kind(params: &str) -> &'static str {
    if params.contains("mode=kwic") || params.contains("mode=kwicCabinet") {
        "kwic"
    } else if params.contains("mode=kgc") || params.contains("mode=syllabus") {
        "kgc"
    } else {
        "detail"
    }
}

fn detail_webview_url(target: &str, params: &str) -> String {
    let suffix = if params.trim().is_empty() {
        String::new()
    } else {
        format!(
            "&{}",
            params.trim_start_matches('?').trim_start_matches('&')
        )
    };
    format!(
        "index.html#surface=university-detail&tabLabel={}&ownerLabel={}{}",
        urlencoding::encode(target),
        urlencoding::encode(OWNER_LABEL),
        suffix
    )
}

fn split_divider_target(parent_target: &str, index: usize) -> String {
    format!("{}-divider-{}", parent_target, index)
}

fn split_divider_url(owner: &str, parent_target: &str, index: usize) -> String {
    format!(
        "index.html#surface=split-divider&ownerLabel={}&parentTarget={}&handleIndex={}",
        urlencoding::encode(owner),
        urlencoding::encode(parent_target),
        index
    )
}

fn ensure_split_divider_webview(
    app: &tauri::AppHandle,
    window: &tauri::Window,
    owner: &str,
    parent_target: &str,
    index: usize,
) -> Result<String, String> {
    let target = split_divider_target(parent_target, index);
    if app.get_webview(&target).is_none() {
        let url = split_divider_url(owner, parent_target, index);
        let builder =
            tauri::webview::WebviewBuilder::new(&target, tauri::WebviewUrl::App(url.into()))
                .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled);
        window
            .add_child(
                builder,
                tauri::Position::Logical(tauri::LogicalPosition::new(
                    OFFSCREEN_X,
                    TAB_STRIP_HEIGHT,
                )),
                tauri::Size::Logical(tauri::LogicalSize::new(SPLIT_DIVIDER_WIDTH, 120.0)),
            )
            .map_err(|e| format!("分割バー作成失敗: {}", e))?;
    }
    Ok(target)
}

fn close_split_dividers(app: &tauri::AppHandle, parent_target: &str, keep: usize) {
    for index in keep..MAX_SPLIT_DIVIDERS {
        if let Some(webview) = app.get_webview(&split_divider_target(parent_target, index)) {
            let _ = webview.close();
        }
    }
}

fn hide_split_dividers(app: &tauri::AppHandle, parent_target: &str) {
    for index in 0..MAX_SPLIT_DIVIDERS {
        if let Some(webview) = app.get_webview(&split_divider_target(parent_target, index)) {
            let _ = webview.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                OFFSCREEN_X,
                TAB_STRIP_HEIGHT,
            )));
            let _ = webview.set_size(tauri::Size::Logical(tauri::LogicalSize::new(0.0, 0.0)));
        }
    }
}

/// Open (or replace) the split child pane attached to the active detail tab.
/// The child sits to the right of its parent and is only shown while the parent
/// is the active tab.
fn create_pane_webview(
    app: &tauri::AppHandle,
    window: &tauri::Window,
    owner: &str,
    target: &str,
    label: &str,
    params: &str,
    title: &str,
) -> Result<(), String> {
    if let Some(existing) = app.get_webview(target) {
        let _ = existing.close();
    }
    crate::webview_toolbar::unregister_readable_label(label);
    let url = detail_webview_url(target, params);
    // Inject the browser bridge (__selahBrowserExtractText / __selahBrowserRunAction)
    // just like open_tab does, so the agent's DOM tools (read_browser_page,
    // browser_click, browser_fill, …) actually work inside split child panes.
    let target_for_load = target.to_string();
    let builder = tauri::webview::WebviewBuilder::new(target, tauri::WebviewUrl::App(url.into()))
        .initialization_script(crate::webview_toolbar::browser_bridge_script())
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
        .on_page_load(move |_webview, payload| {
            crate::webview_toolbar::set_readable_url(&target_for_load, payload.url().as_str());
        });
    window
        .add_child(
            builder,
            tauri::Position::Logical(tauri::LogicalPosition::new(OFFSCREEN_X, TAB_STRIP_HEIGHT)),
            tauri::Size::Logical(tauri::LogicalSize::new(DEFAULT_WIDTH / 3.0, 120.0)),
        )
        .map_err(|e| format!("分割ビュー作成失敗: {}", e))?;
    crate::webview_toolbar::register_readable_child(
        app,
        owner,
        label,
        target,
        title,
        detail_kind(params),
    );
    Ok(())
}

/// Open a split child pane. `origin` is the target of the webview that triggered
/// the open, so we know which pane to attach to. A third pane is only added when
/// drilling out of a discussion-board pane (掲示板).
pub fn open_child_detail(
    app: &tauri::AppHandle,
    params: String,
    title: String,
    origin: &str,
) -> Result<(), String> {
    let owner = OWNER_LABEL;
    let window = ensure_window(app, owner)?;
    let parent = active_tab_for_owner(owner).ok_or_else(|| "親詳細タブがありません".to_string())?;
    if parent.kind == "home" || parent.kind == "files" {
        return open_university_detail_tab(app, params, title).map(|_| ());
    }

    let new_is_board = params.contains("mode=discussion");
    let pane2_target = format!("{}-s1", parent.target);
    let pane2_label = format!("{}-s1", parent.label);
    let pane3_target = format!("{}-s2", parent.target);
    let pane3_label = format!("{}-s2", parent.label);

    // Inspect the current chain to decide where the new pane attaches.
    #[derive(PartialEq)]
    enum Slot {
        Pane2,
        Pane3,
    }
    let (slot, to_close): (Slot, Vec<(String, String)>) = {
        let states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let child = states
            .get(owner)
            .and_then(|s| s.tabs.iter().find(|t| t.id == parent.id))
            .and_then(|t| t.child.as_ref());
        let p2_present = child.is_some();
        let p2_is_board = child.map(|c| c.is_board).unwrap_or(false);
        let p3 = child.and_then(|c| c.child.as_deref());

        if origin == pane2_target && p2_present && p2_is_board {
            // Drilling out of a board pane → (re)place the third pane, keep pane2.
            (Slot::Pane3, pane_chain(&p3.cloned()))
        } else if origin == pane3_target && p2_present {
            // Drilling within the third pane → replace it.
            (Slot::Pane3, pane_chain(&p3.cloned()))
        } else {
            // Drilling from the parent (or a non-board pane) → reset to two panes,
            // dropping any existing children.
            let mut close = Vec::new();
            if let Some(c) = child {
                close.push((c.target.clone(), c.label.clone()));
                close.extend(pane_chain(&c.child.as_deref().cloned()));
            }
            (Slot::Pane2, close)
        }
    };

    for (target, label) in &to_close {
        crate::webview_toolbar::unregister_readable_label(label);
        if let Some(wv) = app.get_webview(target) {
            let _ = wv.close();
        }
    }

    let (target, label) = match slot {
        Slot::Pane2 => (pane2_target.clone(), pane2_label.clone()),
        Slot::Pane3 => (pane3_target.clone(), pane3_label.clone()),
    };
    create_pane_webview(app, &window, owner, &target, &label, &params, &title)?;

    {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = states.get_mut(owner) {
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == parent.id) {
                let new_pane = ChildPane {
                    target: target.clone(),
                    label: label.clone(),
                    is_board: new_is_board,
                    child: None,
                };
                match slot {
                    Slot::Pane2 => {
                        tab.child = Some(new_pane);
                        tab.split_weights = split_weights_for_second(&tab.split_weights);
                    }
                    Slot::Pane3 => {
                        if let Some(p2) = tab.child.as_mut() {
                            p2.child = Some(Box::new(new_pane));
                            // Reset to equal thirds (1:1:1) when the third pane appears.
                            tab.split_weights = default_split_weights(3);
                        } else {
                            tab.child = Some(new_pane);
                            tab.split_weights = split_weights_for_second(&tab.split_weights);
                        }
                    }
                }
            }
        }
    }
    close_split_dividers(
        app,
        &parent.target,
        match slot {
            Slot::Pane2 => 1,
            Slot::Pane3 => 2,
        },
    );
    resize_current_for_owner(app, owner)?;
    emit_tabs_changed(app, owner);
    Ok(())
}

fn close_child_pane(app: &tauri::AppHandle, owner: &str, parent_id: &str) -> Result<(), String> {
    let (parent_target, panes) = {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states
            .get_mut(owner)
            .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
        let tab = state
            .tabs
            .iter_mut()
            .find(|tab| tab.id == parent_id || tab.target == parent_id)
            .ok_or_else(|| format!("タブが見つかりません: {}", parent_id))?;
        let chain = pane_chain(&tab.child);
        let parent_target = tab.target.clone();
        tab.child = None;
        tab.split_weights.clear();
        tab.active_split_drag = None;
        (parent_target, chain)
    };
    for (target, label) in panes {
        crate::webview_toolbar::unregister_readable_label(&label);
        if let Some(webview) = app.get_webview(&target) {
            let _ = webview.close();
        }
    }
    close_split_dividers(app, &parent_target, 0);
    resize_current_for_owner(app, owner)?;
    emit_tabs_changed(app, owner);
    Ok(())
}

pub fn open_university_detail_tab(
    app: &tauri::AppHandle,
    params: String,
    title: String,
) -> Result<DocumentTabInfo, String> {
    let idx = TAB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let label = format!("document-tab-university-detail-{}", idx);
    let target = format!("{}-ct", label);
    let kind = detail_kind(&params);
    let url = detail_webview_url(&target, &params);
    let reopen = Some(serde_json::json!({
        "type": "detail",
        "params": params,
        "title": title,
    }));
    open_tab(
        app,
        None,
        Some(label),
        Some(target),
        tauri::WebviewUrl::App(url.clone().into()),
        url,
        title,
        kind.to_string(),
        &[],
        reopen,
    )
}

pub fn open_files_tab(
    app: &tauri::AppHandle,
    course: Option<String>,
    title: String,
) -> Result<DocumentTabInfo, String> {
    let suffix = match course.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(c) => format!("&course={}", urlencoding::encode(c)),
        None => String::new(),
    };
    // Keyed so re-opening focuses the single existing files tab instead of
    // stacking duplicates; the caller re-emits focus-course on reuse.
    open_app_surface_tab(app, "files", "files", title, Some("files"), &suffix)
}

/// Open (or focus) the Detective ("なるほど") game as a singleton tab in the
/// Copilot window. Keyed on "detective" so re-launching just re-focuses it.
pub fn open_detective_tab(app: &tauri::AppHandle) -> Result<DocumentTabInfo, String> {
    open_app_surface_tab(
        app,
        "detective",
        "detective",
        "なるほど".to_string(),
        Some("detective"),
        "",
    )
}

pub fn open_external_tab(
    app: &tauri::AppHandle,
    url: String,
    title: Option<String>,
) -> Result<crate::webview_toolbar::BrowserWindowInfo, String> {
    open_external_tab_with_scripts(app, url, title, &[])
}

pub fn open_external_tab_with_scripts(
    app: &tauri::AppHandle,
    url: String,
    title: Option<String>,
    init_scripts: &[&str],
) -> Result<crate::webview_toolbar::BrowserWindowInfo, String> {
    let parsed: url::Url = url.parse().map_err(|e| format!("URL parse error: {}", e))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("Unsupported URL scheme: {}", scheme));
    }
    let title = title.unwrap_or_else(|| parsed.host_str().unwrap_or("Web").to_string());
    let reopen = Some(serde_json::json!({ "type": "browser", "url": url }));
    let tab = open_tab(
        app,
        None,
        None,
        None,
        tauri::WebviewUrl::External(parsed),
        url.clone(),
        title.clone(),
        "browser".to_string(),
        init_scripts,
        reopen,
    )?;
    Ok(crate::webview_toolbar::BrowserWindowInfo {
        label: tab.label,
        target: tab.target,
        url,
        title,
        kind: "browser".to_string(),
    })
}

#[tauri::command]
pub fn document_tabs_list(owner: Option<String>) -> Vec<DocumentTabInfo> {
    list_tabs_for_owner(owner.as_deref().unwrap_or(OWNER_LABEL))
}

#[tauri::command]
pub fn document_tabs_set_controls(
    app: tauri::AppHandle,
    owner: Option<String>,
    target: Option<String>,
    controls: Vec<DocumentTabControl>,
) -> Result<(), String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    let active_target = active_tab_for_owner(&owner).map(|tab| tab.target);
    let target = target
        .or(active_target)
        .ok_or_else(|| "アクティブなタブがありません".to_string())?;
    let mut found = false;
    {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = states.get_mut(&owner) {
            if let Some(tab) = state
                .tabs
                .iter_mut()
                .find(|tab| tab.target == target || tab.id == target || tab.label == target)
            {
                tab.controls = controls;
                found = true;
            }
        }
    }
    if !found {
        return Err(format!("タブが見つかりません: {}", target));
    }
    emit_tabs_changed(&app, &owner);
    Ok(())
}

#[tauri::command]
pub fn document_tabs_report_title(app: tauri::AppHandle, target: String, title: String) {
    let title = title.trim().to_string();
    if title.is_empty() {
        return;
    }
    let mut changed_owner: Option<String> = None;
    let mut is_active = false;
    {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        for (owner, state) in states.iter_mut() {
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.target == target) {
                // Only browser tabs carry a placeholder host title; surface tabs
                // (reader/detail/files/home) manage their own labels.
                if tab.kind == "browser" && tab.title != title {
                    is_active = state.active.as_deref() == Some(tab.id.as_str());
                    tab.title = title.clone();
                    changed_owner = Some(owner.clone());
                }
                break;
            }
        }
    }
    if let Some(owner) = changed_owner {
        if is_active {
            crate::webview_toolbar::set_owner_active_target(&owner, &target, &title, "browser");
            if let Some(window) = app.get_window(&owner) {
                let _ = window.set_title(&title);
            }
        }
        emit_tabs_changed(&app, &owner);
    }
}

#[tauri::command]
pub fn document_tabs_send_control(
    app: tauri::AppHandle,
    owner: Option<String>,
    action: String,
    payload: Option<serde_json::Value>,
) -> Result<(), String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    let tab =
        active_tab_for_owner(&owner).ok_or_else(|| "アクティブなタブがありません".to_string())?;
    let mut targets = vec![tab.target.clone()];
    if action == "detail.refresh" {
        targets.extend(pane_chain(&tab.child).into_iter().map(|(target, _)| target));
    }
    let payload = payload.unwrap_or(serde_json::Value::Null);
    for target in targets {
        app.emit_to(
            tauri::EventTarget::AnyLabel {
                label: target.clone(),
            },
            "document-tab-control",
            serde_json::json!({
                "owner": owner,
                "target": target,
                "tabId": tab.id,
                "action": action,
                "payload": payload,
            }),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn document_tabs_report_probe(report: DocumentTabProbeReport) {
    let suspicious = report.kind == "detail"
        && (!report.has_content
            || report.content_children <= 0
            || report.content_text_length <= 0
            || (report.has_loading && !report.has_detail_wrap));
    if suspicious {
        log::warn!(
            "document tab probe suspicious owner={} id={} label={} target={} title={} kind={} ready={} visibility={} url={} href={} content={} children={} text={} html={} loading={} error={} detailWrap={} pageTitle={} viewport={}x{} body={}x{} preview={:?}",
            report.owner,
            report.id,
            report.label,
            report.target,
            report.title,
            report.kind,
            report.ready_state,
            report.visibility_state,
            report.url,
            report.href,
            report.has_content,
            report.content_children,
            report.content_text_length,
            report.content_html_length,
            report.has_loading,
            report.has_error,
            report.has_detail_wrap,
            report.has_page_title,
            report.viewport_width,
            report.viewport_height,
            report.body_width,
            report.body_height,
            report.preview,
        );
    } else {
        log::info!(
            "document tab probe owner={} id={} label={} target={} title={} kind={} ready={} visibility={} url={} href={} content={} children={} text={} html={} loading={} error={} detailWrap={} pageTitle={} viewport={}x{} body={}x{} preview={:?}",
            report.owner,
            report.id,
            report.label,
            report.target,
            report.title,
            report.kind,
            report.ready_state,
            report.visibility_state,
            report.url,
            report.href,
            report.has_content,
            report.content_children,
            report.content_text_length,
            report.content_html_length,
            report.has_loading,
            report.has_error,
            report.has_detail_wrap,
            report.has_page_title,
            report.viewport_width,
            report.viewport_height,
            report.body_width,
            report.body_height,
            report.preview,
        );
    }
}

#[tauri::command]
pub fn document_tabs_activate(
    app: tauri::AppHandle,
    owner: Option<String>,
    id: String,
) -> Result<(), String> {
    activate_tab_inner(&app, owner.as_deref().unwrap_or(OWNER_LABEL), &id)
}

/// Bring the Copilot window to front (showing it if hidden) and, if an id is
/// given, activate that tab. Used by the sidebar Copilot dock in the main window.
#[tauri::command]
pub fn document_tabs_reveal(
    app: tauri::AppHandle,
    owner: Option<String>,
    id: Option<String>,
) -> Result<(), String> {
    let owner = owner.as_deref().unwrap_or(OWNER_LABEL);
    // Reveal only shows an existing (possibly hidden) window — it never conjures
    // an empty one. Creating windows is document_tabs_new_tab's job.
    if app.get_window(owner).is_none() {
        return Ok(());
    }
    ensure_window(&app, owner)?;
    if let Some(id) = id {
        activate_tab_inner(&app, owner, &id)?;
    }
    Ok(())
}

#[tauri::command]
pub fn document_tabs_close(
    app: tauri::AppHandle,
    owner: Option<String>,
    id: String,
) -> Result<(), String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    let (closed, next_active) = {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states
            .get_mut(&owner)
            .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
        let Some(index) = state
            .tabs
            .iter()
            .position(|tab| tab.id == id || tab.target == id || tab.label == id)
        else {
            return Err(format!("タブが見つかりません: {}", id));
        };
        let closed = state.tabs.remove(index);
        let next_active = if state.active.as_deref() == Some(closed.id.as_str()) {
            state
                .tabs
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|i| state.tabs.get(i)))
                .map(|tab| tab.id.clone())
        } else {
            state.active.clone()
        };
        state.active = next_active.clone();
        (closed, next_active)
    };

    crate::webview_toolbar::unregister_readable_label(&closed.label);
    if let Some(webview) = app.get_webview(&closed.target) {
        let _ = webview.close();
    }
    // Tear down all split panes along with their parent tab.
    for (target, label) in pane_chain(&closed.child) {
        crate::webview_toolbar::unregister_readable_label(&label);
        if let Some(child_view) = app.get_webview(&target) {
            let _ = child_view.close();
        }
    }
    close_split_dividers(&app, &closed.target, 0);
    if closed.kind == "browser" {
        // Browser controls live in the shared Svelte tab chrome.
    }

    if let Some(next) = next_active {
        activate_tab_inner(&app, &owner, &next)?;
    } else if AGENT_PANEL_OPEN.load(Ordering::Relaxed) {
        resize_current_for_owner(&app, &owner)?;
        emit_tabs_changed(&app, &owner);
    } else if let Some(window) = app.get_window(&owner) {
        force_close_window(&window);
        DOCUMENT_WINDOWS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&owner);
        emit_tabs_changed(&app, &owner);
    }
    Ok(())
}

#[tauri::command]
pub fn document_tabs_new_tab(app: tauri::AppHandle) -> Result<DocumentTabInfo, String> {
    open_new_tab(&app)
}

#[tauri::command]
pub fn document_tabs_reorder(
    app: tauri::AppHandle,
    owner: Option<String>,
    ids: Vec<String>,
) -> Result<(), String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states
            .get_mut(&owner)
            .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
        // Rebuild the tab order to follow `ids`; ids not found are skipped and any
        // tabs missing from `ids` are appended so nothing is ever dropped.
        let mut reordered: Vec<DocumentTab> = Vec::with_capacity(state.tabs.len());
        for id in &ids {
            if let Some(pos) = state.tabs.iter().position(|tab| &tab.id == id) {
                reordered.push(state.tabs.remove(pos));
            }
        }
        reordered.append(&mut state.tabs);
        state.tabs = reordered;
    }
    emit_tabs_changed(&app, &owner);
    Ok(())
}

#[tauri::command]
pub fn document_tabs_close_split(
    app: tauri::AppHandle,
    owner: Option<String>,
    parent: Option<String>,
) -> Result<(), String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    let parent_id = match parent {
        Some(id) => id,
        None => active_tab_for_owner(&owner)
            .map(|tab| tab.id)
            .ok_or_else(|| "アクティブなタブがありません".to_string())?,
    };
    close_child_pane(&app, &owner, &parent_id)
}

/// Close one split pane (identified by its webview target) and any panes nested
/// below it, leaving the rest of the split intact.
#[tauri::command]
pub fn document_tabs_close_pane(
    app: tauri::AppHandle,
    owner: Option<String>,
    target: String,
) -> Result<(), String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    let removed = {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states
            .get_mut(&owner)
            .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
        let mut removed = Vec::new();
        for tab in state.tabs.iter_mut() {
            if let Some(panes) = truncate_pane_at(&mut tab.child, &target) {
                removed = panes;
                let columns = 1 + pane_chain(&tab.child).len();
                tab.split_weights = if columns <= 1 {
                    Vec::new()
                } else {
                    clamped_split_weights(&tab.split_weights, columns)
                };
                break;
            }
        }
        removed
    };
    for (pane_target, label) in removed {
        crate::webview_toolbar::unregister_readable_label(&label);
        if let Some(webview) = app.get_webview(&pane_target) {
            let _ = webview.close();
        }
    }
    resize_current_for_owner(&app, &owner)?;
    emit_tabs_changed(&app, &owner);
    Ok(())
}

#[tauri::command]
pub fn document_tabs_resize_split(
    app: tauri::AppHandle,
    owner: Option<String>,
    parent: Option<String>,
    ratios: Vec<f64>,
) -> Result<Vec<f64>, String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    let parent_id = match parent {
        Some(id) => id,
        None => active_tab_for_owner(&owner)
            .map(|tab| tab.id)
            .ok_or_else(|| "アクティブなタブがありません".to_string())?,
    };
    let applied = {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states
            .get_mut(&owner)
            .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
        let tab = state
            .tabs
            .iter_mut()
            .find(|tab| tab.id == parent_id || tab.target == parent_id || tab.label == parent_id)
            .ok_or_else(|| format!("タブが見つかりません: {}", parent_id))?;
        let columns = 1 + pane_chain(&tab.child).len();
        if columns <= 1 {
            return Err("分割ペインがありません".to_string());
        }
        let next = clamped_split_weights(&ratios, columns);
        tab.split_weights = next.clone();
        next
    };
    resize_current_for_owner(&app, &owner)?;
    emit_tabs_changed(&app, &owner);
    Ok(applied)
}

#[tauri::command]
pub fn document_tabs_drag_split(
    app: tauri::AppHandle,
    owner: Option<String>,
    parent: Option<String>,
    handle_index: usize,
    base_ratios: Vec<f64>,
    delta_px: f64,
) -> Result<Vec<f64>, String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    let parent_id = match parent {
        Some(id) => id,
        None => active_tab_for_owner(&owner)
            .map(|tab| tab.id)
            .ok_or_else(|| "アクティブなタブがありません".to_string())?,
    };
    let content_width = content_width_for_owner(&app, &owner)?;
    let applied = {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states
            .get_mut(&owner)
            .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
        let tab = state
            .tabs
            .iter_mut()
            .find(|tab| tab.id == parent_id || tab.target == parent_id || tab.label == parent_id)
            .ok_or_else(|| format!("タブが見つかりません: {}", parent_id))?;
        let columns = 1 + pane_chain(&tab.child).len();
        if columns <= 1 {
            return Err("分割ペインがありません".to_string());
        }
        if handle_index >= columns.saturating_sub(1) {
            return Err(format!("無効な分割バー index: {}", handle_index));
        }
        let usable_width =
            (content_width - SPLIT_DIVIDER_WIDTH * (columns.saturating_sub(1)) as f64).max(120.0);
        let delta_ratio = if usable_width <= f64::EPSILON {
            0.0
        } else {
            delta_px / usable_width
        };
        let min_ratio = MIN_SPLIT_RATIO;
        let mut w = clamped_split_weights(&base_ratios, columns);
        let i = handle_index;
        const EPS: f64 = 1e-9;
        if delta_ratio >= 0.0 {
            // Drag right: grow pane i, taking space from panes to its right,
            // cascading to the next one each time one bottoms out at the minimum.
            let mut need = delta_ratio;
            let mut j = i + 1;
            while need > EPS && j < columns {
                let take = (w[j] - min_ratio).max(0.0).min(need);
                w[j] -= take;
                w[i] += take;
                need -= take;
                j += 1;
            }
        } else {
            // Drag left: grow pane i+1, taking space from panes to its left
            // (pane i, then i-1, …), cascading through the minimum.
            let mut need = -delta_ratio;
            let mut j = i as isize;
            while need > EPS && j >= 0 {
                let idx = j as usize;
                let take = (w[idx] - min_ratio).max(0.0).min(need);
                w[idx] -= take;
                w[i + 1] += take;
                need -= take;
                j -= 1;
            }
        }
        let next = clamped_split_weights(&w, columns);
        tab.split_weights = next.clone();
        next
    };
    resize_current_for_owner(&app, &owner)?;
    emit_tabs_changed(&app, &owner);
    Ok(applied)
}

#[tauri::command]
pub fn document_tabs_begin_split_drag(
    app: tauri::AppHandle,
    owner: Option<String>,
    parent: Option<String>,
    handle_index: usize,
) -> Result<(), String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    let parent_id = match parent {
        Some(id) => id,
        None => active_tab_for_owner(&owner)
            .map(|tab| tab.id)
            .ok_or_else(|| "アクティブなタブがありません".to_string())?,
    };
    {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states
            .get_mut(&owner)
            .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
        let tab = state
            .tabs
            .iter_mut()
            .find(|tab| tab.id == parent_id || tab.target == parent_id || tab.label == parent_id)
            .ok_or_else(|| format!("タブが見つかりません: {}", parent_id))?;
        let columns = 1 + pane_chain(&tab.child).len();
        if handle_index >= columns.saturating_sub(1) {
            return Err(format!("無効な分割バー index: {}", handle_index));
        }
        tab.active_split_drag = Some(handle_index);
    }
    resize_current_for_owner(&app, &owner)?;
    emit_tabs_changed(&app, &owner);
    Ok(())
}

#[tauri::command]
pub fn document_tabs_end_split_drag(
    app: tauri::AppHandle,
    owner: Option<String>,
    parent: Option<String>,
) -> Result<(), String> {
    let owner = owner.unwrap_or_else(|| OWNER_LABEL.to_string());
    let parent_id = match parent {
        Some(id) => id,
        None => active_tab_for_owner(&owner)
            .map(|tab| tab.id)
            .ok_or_else(|| "アクティブなタブがありません".to_string())?,
    };
    {
        let mut states = DOCUMENT_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let state = states
            .get_mut(&owner)
            .ok_or_else(|| format!("タブウィンドウが見つかりません: {}", owner))?;
        let tab = state
            .tabs
            .iter_mut()
            .find(|tab| tab.id == parent_id || tab.target == parent_id || tab.label == parent_id)
            .ok_or_else(|| format!("タブが見つかりません: {}", parent_id))?;
        tab.active_split_drag = None;
    }
    resize_current_for_owner(&app, &owner)?;
    emit_tabs_changed(&app, &owner);
    Ok(())
}

#[tauri::command]
pub async fn document_tabs_open_bookmark(
    app: tauri::AppHandle,
    spec: serde_json::Value,
) -> Result<(), String> {
    let kind = spec.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "browser" => {
            let url = spec
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("ブックマークにURLがありません")?;
            open_external_tab(&app, url.to_string(), None).map(|_| ())
        }
        "detail" => {
            let params = spec
                .get("params")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = spec
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("詳細")
                .to_string();
            open_university_detail_tab(&app, params, title).map(|_| ())
        }
        "reader" => {
            let path = spec
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("ブックマークにパスがありません")?;
            crate::commands::open_markdown_file_window(app.clone(), path.to_string()).await
        }
        other => Err(format!("未対応のブックマーク種別: {}", other)),
    }
}

#[tauri::command]
pub fn document_tabs_open_agent(
    app: tauri::AppHandle,
    _owner: Option<String>,
) -> Result<(), String> {
    if AGENT_PANEL_OPEN.load(Ordering::Relaxed) {
        close_agent_panel(&app)
    } else {
        open_agent_workspace(&app)
    }
}

#[tauri::command]
pub fn document_tabs_close_agent(app: tauri::AppHandle) -> Result<(), String> {
    close_agent_panel(&app)
}

#[tauri::command]
pub fn document_tabs_agent_is_open() -> bool {
    AGENT_PANEL_OPEN.load(Ordering::Relaxed)
}
