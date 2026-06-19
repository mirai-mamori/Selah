//! North Star core: a unified, time-aware, user-annotatable item model that
//! every SenseA facet (findings, memory, …) maps onto. Lifecycle (expiry),
//! identity, dedup and user-state are implemented once here instead of being
//! re-coded per facet.
//!
//! - `Item` is what a facet produces. Its `id` is a stable content fingerprint
//!   so dedup, user-state and delivery can key off it across summary rounds.
//! - User-state (known/done/dismissed/approved) is NOT stored on the item — it
//!   lives in a per-course overlay keyed by `id` (see `prune_states`), so the
//!   model regenerating items never clobbers what the user marked.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

fn is_false(value: &bool) -> bool {
    !*value
}

/// Cross-cutting tags a facet attaches; they drive sinks (TODO / notify) and UI
/// emphasis, not identity.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFlags {
    #[serde(default, skip_serializing_if = "is_false")]
    pub action: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub urgent: bool,
}

impl ItemFlags {
    fn is_empty(&self) -> bool {
        !self.action && !self.urgent
    }
}

/// A unified item produced by a facet.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub facet: String,
    pub text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub category: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub expires_at: String,
    #[serde(skip_serializing_if = "ItemFlags::is_empty")]
    pub flags: ItemFlags,
}

// Accepts three shapes so model output, persisted items and legacy data all
// deserialize: a bare string; the model's flat object ({text, expiresAt, action,
// urgent, …}); and a fully-formed persisted item ({id, facet, flags, …}). `id`
// and `facet` are (re)assigned in `normalize_items`, so they may be absent here.
impl<'de> Deserialize<'de> for Item {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Text(String),
            Full {
                #[serde(default)]
                id: String,
                #[serde(default)]
                facet: String,
                #[serde(default)]
                text: String,
                #[serde(default)]
                category: String,
                #[serde(default, rename = "expiresAt")]
                expires_at: String,
                #[serde(default)]
                action: bool,
                #[serde(default)]
                urgent: bool,
                #[serde(default)]
                flags: Option<ItemFlags>,
            },
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::Text(text) => Item {
                text,
                ..Default::default()
            },
            Repr::Full {
                id,
                facet,
                text,
                category,
                expires_at,
                action,
                urgent,
                flags,
            } => {
                let mut flags = flags.unwrap_or_default();
                // Fold flat model flags into the structured form.
                flags.action |= action;
                flags.urgent |= urgent;
                Item {
                    id,
                    facet,
                    text,
                    category,
                    expires_at,
                    flags,
                }
            }
        })
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

/// Stable identity for an item: a short fingerprint of its facet + whitespace-
/// normalized text. Same content → same id across rounds, so user-state and
/// dedup survive the model rephrasing surrounding items.
pub fn item_id(facet: &str, text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hasher = Sha256::new();
    hasher.update(facet.as_bytes());
    hasher.update([0u8]);
    hasher.update(normalized.as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(8)
        .map(|b| format!("{:02x}", b))
        .collect()
}

fn is_expired(expires_at: &str, today: Option<chrono::NaiveDate>) -> bool {
    match (
        today,
        chrono::NaiveDate::parse_from_str(expires_at.get(..10).unwrap_or(""), "%Y-%m-%d").ok(),
    ) {
        (Some(today), Some(expiry)) => expiry < today,
        _ => false,
    }
}

/// Trims, assigns id+facet and dedups by id, then splits into (active, expired)
/// by `expires_at`. Facets that discard expired items (findings) take `.0`;
/// facets that archive them (memory) use both. No item cap.
pub fn partition_by_expiry(
    items: Vec<Item>,
    facet: &str,
    today: &str,
    max_chars: usize,
) -> (Vec<Item>, Vec<Item>) {
    let today_date = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok();
    let mut active = Vec::new();
    let mut expired = Vec::new();
    let mut seen = HashSet::new();
    for mut item in items {
        item.facet = facet.to_string();
        item.text = truncate_chars(item.text.trim(), max_chars);
        item.category = item.category.trim().to_string();
        item.expires_at = item.expires_at.trim().to_string();
        if item.text.is_empty() {
            continue;
        }
        item.id = item_id(facet, &item.text);
        if !seen.insert(item.id.clone()) {
            continue;
        }
        if is_expired(&item.expires_at, today_date) {
            expired.push(item);
        } else {
            active.push(item);
        }
    }
    (active, expired)
}

/// The lifecycle pipeline for facets that simply drop expired items (findings):
/// the active half of `partition_by_expiry`.
pub fn normalize_items(items: Vec<Item>, facet: &str, today: &str, max_chars: usize) -> Vec<Item> {
    partition_by_expiry(items, facet, today, max_chars).0
}

/// Drops user-state entries whose item is no longer present, keeping the overlay
/// bounded. `present` is the set of ids currently produced across all facets.
pub fn prune_states(states: &mut HashMap<String, String>, present: &HashSet<String>) {
    states.retain(|id, _| present.contains(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_id_is_stable_and_whitespace_insensitive() {
        let a = item_id("finding", "提出は 6月20日");
        let b = item_id("finding", "  提出は   6月20日  ");
        assert_eq!(a, b);
        assert_ne!(a, item_id("memory", "提出は 6月20日"));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn normalize_dedups_assigns_id_and_drops_expired() {
        let items = vec![
            Item {
                text: "  keep me  ".into(),
                ..Default::default()
            },
            Item {
                text: "keep me".into(),
                ..Default::default()
            },
            Item {
                text: "gone".into(),
                expires_at: "2020-01-01".into(),
                ..Default::default()
            },
        ];
        let out = normalize_items(items, "finding", "2026-06-19", 280);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "keep me");
        assert_eq!(out[0].facet, "finding");
        assert_eq!(out[0].id, item_id("finding", "keep me"));
    }

    #[test]
    fn flat_model_flags_fold_into_structured_flags() {
        let parsed: Item =
            serde_json::from_str(r#"{"text":"submit","expiresAt":"2026-09-01","action":true}"#)
                .expect("deserialize flat item");
        assert_eq!(parsed.text, "submit");
        assert_eq!(parsed.expires_at, "2026-09-01");
        assert!(parsed.flags.action);
        assert!(!parsed.flags.urgent);
    }

    #[test]
    fn prune_states_drops_absent_ids() {
        let mut states: HashMap<String, String> =
            [("a".into(), "done".into()), ("b".into(), "known".into())].into();
        let present: HashSet<String> = ["a".into()].into();
        prune_states(&mut states, &present);
        assert_eq!(states.len(), 1);
        assert!(states.contains_key("a"));
    }
}
