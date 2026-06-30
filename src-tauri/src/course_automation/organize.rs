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

/// Splits candidates into what the heuristic can place *confidently* (an explicit
/// 第N回 session or a topic marker in title/filename/summary/findings) and the ids
/// it can only guess at (no marker → would fall to a kind folder). The confident
/// half is deterministic and free; returning the ambiguous remainder lets the
/// caller spend the AI only on the few files that actually need semantic/date
/// reasoning, instead of paying to re-derive placements the markers already give.
/// Iteration is in id order, so the split is stable.
pub fn confident_plan(
    candidates: &BTreeMap<String, OrganizeCandidate>,
) -> (Vec<PlannedGroup>, Vec<String>) {
    let mut order: Vec<String> = Vec::new();
    let mut clusters: BTreeMap<String, (String, String, Vec<String>)> = BTreeMap::new();
    let mut ambiguous: Vec<String> = Vec::new();
    for (id, cand) in candidates {
        let (key, label) = theme_of(&cand.title, &cand.summary, &cand.filename, &cand.kind);
        // Only a session/topic marker counts as confident; a bare kind fallback is
        // a guess the AI (with notices/schedule) can place better.
        if key.starts_with("kind:") {
            ambiguous.push(id.clone());
            continue;
        }
        let entry = clusters
            .entry(key.clone())
            .or_insert_with(|| (label, cand.kind.clone(), Vec::new()));
        if entry.2.is_empty() {
            order.push(key);
        }
        entry.2.push(id.clone());
    }
    let plan = order
        .into_iter()
        .filter_map(|key| clusters.remove(&key))
        .map(|(label, kind, doc_ids)| PlannedGroup {
            label,
            kind,
            doc_ids,
        })
        .collect();
    (plan, ambiguous)
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
/// groups, undo log). Every resolved file is filed into its theme folder —
/// including a lone file in a session/topic of its own — so the end state is
/// that *every* document lives under a category, none stranded loose at the
/// course root. Already-filed files are left in place. Returns how many moved.
///
/// `course_root` is the course's top-level folder. Every destination is computed
/// as `course_root/<theme>/<filename>` regardless of where the file currently
/// sits, so filing is a pure function of (root, theme, name): it never nests a
/// file under its own theme folder, and re-running flattens any prior nesting
/// back to the one-level scheme.
pub fn apply_groups(
    status: &mut CourseAutomationStatus,
    candidates: &BTreeMap<String, OrganizeCandidate>,
    planned: &[PlannedGroup],
    course_root: &Path,
) -> usize {
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
        // File every document that resolves on disk, lone files included: the
        // goal is that nothing is left uncategorized at the course root. A single
        // 第NN回 handout still belongs in its 第NN回 folder.
        if members.is_empty() {
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
            if let Some(to) = plan_destination(&from, &folder, course_root) {
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

    // Sweep away any theme folders the moves just emptied — including the deep
    // nested chains a self-heal pulled files out of — so filing never leaves
    // hollow directories behind.
    prune_empty_dirs(course_root);
    path_map.len()
}

/// Removes empty subdirectories under `root` (never `root` itself), bottom-up.
/// A directory counts as empty when it holds only OS junk (.DS_Store / Thumbs.db
/// / desktop.ini) and subdirectories that were themselves pruned; the junk is
/// deleted so the folder can go. Unknown files (incl. other hidden files) keep a
/// directory, so nothing with real content is ever removed.
fn prune_empty_dirs(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            prune_dir(&path);
        }
    }
}

/// Recursively prunes `dir`; returns whether it was (now) removed.
fn prune_dir(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut removable = true;
    let mut junk: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !prune_dir(&path) {
                removable = false;
            }
        } else {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini") {
                junk.push(path);
            } else {
                removable = false;
            }
        }
    }
    if !removable {
        return false;
    }
    for path in junk {
        let _ = std::fs::remove_file(path);
    }
    std::fs::remove_dir(dir).is_ok()
}

/// Reverts the last organize batch: moves the filed files back and removes the
/// now-empty theme folders. Returns how many files were restored. `course_root`
/// scopes the empty-folder sweep so every vacated theme folder is cleared, not
/// just each file's immediate parent.
pub fn undo_organize(status: &mut CourseAutomationStatus, course_root: &Path) -> usize {
    let moves = std::mem::take(&mut status.organize_undo);
    let mut back_map: BTreeMap<String, String> = BTreeMap::new();
    let mut restored = 0usize;
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

    // Clear every theme folder the restore emptied (incl. nested chains), not
    // only the leaf parents.
    prune_empty_dirs(course_root);
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
    // Normalize full-width digits (第１０回) to ASCII (第10回) up front: Rust's
    // `\d` matches both, but `str::parse::<u32>` only accepts ASCII, so without
    // this a full-width marker skips zero-padding and forks a separate folder
    // from its half-width twin.
    let text = normalize_fullwidth_digits(&format!("{title} {filename} {summary}"));
    if let Some(caps) = SESSION_RE.captures(&text) {
        let n = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("回");
        // Zero-pad every unit (第03回 / 第03週) so labels sort and merge uniformly.
        let label = match n.parse::<u32>() {
            Ok(num) => format!("第{num:02}{unit}"),
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
        let word = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let label = canonical_topic(word);
        return (format!("topic:{label}"), label.to_string());
    }
    (format!("kind:{kind}"), kind_label(kind))
}

/// Canonicalizes a session label so the AI planner's grouping merges with the
/// heuristic's: a label that is (or contains) a 第N回 marker becomes the padded,
/// half-width `第NN回` form the heuristic emits; anything else is just trimmed.
/// Keeps `第3回` / `第１０回` / `第3回 オリエン` from forking a second folder next
/// to `第03回` / `第10回`.
pub fn canonical_session_label(label: &str) -> String {
    detect_session(label).unwrap_or_else(|| label.trim().to_string())
}

/// The canonical `第NN回` label if `text` contains a session marker (full-width
/// tolerant), else None. Lets callers cheaply tell whether a notice pins a
/// session without pulling in the regex.
pub fn detect_session(text: &str) -> Option<String> {
    let normalized = normalize_fullwidth_digits(text);
    let caps = SESSION_RE.captures(&normalized)?;
    let n = caps.get(1)?.as_str();
    let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("回");
    let num: u32 = n.parse().ok()?;
    Some(format!("第{num:02}{unit}"))
}

/// Maps full-width digits (０-９) to ASCII so session markers parse and pad the
/// same regardless of which width the source used.
fn normalize_fullwidth_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Canonical Japanese label for a topic match, so an English hit (`report`,
/// `quiz`, `exam`) and its Japanese synonym land in the same folder instead of
/// forking a `report` folder next to a `レポート` one. Unknown words pass through.
fn canonical_topic(word: &str) -> &'static str {
    match word.to_lowercase().as_str() {
        "レポート" | "報告" | "報告書" | "report" => "レポート",
        "課題" | "宿題" | "assignment" => "課題",
        "小テスト" | "クイズ" | "quiz" => "小テスト",
        "試験" | "exam" => "試験",
        "演習" => "演習",
        _ => "課題",
    }
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

/// Destination is always `<course_root>/<folder>/<filename>`, independent of
/// where the file currently sits. This makes filing a pure, convergent function:
/// a file at the root moves in, a file already at the destination stays put
/// (`dest == from` → None, idempotent), and a file buried in a nested theme
/// folder (`第01回/第01回/…`) is pulled back up to the single-level location —
/// so re-running self-heals any historical nesting instead of deepening it.
fn plan_destination(from: &Path, folder: &str, course_root: &Path) -> Option<PathBuf> {
    let filename = from.file_name()?;
    let dest = course_root.join(folder).join(filename);
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
    fn fullwidth_and_halfwidth_session_markers_merge() {
        let (key_full, label_full) = theme_of("第１０回 資料", "", "a.pdf", "material");
        let (key_half, label_half) = theme_of("第10回 解答", "", "b.pdf", "material");
        assert_eq!(key_full, key_half);
        assert_eq!(label_full, "第10回");
        assert_eq!(label_half, "第10回");
    }

    #[test]
    fn topic_marker_groups_reports() {
        let (key, label) = theme_of("個人レポートの提出について", "", "report.pdf", "material");
        assert_eq!(key, "topic:レポート");
        assert_eq!(label, "レポート");
    }

    #[test]
    fn english_and_japanese_topic_merge_to_one_folder() {
        let (key_en, label_en) = theme_of("Final Report guidelines", "", "report.pdf", "material");
        let (key_ja, label_ja) = theme_of("個人レポートについて", "", "doc.pdf", "material");
        assert_eq!(key_en, key_ja);
        assert_eq!(label_en, "レポート");
        assert_eq!(label_ja, "レポート");
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

    fn candidate(path: &str, kind: &str) -> OrganizeCandidate {
        let filename = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string();
        OrganizeCandidate {
            path: path.to_string(),
            filename: filename.clone(),
            title: filename,
            summary: String::new(),
            kind: kind.to_string(),
        }
    }

    #[test]
    fn confident_plan_splits_marker_files_from_ambiguous() {
        let mut candidates: BTreeMap<String, OrganizeCandidate> = BTreeMap::new();
        candidates.insert("a".into(), candidate("/c/第03回資料.pdf", "material")); // session marker
        candidates.insert("b".into(), candidate("/c/レポート課題.pdf", "material")); // topic marker
        candidates.insert("c".into(), candidate("/c/20260526_live.md", "ライブノート")); // date only
        candidates.insert("d".into(), candidate("/c/slides.pdf", "material")); // no marker

        let (plan, ambiguous) = confident_plan(&candidates);

        let placed: HashSet<String> = plan.iter().flat_map(|g| g.doc_ids.clone()).collect();
        assert!(placed.contains("a") && placed.contains("b"));
        // Only the markerless files need the AI.
        assert_eq!(ambiguous.len(), 2);
        assert!(ambiguous.contains(&"c".to_string()) && ambiguous.contains(&"d".to_string()));
    }

    #[test]
    fn filing_is_root_relative_and_never_nests() {
        let root = std::env::temp_dir().join(format!("organize-flat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("第01回資料.pdf");
        let b = root.join("第01回座席表.pdf");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();

        let mut candidates: BTreeMap<String, OrganizeCandidate> = BTreeMap::new();
        candidates.insert("id_a".into(), candidate(a.to_str().unwrap(), "material"));
        candidates.insert("id_b".into(), candidate(b.to_str().unwrap(), "material"));
        let planned = vec![PlannedGroup {
            label: "第01回".into(),
            kind: "material".into(),
            doc_ids: vec!["id_a".into(), "id_b".into()],
        }];
        let mut status = CourseAutomationStatus::default();

        // First filing: both move into <root>/第01回/.
        let moved = apply_groups(&mut status, &candidates, &planned, &root);
        assert_eq!(moved, 2);
        assert!(root.join("第01回").join("第01回資料.pdf").is_file());
        assert!(root.join("第01回").join("第01回座席表.pdf").is_file());

        // Re-run with the files at their new (filed) locations: idempotent, no
        // second level of 第01回/第01回.
        let mut candidates2: BTreeMap<String, OrganizeCandidate> = BTreeMap::new();
        candidates2.insert(
            "id_a".into(),
            candidate(root.join("第01回").join("第01回資料.pdf").to_str().unwrap(), "material"),
        );
        candidates2.insert(
            "id_b".into(),
            candidate(root.join("第01回").join("第01回座席表.pdf").to_str().unwrap(), "material"),
        );
        let moved2 = apply_groups(&mut status, &candidates2, &planned, &root);
        assert_eq!(moved2, 0);
        assert!(!root.join("第01回").join("第01回").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn lone_file_is_still_filed_into_its_theme_folder() {
        let root = std::env::temp_dir().join(format!("organize-solo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let solo = root.join("第07回資料.pdf");
        std::fs::write(&solo, b"x").unwrap();

        let mut candidates: BTreeMap<String, OrganizeCandidate> = BTreeMap::new();
        candidates.insert("id_solo".into(), candidate(solo.to_str().unwrap(), "material"));
        let planned = vec![PlannedGroup {
            label: "第07回".into(),
            kind: "material".into(),
            doc_ids: vec!["id_solo".into()],
        }];
        let mut status = CourseAutomationStatus::default();

        let moved = apply_groups(&mut status, &candidates, &planned, &root);
        assert_eq!(moved, 1);
        assert!(root.join("第07回").join("第07回資料.pdf").is_file());
        // Nothing left loose at the root.
        assert!(!root.join("第07回資料.pdf").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn filing_flattens_existing_nesting() {
        let root = std::env::temp_dir().join(format!("organize-heal-{}", uuid::Uuid::new_v4()));
        // Simulate the old bug: files buried several 第01回 levels deep.
        let nested = root.join("第01回").join("第01回").join("第01回");
        std::fs::create_dir_all(&nested).unwrap();
        let a = nested.join("資料.pdf");
        let b = nested.join("座席表.pdf");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();

        let mut candidates: BTreeMap<String, OrganizeCandidate> = BTreeMap::new();
        candidates.insert("id_a".into(), candidate(a.to_str().unwrap(), "material"));
        candidates.insert("id_b".into(), candidate(b.to_str().unwrap(), "material"));
        let planned = vec![PlannedGroup {
            label: "第01回".into(),
            kind: "material".into(),
            doc_ids: vec!["id_a".into(), "id_b".into()],
        }];
        let mut status = CourseAutomationStatus::default();

        let moved = apply_groups(&mut status, &candidates, &planned, &root);
        assert_eq!(moved, 2);
        // Pulled back up to the single-level location.
        assert!(root.join("第01回").join("資料.pdf").is_file());
        assert!(root.join("第01回").join("座席表.pdf").is_file());
        // The vacated nested chain 第01回/第01回/第01回 is swept away.
        assert!(!root.join("第01回").join("第01回").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pruning_clears_vacated_and_junk_only_folders() {
        let root = std::env::temp_dir().join(format!("organize-prune-{}", uuid::Uuid::new_v4()));
        // An empty theme folder, one with only a .DS_Store, and a nested empty
        // chain — all should be removed. A folder with a real file must survive.
        std::fs::create_dir_all(root.join("空")).unwrap();
        std::fs::create_dir_all(root.join("ゴミ")).unwrap();
        std::fs::write(root.join("ゴミ").join(".DS_Store"), b"x").unwrap();
        std::fs::create_dir_all(root.join("深").join("層").join("空")).unwrap();
        std::fs::create_dir_all(root.join("保持")).unwrap();
        std::fs::write(root.join("保持").join("資料.pdf"), b"x").unwrap();

        prune_empty_dirs(&root);

        assert!(!root.join("空").exists());
        assert!(!root.join("ゴミ").exists());
        assert!(!root.join("深").exists());
        assert!(root.join("保持").join("資料.pdf").is_file());
        // Root itself is never removed.
        assert!(root.is_dir());

        std::fs::remove_dir_all(&root).ok();
    }
}
