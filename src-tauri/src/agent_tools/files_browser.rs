use super::*;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static DOCX_PARA_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"</w:p>").unwrap());
static DOCX_BREAK_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<w:br\s*/?>").unwrap());
static DOCX_TAB_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<w:tab\s*/?>").unwrap());
static DOCX_TAG_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{}…<truncated>", truncated)
}

fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn normalize_extracted_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_blank = false;
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_blank && !out.is_empty() {
                out.push('\n');
            }
            prev_blank = true;
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(trimmed);
        prev_blank = false;
    }
    out.trim().to_string()
}

fn compact_text(s: &str, max_chars: usize) -> Option<String> {
    let normalized = normalize_extracted_text(s);
    if normalized.is_empty() {
        None
    } else {
        Some(truncate_chars(&normalized, max_chars))
    }
}

fn compact_string_list(items: &[String], max_items: usize, max_chars: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let Some(value) = compact_text(item, max_chars) else {
            continue;
        };
        let key = value.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(value);
        if out.len() >= max_items {
            break;
        }
    }
    out
}

fn allowed_download_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(crate::commands::default_download_dir());
    let cfg = crate::commands::load_download_config();
    if !cfg.download_dir.is_empty() {
        roots.push(PathBuf::from(cfg.download_dir));
    }
    let mut uniq = Vec::new();
    for root in roots {
        let canonical = root.canonicalize().unwrap_or(root);
        if !uniq.iter().any(|p: &PathBuf| p == &canonical) {
            uniq.push(canonical);
        }
    }
    uniq
}

fn resolve_allowed_download_path(raw_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw_path);
    if !path.is_absolute() {
        return Err("絶対パスのファイルのみ指定できます".into());
    }
    let canonical = if path.exists() {
        path.canonicalize()
            .map_err(|e| format!("ファイルパスを解決できません: {}", e))?
    } else if let Some(parent) = path.parent() {
        if parent.exists() {
            let parent_canonical = parent
                .canonicalize()
                .map_err(|e| format!("親ディレクトリを解決できません: {}", e))?;
            if let Some(file_name) = path.file_name() {
                parent_canonical.join(file_name)
            } else {
                return Err("ファイル名が不正です".into());
            }
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };
    let allowed = allowed_download_roots()
        .into_iter()
        .any(|root| canonical.starts_with(&root));
    if !allowed {
        return Err("ダウンロードフォルダ外のファイルは読めません".into());
    }
    Ok(canonical)
}

fn file_extension_lower(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn supported_read_extension(ext: &str) -> bool {
    matches!(
        ext,
        "pdf" | "docx" | "txt" | "md" | "json" | "csv" | "html" | "htm"
    )
}

fn supported_write_extension(ext: &str) -> bool {
    matches!(ext, "txt" | "md" | "json" | "csv" | "html" | "htm")
}

fn read_utf8ish_file(path: &Path, max_bytes: usize) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("ファイル情報取得失敗: {}", e))?;
    if metadata.len() as usize > max_bytes {
        return Err(format!("ファイルが大きすぎます ({} bytes)", metadata.len()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("ファイル読み込み失敗: {}", e))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn extract_pdf_text(path: &Path) -> Result<String, String> {
    let doc = lopdf::Document::load(path).map_err(|e| format!("PDF読み込み失敗: {}", e))?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err("PDFにページがありません".into());
    }
    let mut out = String::new();
    for page_num in pages.keys().take(20) {
        match doc.extract_text(&[*page_num]) {
            Ok(text) => {
                if !text.trim().is_empty() {
                    if !out.is_empty() {
                        out.push_str("\n\n");
                    }
                    out.push_str(&text);
                }
            }
            Err(e) => {
                log::warn!("pdf text extraction failed for page {}: {}", page_num, e);
            }
        }
    }
    let text = normalize_extracted_text(&out);
    if text.is_empty() {
        Err("PDFからテキストを抽出できませんでした".into())
    } else {
        Ok(text)
    }
}

fn extract_docx_text(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("DOCX読み込み失敗: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("DOCX展開失敗: {}", e))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| format!("DOCX本文が見つかりません: {}", e))?
        .read_to_string(&mut xml)
        .map_err(|e| format!("DOCX本文読み込み失敗: {}", e))?;

    let xml = DOCX_PARA_RE.replace_all(&xml, "\n");
    let xml = DOCX_BREAK_RE.replace_all(&xml, "\n");
    let xml = DOCX_TAB_RE.replace_all(&xml, "\t");
    let text = DOCX_TAG_RE.replace_all(&xml, " ");
    let text = decode_xml_entities(&text);
    let text = normalize_extracted_text(&text);
    if text.is_empty() {
        Err("DOCXからテキストを抽出できませんでした".into())
    } else {
        Ok(text)
    }
}

fn read_supported_download_file(path: &Path) -> Result<String, String> {
    let ext = file_extension_lower(path);
    match ext.as_str() {
        "pdf" => extract_pdf_text(path),
        "docx" => extract_docx_text(path),
        "txt" | "md" | "json" | "csv" | "html" | "htm" => {
            read_utf8ish_file(path, 2_000_000).map(|s| normalize_extracted_text(&s))
        }
        "doc" => Err("旧式 .doc は未対応です。.docx か PDF に変換してから試してください".into()),
        _ => Err(format!("未対応の拡張子です: .{}", ext)),
    }
}

pub(super) async fn list_downloaded_files(args: &Value) -> Result<Value, String> {
    let keyword = sanitize_text_arg(args, "keyword", 80).unwrap_or_default();
    let keyword_norm = normalize_text(&keyword);
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .min(LIST_CAP as u64) as usize;

    let mut records = crate::commands::list_downloads();
    records.retain(|r| r.file_exists);
    if !keyword_norm.is_empty() {
        records.retain(|r| {
            let hay = normalize_text(&format!("{} {} {}", r.filename, r.course_name, r.path));
            hay.contains(&keyword_norm)
        });
    }

    let files: Vec<Value> = records
        .into_iter()
        .take(limit)
        .map(|r| {
            json!({
                "filename": r.filename,
                "path": r.path,
                "course_name": r.course_name,
                "source": r.source,
                "size_bytes": r.size_bytes,
                "downloaded_at": r.downloaded_at,
            })
        })
        .collect();

    Ok(json!({
        "keyword": keyword,
        "files": files,
    }))
}

pub(super) async fn read_downloaded_file(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let mut path = resolve_downloaded_file_arg(args)?;
    if !path.exists() {
        if let Ok(downloaded_path) = auto_download_missing_file(app, &path).await {
            path = downloaded_path;
        } else {
            return Err(format!(
                "ファイルが見つかりません。自動ダウンロードも失敗しました。物理パス: {:?}",
                path
            ));
        }
    }
    let ext = file_extension_lower(&path);
    if !supported_read_extension(&ext) && ext != "doc" {
        return Err(format!("未対応の拡張子です: .{}", ext));
    }
    let metadata = std::fs::metadata(&path).map_err(|e| format!("ファイル情報取得失敗: {}", e))?;
    let text = read_supported_download_file(&path)?;
    Ok(json!({
        "path": path.to_string_lossy(),
        "filename": path.file_name().and_then(|n| n.to_str()).unwrap_or_default(),
        "extension": ext,
        "size_bytes": metadata.len(),
        "content": truncate_chars(&text, 12_000),
    }))
}

fn resolve_downloaded_file_arg(args: &Value) -> Result<PathBuf, String> {
    let raw_path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if !raw_path.is_empty() {
        return resolve_allowed_download_path(raw_path);
    }

    let filename = args
        .get("filename")
        .or_else(|| args.get("file_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if filename.is_empty() {
        return Err("path または filename を指定してください".into());
    }
    if filename.contains('\0') || filename.contains('/') || filename.contains('\\') {
        return Err("filename が不正です".into());
    }

    let course_hint = args
        .get("course_name")
        .or_else(|| args.get("course"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let filename_norm = normalize_text(filename);
    let course_norm = normalize_text(course_hint);
    let mut records = crate::commands::list_downloads();
    records.retain(|r| r.file_exists);
    if !course_norm.is_empty() {
        records.retain(|r| normalize_text(&r.course_name).contains(&course_norm));
    }

    records
        .iter()
        .find(|r| r.filename == filename)
        .or_else(|| {
            records
                .iter()
                .find(|r| normalize_text(&r.filename) == filename_norm)
        })
        .or_else(|| {
            records
                .iter()
                .find(|r| normalize_text(&r.filename).contains(&filename_norm))
        })
        .map(|r| PathBuf::from(&r.path))
        .ok_or_else(|| {
            format!(
                "filename に一致するダウンロード済みファイルが見つかりません: {}",
                filename
            )
        })
}

pub(super) async fn write_downloaded_text_file(args: &Value) -> Result<Value, String> {
    let raw_path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if raw_path.is_empty() {
        return Err("pathを指定してください".into());
    }
    if content.is_empty() {
        return Err("contentが空です".into());
    }
    let path = resolve_allowed_download_path(raw_path)?;
    let ext = file_extension_lower(&path);
    if !supported_write_extension(&ext) {
        return Err("書き込みできるのは .txt / .md / .json / .csv / .html のみです".into());
    }
    let metadata = std::fs::metadata(&path).map_err(|e| format!("ファイル情報取得失敗: {}", e))?;
    if metadata.len() > 2_000_000 {
        return Err("大きすぎるファイルは編集できません".into());
    }
    std::fs::write(&path, content).map_err(|e| format!("ファイル保存失敗: {}", e))?;
    Ok(json!({
        "path": path.to_string_lossy(),
        "bytes_written": content.len(),
        "status": "saved",
    }))
}

pub(super) async fn delete_downloaded_file(args: &Value) -> Result<Value, String> {
    let path = resolve_downloaded_file_arg(args)?;
    if !path.is_file() {
        return Err("対象はファイルではありません".into());
    }
    let metadata = std::fs::metadata(&path).map_err(|e| format!("ファイル情報取得失敗: {}", e))?;
    std::fs::remove_file(&path).map_err(|e| format!("ファイル削除失敗: {}", e))?;
    Ok(json!({
        "status": "deleted",
        "path": path.to_string_lossy(),
        "filename": path.file_name().and_then(|n| n.to_str()).unwrap_or_default(),
        "size_bytes": metadata.len(),
    }))
}

pub(super) async fn open_downloaded_file(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let mut path = resolve_downloaded_file_arg(args)?;
    if !path.exists() {
        if let Ok(downloaded_path) = auto_download_missing_file(app, &path).await {
            path = downloaded_path;
        } else {
            return Err(format!(
                "ファイルが見つかりません。自動ダウンロードも失敗しました。物理パス: {:?}",
                path
            ));
        }
    }
    crate::commands::open_downloaded_file(app.clone(), path.to_string_lossy().to_string()).await?;
    Ok(json!({
        "status": "opened",
        "path": path.to_string_lossy(),
        "filename": path.file_name().and_then(|n| n.to_str()).unwrap_or_default(),
    }))
}

pub(super) async fn fetch_luna_detail_html_cached(
    app: &tauri::AppHandle,
    detail_path: &str,
) -> Result<String, String> {
    fetch_luna_detail_html_inner(app, detail_path)
        .await
        .map(|(html, _age)| html)
}

/// Returns (html, cache_age_secs). cache_age_secs = 0 means it was just fetched from network.
pub(super) async fn fetch_luna_detail_html_with_age(
    app: &tauri::AppHandle,
    detail_path: &str,
) -> Result<(String, i64), String> {
    fetch_luna_detail_html_inner(app, detail_path).await
}

async fn fetch_luna_detail_html_inner(
    app: &tauri::AppHandle,
    detail_path: &str,
) -> Result<(String, i64), String> {
    let db = app.state::<Database>();
    let cache_key = format!("luna_detail_html:{}", detail_path);

    // Check SQLite cache first
    if let Ok(Some((html, updated_at))) = db.get_data_cache(&cache_key) {
        let now = crate::db::epoch_secs();
        let age = now - updated_at;
        // If the cache is within 7 days, let's use it directly to save API hits & allow offline resolve
        if age < 7 * 24 * 3600 {
            log::debug!("Cache hit for Luna details of {}", detail_path);
            return Ok((html, age));
        }

        // Try requesting online, but if session expired/offline, fallback to the expired cache instead of breaking!
        let luna_state = app.state::<crate::LunaState>();
        let http_opt = {
            let luna = luna_state.client.lock().await;
            if luna.authenticated {
                Some(luna.http.clone())
            } else {
                None
            }
        };

        if let Some(http) = http_opt {
            let url = format!("{}{}", crate::config::LUNA_BASE, detail_path);
            if let Ok(fresh_html) = crate::client::fetch_with_redirect(
                &http,
                &url,
                crate::config::LUNA_BASE,
                crate::luna_client::LUNA_SESSION_EXPIRED_MSG,
                crate::luna_client::is_luna_session_expired,
            )
            .await
            {
                let _ = db.save_data_cache(&cache_key, &fresh_html);
                return Ok((fresh_html, 0));
            }
        }
        log::warn!(
            "Failed online fetch for {}, falling back to expired cached HTML",
            detail_path
        );
        return Ok((html, age));
    }

    // Cache miss, must resolve online
    let luna_state = app.state::<crate::LunaState>();
    let http = {
        let luna = luna_state.client.lock().await;
        if !luna.authenticated {
            return Err(crate::luna_client::LUNA_AUTH_REQUIRED_MSG.into());
        }
        luna.http.clone()
    };

    let url = format!("{}{}", crate::config::LUNA_BASE, detail_path);
    let html = crate::client::fetch_with_redirect(
        &http,
        &url,
        crate::config::LUNA_BASE,
        crate::luna_client::LUNA_SESSION_EXPIRED_MSG,
        crate::luna_client::is_luna_session_expired,
    )
    .await
    .map_err(|e| format!("Luna取得失敗: {}", e))?;

    let _ = db.save_data_cache(&cache_key, &html);
    Ok((html, 0))
}

struct LunaAttachmentResolved {
    title: String,
    course_name: String,
    detail_path: String,
    detail_url: String,
    attachment: crate::luna_parser::LunaAttachment,
}

struct SavedLunaAttachment {
    saved_path: String,
}

async fn download_resolved_luna_attachment(
    app: &tauri::AppHandle,
    resolved: &LunaAttachmentResolved,
) -> Result<SavedLunaAttachment, String> {
    let bytes = fetch_luna_attachment_bytes(app, &resolved.attachment).await?;
    if bytes.is_empty() {
        return Err("添付データが空です".into());
    }
    let saved_path = crate::luna_commands::save_to_downloads(
        &resolved.attachment.name,
        &bytes,
        Some(&resolved.course_name),
    )?;
    Ok(SavedLunaAttachment { saved_path })
}

async fn fetch_luna_attachment_bytes(
    app: &tauri::AppHandle,
    attachment: &crate::luna_parser::LunaAttachment,
) -> Result<Vec<u8>, String> {
    let luna_state = app.state::<crate::LunaState>();
    let http = {
        let luna = luna_state.client.lock().await;
        if !luna.authenticated {
            return Err(crate::luna_client::LUNA_AUTH_REQUIRED_MSG.into());
        }
        luna.http.clone()
    };

    let download_url = if attachment.url.is_empty() {
        let action = attachment.download_action.as_str();
        if action.is_empty() {
            return Err("添付のダウンロード情報が不足しています".into());
        }
        let params = attachment
            .download_params
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    crate::luna_commands::form_encode(k),
                    crate::luna_commands::form_encode(v)
                )
            })
            .collect::<Vec<_>>()
            .join("&");
        let path_name = crate::luna_commands::make_down_file_name(&attachment.name);
        format!("{}/{}?{}", action, path_name, params)
    } else {
        attachment.url.clone()
    };

    crate::luna_commands::luna_download(&http, &download_url).await
}

async fn resolve_luna_attachment(
    app: &tauri::AppHandle,
    title: &str,
    attachment_name: &str,
) -> Result<LunaAttachmentResolved, String> {
    resolve_luna_attachment_with_lid(app, title, attachment_name, "").await
}

async fn resolve_luna_attachment_with_lid(
    app: &tauri::AppHandle,
    title: &str,
    attachment_name: &str,
    luna_id_filter: &str,
) -> Result<LunaAttachmentResolved, String> {
    let db = app.state::<Database>();
    let acts = db.get_all_luna_activities().unwrap_or_default();

    // Filter by luna_id if provided
    let filtered_acts: Vec<_> = if !luna_id_filter.is_empty() {
        acts.into_iter()
            .filter(|a| a.luna_id == luna_id_filter)
            .collect()
    } else {
        acts
    };

    let needle = title.to_lowercase();
    let row = filtered_acts
        .iter()
        .find(|a| a.title == title)
        .or_else(|| {
            filtered_acts
                .iter()
                .find(|a| a.title.to_lowercase().contains(&needle))
        })
        .or_else(|| {
            filtered_acts
                .iter()
                .find(|a| needle.contains(&a.title.to_lowercase()) && !a.title.is_empty())
        })
        .ok_or_else(|| format!("「{}」に一致する活動が見つかりません", title))?;
    if row.detail_path.is_empty() {
        return Err(format!("「{}」には詳細ページのパスがありません", row.title));
    }

    let luna_courses = db.get_luna_courses().unwrap_or_default();
    let course_name = luna_courses
        .iter()
        .find(|c| c.luna_id == row.luna_id)
        .map(|c| c.name.clone())
        .unwrap_or_default();

    let (html, cache_age) = fetch_luna_detail_html_with_age(app, &row.detail_path).await?;
    let detail_url = format!("{}{}", crate::config::LUNA_BASE, row.detail_path);

    let parse_detail = |h: &str| -> crate::luna_parser::LunaDetailPage {
        if row.activity_type == "announcement" {
            crate::luna_parser::parse_luna_announcement_detail(h)
        } else {
            crate::luna_parser::parse_luna_detail_page(h)
        }
    };

    let mut detail = parse_detail(&html);

    // If the cached page yielded no attachments AND the cache is not brand-new,
    // force a fresh fetch — the cache may have been stored before the attachment was uploaded.
    // Skip the re-fetch when age ≤ 60 s: the page was just refreshed and has no attachments.
    if detail.attachments.is_empty() && cache_age > 60 {
        log::debug!(
            "No attachments in cached HTML (age={}s) for '{}', forcing fresh fetch",
            cache_age,
            row.detail_path
        );
        let db = app.state::<Database>();
        let cache_key = format!("luna_detail_html:{}", row.detail_path);
        let _ = db.delete_data_cache(&cache_key);

        match fetch_luna_detail_html_cached(app, &row.detail_path).await {
            Ok(fresh_html) => {
                detail = parse_detail(&fresh_html);
            }
            Err(e) => {
                log::warn!("Fresh fetch failed for '{}': {}", row.detail_path, e);
                // Keep detail as-is (empty attachments) — error will surface below
            }
        }
    }

    let attachment = if attachment_name.is_empty() {
        detail.attachments.first()
    } else {
        let needle = attachment_name.to_lowercase();
        detail
            .attachments
            .iter()
            .find(|a| a.name == attachment_name)
            .or_else(|| {
                detail
                    .attachments
                    .iter()
                    .find(|a| a.name.to_lowercase().contains(&needle))
            })
            .or_else(|| {
                detail
                    .attachments
                    .iter()
                    .find(|a| needle.contains(&a.name.to_lowercase()))
            })
    }
    .cloned()
    .ok_or_else(|| {
        if attachment_name.is_empty() {
            format!("「{}」には開ける添付がありません", row.title)
        } else {
            format!(
                "「{}」の添付「{}」が見つかりません",
                row.title, attachment_name
            )
        }
    })?;

    Ok(LunaAttachmentResolved {
        title: row.title.clone(),
        course_name,
        detail_path: row.detail_path.clone(),
        detail_url,
        attachment,
    })
}

pub(super) async fn open_luna_attachment(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let attachment_name = args
        .get("attachment_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        return Err("titleを指定してください".into());
    }

    let resolved = resolve_luna_attachment(app, title, &attachment_name).await?;
    let attachment = &resolved.attachment;

    if attachment.url.starts_with("http") {
        crate::commands::open_external_url(
            app.clone(),
            attachment.url.clone(),
            Some(attachment.name.clone()),
        )
        .await?;
        return Ok(json!({
            "status": "opened_external",
            "title": resolved.title,
            "attachment_name": attachment.name,
            "url": attachment.url,
            "course": resolved.course_name,
            "source": { "service": "luna", "detail_path": resolved.detail_path, "detail_url": resolved.detail_url },
        }));
    }

    let saved = download_resolved_luna_attachment(app, &resolved).await?;
    let saved_path = saved.saved_path;
    crate::commands::open_downloaded_file(app.clone(), saved_path.clone()).await?;

    Ok(json!({
        "status": "downloaded_and_opened",
        "title": resolved.title,
        "attachment_name": attachment.name,
        "saved_path": saved_path,
        "course": resolved.course_name,
        "source": { "service": "luna", "detail_path": resolved.detail_path, "detail_url": resolved.detail_url },
    }))
}

pub(super) async fn download_luna_attachment(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let attachment_name = args
        .get("attachment_name")
        .or_else(|| args.get("filename"))
        .or_else(|| args.get("file_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let luna_id = args
        .get("luna_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    if title.is_empty() {
        return Err("titleを指定してください".into());
    }

    let resolved = resolve_luna_attachment_with_lid(app, title, &attachment_name, luna_id).await?;
    let attachment = &resolved.attachment;

    if attachment.url.starts_with("http") {
        return Ok(json!({
            "status": "external_url",
            "title": resolved.title,
            "attachment_name": attachment.name,
            "url": attachment.url,
            "course": resolved.course_name,
            "source": { "service": "luna", "detail_path": resolved.detail_path, "detail_url": resolved.detail_url },
        }));
    }

    let saved = download_resolved_luna_attachment(app, &resolved).await?;
    Ok(json!({
        "status": "downloaded",
        "title": resolved.title,
        "attachment_name": attachment.name,
        "saved_path": saved.saved_path,
        "course": resolved.course_name,
        "source": { "service": "luna", "detail_path": resolved.detail_path, "detail_url": resolved.detail_url },
    }))
}

#[derive(Clone)]
struct MatchedCourseMaterial {
    course_name: String,
    material_title: String,
    file: crate::luna_parser::LunaMaterialFile,
}

fn effective_material_filename(file: &crate::luna_parser::LunaMaterialFile) -> String {
    if file.file_name.trim().is_empty() {
        file.display_name.trim().to_string()
    } else {
        file.file_name.trim().to_string()
    }
}

fn loose_filename_match(candidate: &str, requested: &str) -> bool {
    let candidate = candidate.trim();
    let requested = requested.trim();
    if candidate.is_empty() || requested.is_empty() {
        return false;
    }
    if candidate == requested {
        return true;
    }
    let candidate_norm = normalize_text(candidate);
    let requested_norm = normalize_text(requested);
    !candidate_norm.is_empty()
        && !requested_norm.is_empty()
        && (candidate_norm == requested_norm
            || candidate_norm.contains(&requested_norm)
            || requested_norm.contains(&candidate_norm))
}

fn match_material_file(
    contents: &crate::luna_parser::LunaCourseContents,
    filename: &str,
) -> Option<MatchedCourseMaterial> {
    for material in &contents.materials {
        for file in &material.files {
            let effective_name = effective_material_filename(file);
            let candidates = [
                effective_name.as_str(),
                file.file_name.as_str(),
                file.display_name.as_str(),
                material.title.as_str(),
            ];
            if candidates
                .iter()
                .any(|candidate| loose_filename_match(candidate, filename))
            {
                return Some(MatchedCourseMaterial {
                    course_name: contents.course_name.clone(),
                    material_title: material.title.clone(),
                    file: file.clone(),
                });
            }
        }
    }
    None
}

fn cached_luna_course_contents(
    db: &Database,
    luna_id: &str,
) -> Option<crate::luna_parser::LunaCourseContents> {
    let cache_key = format!("luna_course:{}", luna_id);
    db.get_data_cache(&cache_key)
        .ok()
        .flatten()
        .and_then(|(json_str, _)| serde_json::from_str(&json_str).ok())
}

async fn fetch_luna_course_contents_for_download(
    app: &tauri::AppHandle,
    luna_id: &str,
) -> Result<crate::luna_parser::LunaCourseContents, String> {
    let luna_state = app.state::<crate::LunaState>();
    let http = {
        let luna = luna_state.client.lock().await;
        if !luna.authenticated {
            return Err(crate::luna_client::LUNA_AUTH_REQUIRED_MSG.into());
        }
        luna.http.clone()
    };

    let course_url = format!(
        "{}/lms/course?idnumber={}",
        crate::config::LUNA_BASE,
        luna_id
    );
    let contents_url = format!(
        "{}/lms/contents?idnumber={}",
        crate::config::LUNA_BASE,
        luna_id
    );
    let course_html = crate::client::fetch_with_redirect(
        &http,
        &course_url,
        crate::config::LUNA_BASE,
        crate::luna_client::LUNA_SESSION_EXPIRED_MSG,
        crate::luna_client::is_luna_session_expired,
    )
    .await
    .map_err(|e| format!("Luna course取得失敗: {}", e))?;
    let mut contents = crate::luna_parser::parse_luna_course_contents(&course_html, luna_id);

    let contents_html = crate::client::fetch_with_redirect(
        &http,
        &contents_url,
        crate::config::LUNA_BASE,
        crate::luna_client::LUNA_SESSION_EXPIRED_MSG,
        crate::luna_client::is_luna_session_expired,
    )
    .await
    .map_err(|e| format!("Luna contents取得失敗: {}", e))?;
    let (materials, reports, examinations, discussions, surveys) =
        crate::luna_parser::parse_luna_contents_page(&contents_html);
    contents.materials = materials;
    contents.reports = reports;
    contents.examinations = examinations;
    contents.discussions = discussions;
    contents.surveys = surveys;

    if let Ok(json) = serde_json::to_string(&contents) {
        let db = app.state::<Database>();
        let _ = db.save_data_cache(&format!("luna_course:{}", luna_id), &json);
    }
    Ok(contents)
}

async fn find_course_material_file(
    app: &tauri::AppHandle,
    luna_id: &str,
    filename: &str,
) -> Result<Option<MatchedCourseMaterial>, String> {
    let db = app.state::<Database>();
    if let Some(contents) = cached_luna_course_contents(&db, luna_id) {
        if let Some(matched) = match_material_file(&contents, filename) {
            return Ok(Some(matched));
        }
    }

    let fresh = fetch_luna_course_contents_for_download(app, luna_id).await?;
    Ok(match_material_file(&fresh, filename))
}

async fn download_course_material_from_contents(
    app: &tauri::AppHandle,
    luna_id: &str,
    filename: &str,
) -> Result<Option<Value>, String> {
    let Some(mut matched) = find_course_material_file(app, luna_id, filename).await? else {
        return Ok(None);
    };
    if matched.course_name.trim().is_empty() {
        let db = app.state::<Database>();
        matched.course_name = db
            .get_luna_courses()
            .unwrap_or_default()
            .into_iter()
            .find(|c| c.luna_id == luna_id)
            .map(|c| c.name)
            .unwrap_or_default();
    }

    let attachment_name = effective_material_filename(&matched.file);
    if !matched.file.external_url.trim().is_empty() {
        return Ok(Some(json!({
            "status": "external_url",
            "filename": attachment_name,
            "display_name": matched.file.display_name,
            "material_title": matched.material_title,
            "url": matched.file.external_url,
            "course": matched.course_name,
            "source": { "service": "luna", "luna_id": luna_id, "kind": "course_material" },
        })));
    }

    let luna_state = app.state::<crate::LunaState>();
    let saved_path = crate::luna_commands::download_luna_material_file(
        luna_state.inner(),
        luna_id,
        &matched.file,
        if matched.course_name.trim().is_empty() {
            None
        } else {
            Some(matched.course_name.as_str())
        },
    )
    .await?;
    Ok(Some(json!({
        "status": "downloaded",
        "filename": attachment_name,
        "display_name": matched.file.display_name,
        "material_title": matched.material_title,
        "saved_path": saved_path,
        "course": matched.course_name,
        "source": { "service": "luna", "luna_id": luna_id, "kind": "course_material" },
    })))
}

pub(super) async fn auto_download_missing_file(
    app: &tauri::AppHandle,
    path: &Path,
) -> Result<PathBuf, String> {
    if path.exists() {
        return Ok(path.to_path_buf());
    }
    log::info!(
        "File not found locally: {:?}. Attempting auto-download...",
        path
    );
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if filename.is_empty() {
        return Err("Filename is empty".into());
    }
    let parent = path.parent();
    let course_dir_name = parent
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    let db = app.state::<Database>();
    let acts = db.get_all_luna_activities().unwrap_or_default();

    let mut candidate_acts: Vec<_> = if !course_dir_name.is_empty() {
        let luna_courses = db.get_luna_courses().unwrap_or_default();
        let target_luna_ids: Vec<String> = luna_courses
            .iter()
            .filter(|c| {
                let simplified_db = crate::commands::simplify_course_name(&c.name);
                let simplified_dir = crate::commands::simplify_course_name(course_dir_name);
                simplified_db
                    .to_lowercase()
                    .contains(&simplified_dir.to_lowercase())
                    || simplified_dir
                        .to_lowercase()
                        .contains(&simplified_db.to_lowercase())
            })
            .map(|c| c.luna_id.clone())
            .collect();
        acts.into_iter()
            .filter(|a| target_luna_ids.contains(&a.luna_id))
            .collect()
    } else {
        acts
    };

    if candidate_acts.is_empty() {
        candidate_acts = db.get_all_luna_activities().unwrap_or_default();
    }

    log::info!(
        "Searching across {} candidate Luna activities for attachment '{}'",
        candidate_acts.len(),
        filename
    );

    let mut candidate_luna_ids = Vec::new();
    for act in &candidate_acts {
        if !candidate_luna_ids
            .iter()
            .any(|id: &String| id == &act.luna_id)
        {
            candidate_luna_ids.push(act.luna_id.clone());
        }
    }
    for luna_id in candidate_luna_ids {
        match download_course_material_from_contents(app, &luna_id, filename).await {
            Ok(Some(value)) => {
                if let Some(saved_path) = value.get("saved_path").and_then(|v| v.as_str()) {
                    let saved_p = PathBuf::from(saved_path);
                    if saved_p.exists() {
                        log::info!(
                            "Successfully auto-downloaded missing material file to {:?}",
                            saved_p
                        );
                        return Ok(saved_p);
                    }
                }
            }
            Ok(None) => {}
            Err(e) => log::warn!(
                "Course material auto-download failed for luna_id='{}': {}",
                luna_id,
                e
            ),
        }
    }

    for act in candidate_acts {
        if act.detail_path.is_empty() {
            continue;
        }
        if let Ok(resolved) =
            resolve_luna_attachment_with_lid(app, &act.title, filename, &act.luna_id).await
        {
            log::info!(
                "Found attachment in activity '{}', downloading...",
                act.title
            );
            if let Ok(saved) = download_resolved_luna_attachment(app, &resolved).await {
                let saved_p = PathBuf::from(&saved.saved_path);
                if saved_p.exists() {
                    log::info!("Successfully auto-downloaded missing file to {:?}", saved_p);
                    return Ok(saved_p);
                }
            }
        }
    }

    Err(format!(
        "ファイルが見つかりません。自動ダウンロードも失敗しました: {}",
        filename
    ))
}

pub(super) async fn download_course_material(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let filename = args
        .get("filename")
        .or_else(|| args.get("file_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if filename.is_empty() {
        return Err("filename（ファイル名）を指定してください".into());
    }
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let luna_id = args
        .get("luna_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    log::info!(
        "download_course_material: filename='{}', title='{}', luna_id='{}'",
        filename,
        title,
        luna_id
    );

    // When title is provided, use it directly (with luna_id to disambiguate same-title activities
    // across different courses).
    if !title.is_empty() {
        let resolved = resolve_luna_attachment_with_lid(app, title, &filename, luna_id).await?;
        let attachment_name = resolved.attachment.name.clone();
        let saved = download_resolved_luna_attachment(app, &resolved).await?;
        return Ok(json!({
            "status": "downloaded",
            "filename": attachment_name,
            "saved_path": saved.saved_path,
            "course": resolved.course_name,
        }));
    }

    // No title: if luna_id is provided, scan all activities under that course for the attachment.
    if !luna_id.is_empty() {
        let mut material_error = None;
        match download_course_material_from_contents(app, luna_id, &filename).await {
            Ok(Some(value)) => return Ok(value),
            Ok(None) => {}
            Err(e) => {
                log::warn!(
                    "download_course_material: material lookup/download failed for luna_id='{}': {}",
                    luna_id,
                    e
                );
                material_error = Some(e);
            }
        }

        let db = app.state::<Database>();
        let sub_acts: Vec<_> = db
            .get_all_luna_activities()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.luna_id == luna_id && !a.detail_path.is_empty())
            .collect();
        for act in sub_acts {
            if let Ok(resolved) =
                resolve_luna_attachment_with_lid(app, &act.title, &filename, luna_id).await
            {
                let attachment_name = resolved.attachment.name.clone();
                let Ok(saved) = download_resolved_luna_attachment(app, &resolved).await else {
                    continue;
                };
                return Ok(json!({
                    "status": "downloaded",
                    "filename": attachment_name,
                    "saved_path": saved.saved_path,
                    "course": resolved.course_name,
                }));
            }
        }
        if let Some(e) = material_error {
            return Err(format!(
                "luna_id='{}'の資料「{}」の確認またはダウンロードに失敗しました: {}",
                luna_id, filename, e
            ));
        }
        return Err(format!(
            "luna_id='{}'の課程内に「{}」の添付が見つかりませんでした",
            luna_id, filename
        ));
    }

    // Last resort: scan all course-material caches, then fall back to the broad activity sweep.
    let db = app.state::<Database>();
    for course in db.get_luna_courses().unwrap_or_default() {
        match download_course_material_from_contents(app, &course.luna_id, &filename).await {
            Ok(Some(value)) => return Ok(value),
            Ok(None) => {}
            Err(e) => log::warn!(
                "download_course_material: broad material lookup failed for luna_id='{}': {}",
                course.luna_id,
                e
            ),
        }
    }
    let path_to_resolve = crate::commands::default_download_dir().join(&filename);
    let resolved_path = auto_download_missing_file(app, &path_to_resolve).await?;
    Ok(json!({
        "status": "downloaded",
        "filename": filename,
        "saved_path": resolved_path.to_string_lossy(),
    }))
}

pub(super) async fn download_url(args: &Value) -> Result<Value, String> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if url.is_empty() {
        return Err("urlを指定してください".into());
    }
    let parsed = url::Url::parse(&url).map_err(|e| format!("URL解析失敗: {}", e))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("http/https のみダウンロードできます".into());
    }
    let custom_name = sanitize_text_arg(args, "filename", 200);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTPクライアント生成失敗: {}", e))?;
    let resp = client
        .get(parsed.clone())
        .send()
        .await
        .map_err(|e| format!("ダウンロード失敗: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status));
    }

    let content_disposition = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("レスポンス読み取り失敗: {}", e))?;
    if bytes.len() > 50 * 1024 * 1024 {
        return Err(format!(
            "ファイルが大きすぎます ({} bytes、上限50MB)",
            bytes.len()
        ));
    }

    let filename = custom_name
        .or_else(|| content_disposition.as_deref().and_then(filename_from_cd))
        .unwrap_or_else(|| filename_from_url(&parsed));
    let safe_name = sanitize_filename_basic(&filename);

    let saved = crate::luna_commands::save_to_downloads(&safe_name, &bytes, None)?;
    Ok(json!({
        "status": "downloaded",
        "url": url,
        "saved_path": saved,
        "filename": safe_name,
        "size_bytes": bytes.len(),
    }))
}

fn filename_from_cd(header: &str) -> Option<String> {
    for part in header.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed
            .strip_prefix("filename*=UTF-8''")
            .or_else(|| trimmed.strip_prefix("filename*=utf-8''"))
        {
            if let Ok(decoded) = urlencoding::decode(value) {
                return Some(decoded.into_owned());
            }
        }
        if let Some(value) = trimmed.strip_prefix("filename=") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

fn filename_from_url(parsed: &url::Url) -> String {
    parsed
        .path_segments()
        .and_then(|mut segs| segs.rfind(|s| !s.is_empty()))
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("download-{}.bin", chrono::Utc::now().format("%Y%m%d%H%M%S")))
}

fn sanitize_filename_basic(name: &str) -> String {
    let trimmed: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = trimmed.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        format!("download-{}.bin", chrono::Utc::now().format("%Y%m%d%H%M%S"))
    } else if trimmed.len() > 200 {
        let mut cut = 200;
        while cut > 0 && !trimmed.is_char_boundary(cut) {
            cut -= 1;
        }
        trimmed[..cut].to_string()
    } else {
        trimmed
    }
}

pub(super) async fn browser_close_tool(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    let label = crate::webview_toolbar::browser_close(app.clone(), target.clone()).await?;
    Ok(json!({
        "status": "closed",
        "label": label,
        "target": target,
    }))
}

pub(super) async fn list_browser_windows(app: &tauri::AppHandle) -> Result<Value, String> {
    let items = crate::webview_toolbar::list_browser_windows(app);
    Ok(json!({
        "windows": items.into_iter().map(|w| json!({
            "label": w.label,
            "target": w.target,
            "url": w.url,
        })).collect::<Vec<_>>()
    }))
}

pub(super) async fn open_browser_url(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if url.is_empty() {
        return Err("urlを指定してください".into());
    }
    crate::commands::open_external_url(app.clone(), url.clone(), None).await?;
    Ok(json!({
        "status": "opened",
        "url": url,
    }))
}

fn resolve_browser_target_from_args(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<String, String> {
    crate::webview_toolbar::resolve_browser_target(app, args.get("target").and_then(|v| v.as_str()))
}

fn browser_action_failed_message(result: &Value, fallback: &str) -> Option<String> {
    match result.get("ok").and_then(|v| v.as_bool()) {
        Some(true) => None,
        _ => Some(
            result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or(fallback)
                .to_string(),
        ),
    }
}

async fn run_browser_action_tool(
    app: &tauri::AppHandle,
    target: &str,
    action: Value,
    timeout_ms: u64,
    settle_ms: u64,
    fallback_error: &str,
) -> Result<Value, String> {
    let result =
        crate::webview_toolbar::run_browser_action(app, target, &action, timeout_ms).await?;
    if let Some(message) = browser_action_failed_message(&result, fallback_error) {
        return Err(message);
    }
    if settle_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(settle_ms)).await;
    }
    let current_url = crate::webview_toolbar::browser_get_url(app.clone(), target.to_string())
        .await
        .unwrap_or_default();
    let mut out = match result {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("result".into(), other);
            map
        }
    };
    out.insert("target".into(), Value::String(target.to_string()));
    if !current_url.is_empty() {
        out.insert("current_url".into(), Value::String(current_url));
    }
    Ok(Value::Object(out))
}

pub(super) async fn read_browser_page(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    let payload = crate::webview_toolbar::extract_page_text(app, &target).await?;
    let headings = compact_string_list(&payload.headings, 10, 140);
    let links: Vec<Value> = payload
        .links
        .iter()
        .filter_map(|link| {
            let text = compact_text(&link.text, 120);
            let url = compact_text(&link.url, 240);
            if text.is_none() && url.is_none() {
                return None;
            }
            let mut item = serde_json::Map::new();
            if let Some(text) = text {
                item.insert("text".into(), Value::String(text));
            }
            if let Some(url) = url {
                item.insert("url".into(), Value::String(url));
            }
            Some(Value::Object(item))
        })
        .take(8)
        .collect();
    let buttons: Vec<Value> = payload
        .buttons
        .iter()
        .filter_map(|button| {
            let text = compact_text(&button.text, 120)?;
            let mut item = serde_json::Map::new();
            item.insert("text".into(), Value::String(text));
            if let Some(kind) = compact_text(&button.kind, 32) {
                item.insert("type".into(), Value::String(kind));
            }
            Some(Value::Object(item))
        })
        .take(10)
        .collect();
    let inputs: Vec<Value> = payload
        .inputs
        .iter()
        .filter_map(|input| {
            let label = compact_text(&input.label, 120);
            let name = compact_text(&input.name, 80);
            let placeholder = compact_text(&input.placeholder, 120);
            let value = compact_text(&input.value, 120);
            let kind = compact_text(&input.kind, 32);
            if label.is_none()
                && name.is_none()
                && placeholder.is_none()
                && value.is_none()
                && kind.is_none()
            {
                return None;
            }
            let mut item = serde_json::Map::new();
            if let Some(label) = label {
                item.insert("label".into(), Value::String(label));
            }
            if let Some(kind) = kind {
                item.insert("type".into(), Value::String(kind));
            }
            if let Some(name) = name {
                item.insert("name".into(), Value::String(name));
            }
            if let Some(placeholder) = placeholder {
                item.insert("placeholder".into(), Value::String(placeholder));
            }
            if let Some(value) = value {
                item.insert("value".into(), Value::String(value));
            }
            if input.required {
                item.insert("required".into(), Value::Bool(true));
            }
            if input.disabled {
                item.insert("disabled".into(), Value::Bool(true));
            }
            Some(Value::Object(item))
        })
        .take(10)
        .collect();
    Ok(json!({
        "target": target,
        "title": compact_text(&payload.title, 200).unwrap_or_default(),
        "url": payload.url,
        "content_source": compact_text(&payload.content_source, 40).unwrap_or_else(|| "document".into()),
        "content": compact_text(&payload.text, 8_000).unwrap_or_default(),
        "headings": headings,
        "links": links,
        "interactive_elements": {
            "buttons": buttons,
            "inputs": inputs,
        },
    }))
}

pub(super) async fn browser_back(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    crate::webview_toolbar::browser_go_back(app.clone(), target.clone()).await?;
    let url = crate::webview_toolbar::browser_get_url(app.clone(), target.clone())
        .await
        .unwrap_or_default();
    Ok(json!({ "target": target, "status": "ok", "url": url }))
}

pub(super) async fn browser_forward(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    crate::webview_toolbar::browser_go_forward(app.clone(), target.clone()).await?;
    let url = crate::webview_toolbar::browser_get_url(app.clone(), target.clone())
        .await
        .unwrap_or_default();
    Ok(json!({ "target": target, "status": "ok", "url": url }))
}

pub(super) async fn browser_reload_page(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    crate::webview_toolbar::browser_reload(app.clone(), target.clone()).await?;
    let url = crate::webview_toolbar::browser_get_url(app.clone(), target.clone())
        .await
        .unwrap_or_default();
    Ok(json!({ "target": target, "status": "ok", "url": url }))
}

pub(super) async fn browser_click(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    let mut action = serde_json::Map::new();
    action.insert("kind".into(), Value::String("click".into()));
    if let Some(selector) = args.get("selector").and_then(|v| v.as_str()) {
        action.insert("selector".into(), Value::String(selector.to_string()));
    }
    if let Some(text) = args.get("text").and_then(|v| v.as_str()) {
        action.insert("text".into(), Value::String(text.to_string()));
    }
    if let Some(href_contains) = args.get("href_contains").and_then(|v| v.as_str()) {
        action.insert(
            "hrefContains".into(),
            Value::String(href_contains.to_string()),
        );
    }
    if let Some(index) = args.get("index").and_then(|v| v.as_u64()) {
        action.insert("index".into(), Value::Number(index.into()));
    }
    run_browser_action_tool(
        app,
        &target,
        Value::Object(action),
        4_000,
        450,
        "ページ内のクリック対象が見つかりません",
    )
    .await
}

pub(super) async fn browser_fill(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    let mut action = serde_json::Map::new();
    action.insert("kind".into(), Value::String("fill".into()));
    if let Some(selector) = args.get("selector").and_then(|v| v.as_str()) {
        action.insert("selector".into(), Value::String(selector.to_string()));
    }
    if let Some(label) = args.get("label").and_then(|v| v.as_str()) {
        action.insert("label".into(), Value::String(label.to_string()));
    }
    if let Some(value) = args.get("value").and_then(|v| v.as_str()) {
        action.insert("value".into(), Value::String(value.to_string()));
    }
    if let Some(index) = args.get("index").and_then(|v| v.as_u64()) {
        action.insert("index".into(), Value::Number(index.into()));
    }
    run_browser_action_tool(
        app,
        &target,
        Value::Object(action),
        4_000,
        120,
        "ページ内の入力欄が見つかりません",
    )
    .await
}

pub(super) async fn browser_select_option(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    let mut action = serde_json::Map::new();
    action.insert("kind".into(), Value::String("select_option".into()));
    if let Some(selector) = args.get("selector").and_then(|v| v.as_str()) {
        action.insert("selector".into(), Value::String(selector.to_string()));
    }
    if let Some(label) = args.get("label").and_then(|v| v.as_str()) {
        action.insert("label".into(), Value::String(label.to_string()));
    }
    if let Some(value) = args.get("value").and_then(|v| v.as_str()) {
        action.insert("value".into(), Value::String(value.to_string()));
    }
    if let Some(index) = args.get("index").and_then(|v| v.as_u64()) {
        action.insert("index".into(), Value::Number(index.into()));
    }
    run_browser_action_tool(
        app,
        &target,
        Value::Object(action),
        4_000,
        120,
        "ページ内の選択欄が見つかりません",
    )
    .await
}

pub(super) async fn browser_press(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    let mut action = serde_json::Map::new();
    action.insert("kind".into(), Value::String("press".into()));
    if let Some(selector) = args.get("selector").and_then(|v| v.as_str()) {
        action.insert("selector".into(), Value::String(selector.to_string()));
    }
    if let Some(key) = args.get("key").and_then(|v| v.as_str()) {
        action.insert("key".into(), Value::String(key.to_string()));
    }
    run_browser_action_tool(
        app,
        &target,
        Value::Object(action),
        4_000,
        300,
        "ページへキー入力を送れませんでした",
    )
    .await
}

pub(super) async fn browser_scroll(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    let mut action = serde_json::Map::new();
    action.insert("kind".into(), Value::String("scroll".into()));
    if let Some(selector) = args.get("selector").and_then(|v| v.as_str()) {
        action.insert("selector".into(), Value::String(selector.to_string()));
    }
    if let Some(direction) = args.get("direction").and_then(|v| v.as_str()) {
        action.insert("direction".into(), Value::String(direction.to_string()));
    }
    if let Some(amount) = args.get("amount").and_then(|v| v.as_u64()) {
        action.insert("amount".into(), Value::Number(amount.into()));
    }
    run_browser_action_tool(
        app,
        &target,
        Value::Object(action),
        3_500,
        120,
        "ページをスクロールできませんでした",
    )
    .await
}

pub(super) async fn browser_wait_for(
    app: &tauri::AppHandle,
    args: &Value,
) -> Result<Value, String> {
    let target = resolve_browser_target_from_args(app, args)?;
    let mut action = serde_json::Map::new();
    action.insert("kind".into(), Value::String("wait_for".into()));
    if let Some(selector) = args.get("selector").and_then(|v| v.as_str()) {
        action.insert("selector".into(), Value::String(selector.to_string()));
    }
    if let Some(text) = args.get("text").and_then(|v| v.as_str()) {
        action.insert("text".into(), Value::String(text.to_string()));
    }
    if let Some(timeout_ms) = args.get("timeout_ms").and_then(|v| v.as_u64()) {
        action.insert("timeoutMs".into(), Value::Number(timeout_ms.into()));
    }
    run_browser_action_tool(
        app,
        &target,
        Value::Object(action),
        args.get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(3_000)
            + 700,
        80,
        "等待页面变化超时了",
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material_file(display_name: &str, file_name: &str) -> crate::luna_parser::LunaMaterialFile {
        crate::luna_parser::LunaMaterialFile {
            display_name: display_name.to_string(),
            file_name: file_name.to_string(),
            object_name: "object".into(),
            resource_id: "resource".into(),
            material_id: "material".into(),
            file_type: "0".into(),
            end_date: String::new(),
            scan_status: "1".into(),
            link_type: "file".into(),
            external_url: String::new(),
        }
    }

    fn course_contents(
        files: Vec<crate::luna_parser::LunaMaterialFile>,
    ) -> crate::luna_parser::LunaCourseContents {
        crate::luna_parser::LunaCourseContents {
            course_name: "政治学基礎 ２".into(),
            semester: String::new(),
            teachers: String::new(),
            ta_info: String::new(),
            la_info: String::new(),
            syllabus_url: String::new(),
            grade_url: String::new(),
            menus: Vec::new(),
            announcements: Vec::new(),
            online_tools: Vec::new(),
            materials: vec![crate::luna_parser::LunaContentItem {
                title: "中間試験資料".into(),
                url: String::new(),
                period: String::new(),
                status: String::new(),
                item_type: "material".into(),
                description: String::new(),
                files,
            }],
            reports: Vec::new(),
            examinations: Vec::new(),
            discussions: Vec::new(),
            surveys: Vec::new(),
            attendances: Vec::new(),
        }
    }

    #[test]
    fn matches_course_material_by_file_name() {
        let contents = course_contents(vec![material_file(
            "試験要項",
            "2026年度春中間試験の実施要項.pdf",
        )]);
        let matched = match_material_file(&contents, "2026年度春中間試験の実施要項.pdf")
            .expect("material should match");
        assert_eq!(
            effective_material_filename(&matched.file),
            "2026年度春中間試験の実施要項.pdf"
        );
    }

    #[test]
    fn matches_course_material_by_display_name_when_file_name_is_empty() {
        let contents = course_contents(vec![material_file("2026年度春中間試験の実施要項.pdf", "")]);
        let matched = match_material_file(&contents, "2026年度春中間試験の実施要項")
            .expect("display name should match");
        assert_eq!(
            effective_material_filename(&matched.file),
            "2026年度春中間試験の実施要項.pdf"
        );
    }
}
