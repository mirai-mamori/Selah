//! 自動検知 file organization.
//!
//! Groups a course's already-downloaded documents by theme (same session /
//! same assignment topic) and files each group into a theme subfolder next to
//! where the files already live. Best-effort and fully reversible: every move
//! is recorded so the whole batch can be undone in one step.
//!
//! The grouping reuses the per-document AI analysis (title / kind) the agent
//! already produced — the "smart" part is the perception upstream; here we turn
//! that into a deterministic, predictable filing so the same input always files
//! the same way.

use super::CourseAutomationStatus;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// One document filed under a theme group (paths kept for provenance / undo).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeFile {
    pub filename: String,
    pub title: String,
    pub from_path: String,
    pub to_path: String,
    pub moved: bool,
}

/// A theme cluster — its files share a session marker (第3回) or an assignment
/// topic (レポート), else they fall back to a kind folder (教材 / 課題).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeGroup {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub folder: String,
    pub files: Vec<OrganizeFile>,
}

/// One reversible move: where the file is now, and where to put it back.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeMove {
    pub current: String,
    pub original: String,
}

static SESSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"第\s*(\d+)\s*(回|週|章|講|課)").unwrap());
static LESSON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(lesson|week|unit|chapter|day)\s*0*(\d+)").unwrap());
static TOPIC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(小テスト|レポート|報告書|報告|課題|宿題|試験|クイズ|演習|quiz|report|assignment|exam)")
        .unwrap()
});

/// A grouping decision (from the AI planner or the heuristic fallback): a theme
/// label and the ids of the documents that belong under it. Decoupled from disk
/// work so the same `apply_groups` machinery files either source.
#[derive(Debug, Clone, Default)]
pub struct PlannedGroup {
    pub label: String,
    pub kind: String,
    pub doc_ids: Vec<String>,
}

/// One file the organizer can move — either a tracked ledger document or a loose
/// file sitting in the course folder (e.g. a Live note the agent never tracked).
/// Unifying both lets the AI place them together and the same machinery file them.
/// Ledger documents and loose files share this shape. On move, `apply_groups`
/// patches any matching ledger path; a loose file simply has no entry to patch.
#[derive(Debug, Clone)]
pub struct OrganizeCandidate {
    pub path: String,
    pub filename: String,
    pub title: String,
    pub summary: String,
    pub kind: String,
}

/// Heuristic grouping from each candidate's title/summary/kind, used to sweep up
/// files the AI planner left unfiled (`exclude` holds the ids it already placed).
/// Iteration is in id order, so the same input always yields the same plan.
pub fn heuristic_plan(
    candidates: &BTreeMap<String, OrganizeCandidate>,
    exclude: &HashSet<String>,
) -> Vec<PlannedGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut clusters: BTreeMap<String, (String, String, Vec<String>)> = BTreeMap::new();
    for (id, cand) in candidates {
        if exclude.contains(id) {
            continue;
        }
        let (key, label) = theme_of(&cand.title, &cand.summary, &cand.filename, &cand.kind);
        let entry = clusters
            .entry(key.clone())
            .or_insert_with(|| (label, cand.kind.clone(), Vec::new()));
        if entry.2.is_empty() {
            order.push(key);
        }
        entry.2.push(id.clone());
    }
    order
        .into_iter()
        .filter_map(|key| clusters.remove(&key))
        .map(|(label, kind, doc_ids)| PlannedGroup {
            label,
            kind,
            doc_ids,
        })
        .collect()
}

/// Merges a secondary plan (the heuristic sweep) into a primary one (the AI
/// groups): same-folder groups combine their members so the heuristic feeds the
/// session folders the AI already opened instead of forking near-duplicates. An
/// id placed by the primary plan is never re-added by the secondary.
pub fn merge_plans(mut primary: Vec<PlannedGroup>, secondary: Vec<PlannedGroup>) -> Vec<PlannedGroup> {
    let mut by_folder: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (index, group) in primary.iter().enumerate() {
        by_folder.entry(sanitize_component(&group.label)).or_insert(index);
        for id in &group.doc_ids {
            seen.insert(id.clone());
        }
    }
    for group in secondary {
        let ids: Vec<String> = group
            .doc_ids
            .into_iter()
            .filter(|id| seen.insert(id.clone()))
            .collect();
        if ids.is_empty() {
            continue;
        }
        let folder = sanitize_component(&group.label);
        if let Some(&index) = by_folder.get(&folder) {
            primary[index].doc_ids.extend(ids);
        } else {
            by_folder.insert(folder, primary.len());
            primary.push(PlannedGroup {
                label: group.label,
                kind: group.kind,
                doc_ids: ids,
            });
        }
    }
    primary
}

/// Files documents into theme folders per `planned`, mutating `status` (paths,
/// groups, undo log). Only groups that resolve to ≥2 existing files are filed;
/// already-filed files are left in place. Returns how many files were moved.
pub fn apply_groups(
    status: &mut CourseAutomationStatus,
    candidates: &BTreeMap<String, OrganizeCandidate>,
    planned: &[PlannedGroup],
) -> usize {
    // Folders a previous organize created — so re-running treats a file already
    // inside a theme folder as living in the course root, not nesting under it.
    let known_folders: HashSet<String> = status
        .organize_groups
        .iter()
        .map(|group| group.folder.clone())
        .collect();

    let mut groups: Vec<OrganizeGroup> = Vec::new();
    let mut moves: Vec<OrganizeMove> = Vec::new();
    let mut path_map: BTreeMap<String, String> = BTreeMap::new();
    let mut used_folders: Vec<String> = Vec::new();

    for group in planned {
        // Resolve members to candidates whose file still exists on disk.
        let members: Vec<&OrganizeCandidate> = group
            .doc_ids
            .iter()
            .filter_map(|id| candidates.get(id))
            .filter(|cand| Path::new(&cand.path).is_file())
            .collect();
        // Only file clusters that actually group related files; lone documents
        // stay where they are rather than spawning a one-file folder.
        if members.len() < 2 {
            continue;
        }
        let mut folder = sanitize_component(&group.label);
        if folder.is_empty() {
            continue;
        }
        // Disambiguate a folder name the planner reused for two distinct groups.
        while used_folders.contains(&folder) {
            folder.push('_');
        }
        used_folders.push(folder.clone());

        let mut files: Vec<OrganizeFile> = Vec::new();
        for cand in members {
            let from = PathBuf::from(&cand.path);
            let mut file = OrganizeFile {
                filename: cand.filename.clone(),
                title: cand.title.clone(),
                from_path: cand.path.clone(),
                to_path: cand.path.clone(),
                moved: false,
            };
            if let Some(to) = plan_destination(&from, &folder, &known_folders) {
                let to_str = to.to_string_lossy().to_string();
                if move_file(&from, &to) {
                    path_map.insert(cand.path.clone(), to_str.clone());
                    moves.push(OrganizeMove {
                        current: to_str.clone(),
                        original: cand.path.clone(),
                    });
                    file.to_path = to_str;
                    file.moved = true;
                }
            }
            files.push(file);
        }
        groups.push(OrganizeGroup {
            id: folder.clone(),
            label: group.label.clone(),
            kind: group.kind.clone(),
            folder,
            files,
        });
    }

    // Re-point the ledger at the new locations so reuse / re-analysis still find
    // the files after they move.
    if !path_map.is_empty() {
        for doc in status.document_analyses.iter_mut() {
            if let Some(next) = path_map.get(&doc.path) {
                doc.path = next.clone();
            }
        }
        for artifact in status.artifacts.iter_mut() {
            if let Some(next) = path_map.get(&artifact.path) {
                artifact.path = next.clone();
            }
        }
    }

    if groups.is_empty() {
        status.organize_groups.clear();
    } else {
        status.organize_groups = groups;
    }
    if !moves.is_empty() {
        status.organize_undo = moves;
        status.organize_can_undo = true;
    }
    path_map.len()
}

/// Reverts the last organize batch: moves the filed files back and removes the
/// now-empty theme folders. Returns how many files were restored.
pub fn undo_organize(status: &mut CourseAutomationStatus) -> usize {
    let moves = std::mem::take(&mut status.organize_undo);
    let mut back_map: BTreeMap<String, String> = BTreeMap::new();
    let mut restored = 0usize;
    // Reverse order so a folder is emptied before we try to remove it.
    for mv in moves.iter().rev() {
        let current = PathBuf::from(&mv.current);
        let original = PathBuf::from(&mv.original);
        if !current.is_file() || original.exists() {
            continue;
        }
        if let Some(parent) = original.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::rename(&current, &original).is_ok() {
            restored += 1;
            back_map.insert(mv.current.clone(), mv.original.clone());
            if let Some(dir) = current.parent() {
                // Only succeeds when the theme folder is now empty.
                let _ = std::fs::remove_dir(dir);
            }
        }
    }

    if !back_map.is_empty() {
        for doc in status.document_analyses.iter_mut() {
            if let Some(next) = back_map.get(&doc.path) {
                doc.path = next.clone();
            }
        }
        for artifact in status.artifacts.iter_mut() {
            if let Some(next) = back_map.get(&artifact.path) {
                artifact.path = next.clone();
            }
        }
    }

    status.organize_groups.clear();
    status.organize_can_undo = false;
    restored
}

/// Theme key + display label for a document. A session/lesson marker wins (it
/// groups everything from the same 回 together, with a zero-padded 第NN回 label
/// so it merges with the AI's session folders); otherwise a topic word; failing
/// both, the document kind. The summary is searched too, so a note whose title
/// is just a date still finds its 第N回 when the body mentions it.
fn theme_of(title: &str, summary: &str, filename: &str, kind: &str) -> (String, String) {
    let text = format!("{title} {filename} {summary}");
    if let Some(caps) = SESSION_RE.captures(&text) {
        let n = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("回");
        let label = match n.parse::<u32>() {
            Ok(num) if unit == "回" => format!("第{num:02}回"),
            _ => format!("第{n}{unit}"),
        };
        return (format!("session:{label}"), label);
    }
    if let Some(caps) = LESSON_RE.captures(&text) {
        let word = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let n = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
        let label = format!("{} {}", title_case(word), n);
        return (format!("session:{}", label.to_lowercase()), label);
    }
    if let Some(caps) = TOPIC_RE.captures(&text) {
        let word = caps.get(1).map(|m| m.as_str()).unwrap_or_default().to_string();
        return (format!("topic:{}", word.to_lowercase()), word);
    }
    (format!("kind:{kind}"), kind_label(kind))
}

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

fn kind_label(kind: &str) -> String {
    match kind {
        "material" => "教材".into(),
        "announcement" => "お知らせ".into(),
        "report" => "課題".into(),
        other if !other.is_empty() => other.to_string(),
        _ => "資料".into(),
    }
}

/// Destination inside `<course base>/<folder>/`. The base is the file's current
/// directory, unless that directory is itself a theme folder we created before
/// (then we step up one), so re-clustering re-files instead of nesting. Returns
/// None when the file is already at the destination (re-runs are idempotent).
fn plan_destination(from: &Path, folder: &str, known_folders: &HashSet<String>) -> Option<PathBuf> {
    let parent = from.parent()?;
    let in_known_folder = parent
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| known_folders.contains(name));
    let base = if in_known_folder {
        parent.parent()?
    } else {
        parent
    };
    let filename = from.file_name()?;
    let dest = base.join(folder).join(filename);
    if dest == from {
        return None;
    }
    Some(dest)
}

/// Moves `from` → `to`, creating the folder. Never overwrites an existing file.
fn move_file(from: &Path, to: &Path) -> bool {
    if to.exists() {
        return false;
    }
    if let Some(dir) = to.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
    }
    std::fs::rename(from, to).is_ok()
}

/// Safe single path component: strips separators / reserved characters and bounds
/// the length so a theme label can never escape its folder.
fn sanitize_component(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    trimmed.chars().take(60).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_marker_groups_across_kinds() {
        let (key_a, label_a) = theme_of("第3回 レポート課題", "", "report3.pdf", "report");
        let (key_b, _) = theme_of("講義スライド", "第3回の内容です", "slides.pdf", "material");
        assert_eq!(key_a, key_b);
        assert_eq!(label_a, "第03回");
    }

    #[test]
    fn topic_marker_groups_reports() {
        let (key, label) = theme_of("個人レポートの提出について", "", "report.pdf", "material");
        assert_eq!(key, "topic:レポート");
        assert_eq!(label, "レポート");
    }

    #[test]
    fn falls_back_to_kind_folder() {
        let (key, label) = theme_of("配布スライド", "", "slide.pdf", "material");
        assert_eq!(key, "kind:material");
        assert_eq!(label, "教材");
    }

    #[test]
    fn sanitize_strips_separators() {
        assert_eq!(sanitize_component("第3回/レポート"), "第3回_レポート");
        assert!(!sanitize_component("..").contains('.'));
    }
}
