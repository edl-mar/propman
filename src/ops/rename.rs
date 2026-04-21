use std::collections::HashSet;
use crate::domain::{self, DomainModel};
use crate::store::{KeyId, NodeId};

/// Rename one exact key. Routes to cross-bundle move when the bundle prefix changes.
/// Returns `Ok(None)` on success, `Ok(Some(msg))` when a status message should be shown,
/// or `Err(msg)` on conflict (caller stays in KeyRenaming mode).
pub fn commit_exact_rename(dm: &mut DomainModel, key_id: KeyId, new_key: String) -> Result<Option<String>, String> {
    let old_key = dm.key_qualified_str(key_id);
    let (old_bundle, _) = domain::split_key(&old_key);
    let (new_bundle, new_real) = domain::split_key(&new_key);
    if old_bundle != new_bundle {
        return commit_cross_bundle_rename(dm, key_id, &old_key, new_key);
    }
    if new_real.is_empty() {
        return Err("Key must have a name after ':'".to_string());
    }
    if dm.find_key(&new_key).is_some() {
        return Err(format!("'{new_key}' already exists"));
    }
    dm.rename_key(key_id, &new_key);
    Ok(None)
}

/// Rename every key whose node equals `anchor` or is a strict descendant of it.
/// Routes to cross-bundle move when the bundle prefix changes.
/// `visible`: currently filter-visible key ids; used when `all_children` is false.
pub fn commit_prefix_rename(dm: &mut DomainModel, anchor: NodeId, new_prefix: String, all_children: bool, visible: &HashSet<KeyId>) -> Result<Option<String>, String> {
    let old_prefix = dm.node_qualified_str(anchor);
    let (old_bundle, _) = domain::split_key(&old_prefix);
    let (new_bundle, new_real) = domain::split_key(&new_prefix);
    if old_bundle != new_bundle {
        return commit_cross_bundle_prefix_rename(dm, anchor, &old_prefix, new_prefix, all_children, visible);
    }
    if new_real.is_empty() {
        return Err("Key must have a name after ':'".to_string());
    }
    let keys_to_rename: Vec<KeyId> = dm.all_key_ids()
        .filter(|&k| {
            let node = dm.key_node_id(k);
            (node == anchor || dm.node_is_strict_ancestor(anchor, node))
                && (all_children || visible.contains(&k))
        })
        .collect();

    for &k in &keys_to_rename {
        let old_k = dm.key_qualified_str(k);
        let new_k = format!("{new_prefix}{}", &old_k[old_prefix.len()..]);
        if let Some(existing) = dm.find_key(&new_k) {
            if !keys_to_rename.contains(&existing) {
                return Err(format!("'{new_k}' already exists"));
            }
        }
    }
    for k in keys_to_rename {
        let old_k = dm.key_qualified_str(k);
        let new_k = format!("{new_prefix}{}", &old_k[old_prefix.len()..]);
        dm.rename_key(k, &new_k);
    }
    Ok(None)
}

/// Copy one exact key to `new_key`, keeping the source untouched.
pub fn commit_exact_copy(dm: &mut DomainModel, key_id: KeyId, new_key: String) -> Result<Option<String>, String> {
    let old_key = dm.key_qualified_str(key_id);
    let (old_bundle, _) = domain::split_key(&old_key);
    let (new_bundle, new_real) = domain::split_key(&new_key);
    if new_real.is_empty() {
        return Err("Key must have a name after ':'".to_string());
    }
    if dm.find_key(&new_key).is_some() {
        return Err(format!("'{new_key}' already exists"));
    }
    if old_bundle != new_bundle && !new_bundle.is_empty() && !dm.is_bundle_name(new_bundle) {
        return Err(format!("Bundle '{new_bundle}' does not exist"));
    }
    let dest_bundle = if new_bundle.is_empty() { old_bundle } else { new_bundle };
    let missed = dm.copy_translations_to(key_id, dest_bundle, new_real);
    let base = format!("Copied {old_key} → {new_key}");
    Ok(Some(if missed.is_empty() {
        base
    } else {
        format!("{base}  (no file for: {})", missed.join(", "))
    }))
}

/// Copy every key whose node equals `anchor` or is a strict descendant of it.
pub fn commit_prefix_copy(dm: &mut DomainModel, anchor: NodeId, new_prefix: String, all_children: bool, visible: &HashSet<KeyId>) -> Result<Option<String>, String> {
    let old_prefix = dm.node_qualified_str(anchor);
    let (old_bundle, _) = domain::split_key(&old_prefix);
    let (new_bundle, new_real) = domain::split_key(&new_prefix);
    if new_real.is_empty() {
        return Err("Key must have a name after ':'".to_string());
    }
    if old_bundle != new_bundle && !new_bundle.is_empty() && !dm.is_bundle_name(new_bundle) {
        return Err(format!("Bundle '{new_bundle}' does not exist"));
    }
    let keys_to_copy: Vec<KeyId> = dm.all_key_ids()
        .filter(|&k| {
            let node = dm.key_node_id(k);
            (node == anchor || dm.node_is_strict_ancestor(anchor, node))
                && (all_children || visible.contains(&k))
        })
        .collect();

    let copy_set: HashSet<KeyId> = keys_to_copy.iter().copied().collect();
    for &k in &keys_to_copy {
        let old_k = dm.key_qualified_str(k);
        let new_k = format!("{new_prefix}{}", &old_k[old_prefix.len()..]);
        if let Some(existing) = dm.find_key(&new_k) {
            if !copy_set.contains(&existing) {
                return Err(format!("'{new_k}' already exists"));
            }
        }
    }
    let dest_bundle = if new_bundle.is_empty() { old_bundle } else { new_bundle };
    let mut all_missed: HashSet<String> = HashSet::new();
    let count = keys_to_copy.len();
    for old_key_id in &keys_to_copy {
        let old_k = dm.key_qualified_str(*old_key_id);
        let new_k = format!("{new_prefix}{}", &old_k[old_prefix.len()..]);
        let (_, new_real_part) = domain::split_key(&new_k);
        let missed = dm.copy_translations_to(*old_key_id, dest_bundle, new_real_part);
        all_missed.extend(missed);
    }
    let base = format!("Copied {count} key(s): {old_prefix} → {new_prefix}");
    Ok(Some(if all_missed.is_empty() {
        base
    } else {
        let mut v: Vec<_> = all_missed.into_iter().collect();
        v.sort();
        format!("{base}  (no file for: {})", v.join(", "))
    }))
}

fn commit_cross_bundle_rename(dm: &mut DomainModel, key_id: KeyId, old_key: &str, new_key: String) -> Result<Option<String>, String> {
    let (new_bundle, new_real) = domain::split_key(&new_key);
    if !dm.is_bundle_name(new_bundle) {
        return Err(format!("Bundle '{new_bundle}' does not exist"));
    }
    if new_real.is_empty() {
        return Err("Key must have a name after ':'".to_string());
    }
    if dm.find_key(&new_key).is_some() {
        return Err(format!("'{new_key}' already exists"));
    }
    let missed = dm.copy_translations_to(key_id, new_bundle, new_real);
    dm.delete_key(key_id);
    let base = format!("Moved {old_key} → {new_key}");
    Ok(Some(if missed.is_empty() {
        base
    } else {
        format!("{base}  (no file for: {})", missed.join(", "))
    }))
}

fn commit_cross_bundle_prefix_rename(dm: &mut DomainModel, anchor: NodeId, old_prefix: &str, new_prefix: String, all_children: bool, visible: &HashSet<KeyId>) -> Result<Option<String>, String> {
    let (new_bundle, new_real) = domain::split_key(&new_prefix);
    if !dm.is_bundle_name(new_bundle) {
        return Err(format!("Bundle '{new_bundle}' does not exist"));
    }
    if new_real.is_empty() {
        return Err("Key must have a name after ':'".to_string());
    }
    let keys_to_move: Vec<KeyId> = dm.all_key_ids()
        .filter(|&k| {
            let node = dm.key_node_id(k);
            (node == anchor || dm.node_is_strict_ancestor(anchor, node))
                && (all_children || visible.contains(&k))
        })
        .collect();

    let move_set: HashSet<KeyId> = keys_to_move.iter().copied().collect();
    for &k in &keys_to_move {
        let old_k = dm.key_qualified_str(k);
        let new_k = format!("{new_prefix}{}", &old_k[old_prefix.len()..]);
        if let Some(existing) = dm.find_key(&new_k) {
            if !move_set.contains(&existing) {
                return Err(format!("'{new_k}' already exists"));
            }
        }
    }
    let mut all_missed: HashSet<String> = HashSet::new();
    let count = keys_to_move.len();
    for old_key_id in keys_to_move {
        let old_k = dm.key_qualified_str(old_key_id);
        let new_k = format!("{new_prefix}{}", &old_k[old_prefix.len()..]);
        let (_, new_real_part) = domain::split_key(&new_k);
        let missed = dm.copy_translations_to(old_key_id, new_bundle, new_real_part);
        all_missed.extend(missed);
        dm.delete_key(old_key_id);
    }
    let base = format!("Moved {count} key(s): {old_prefix} → {new_prefix}");
    Ok(Some(if all_missed.is_empty() {
        base
    } else {
        let mut v: Vec<_> = all_missed.into_iter().collect();
        v.sort();
        format!("{base}  (no file for: {})", v.join(", "))
    }))
}
