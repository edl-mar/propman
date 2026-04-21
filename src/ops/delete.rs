use std::collections::HashSet;
use crate::domain::DomainModel;
use crate::store::{KeyId, NodeId};

/// Delete `key_id` from all locale files. Returns the status message.
pub fn delete_key(dm: &mut DomainModel, key_id: KeyId) -> String {
    let display = dm.key_qualified_str(key_id);
    dm.delete_key(key_id);
    format!("Deleted {display}")
}

/// Delete every key whose node equals `anchor` or is a strict descendant of it.
/// When `all_children` is false, only keys in `visible` are deleted.
/// Returns a summary status message.
pub fn delete_key_prefix(dm: &mut DomainModel, anchor: NodeId, all_children: bool, visible: &HashSet<KeyId>) -> String {
    let display = dm.node_qualified_str(anchor);
    let keys: Vec<KeyId> = dm.all_key_ids()
        .filter(|&k| {
            let node = dm.key_node_id(k);
            (node == anchor || dm.node_is_strict_ancestor(anchor, node))
                && (all_children || visible.contains(&k))
        })
        .collect();
    let count = keys.len();
    for key_id in keys {
        dm.delete_key(key_id);
    }
    format!("Deleted {count} key(s) under {display}")
}

/// Delete `key_id`'s entry from a single locale, leaving all other locales
/// untouched. Returns the status message.
pub fn delete_locale_entry(dm: &mut DomainModel, key_id: KeyId, locale: &str) -> String {
    let display = dm.key_qualified_str(key_id);
    dm.remove_translation(key_id, locale);
    format!("Deleted [{locale}] entry for {display}")
}
