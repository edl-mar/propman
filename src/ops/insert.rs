use crate::{domain::DomainModel, store::KeyId};

/// Update an existing translation in the domain model.
pub fn commit_cell_edit(dm: &mut DomainModel, key_id: KeyId, locale: String, new_value: String) {
    dm.set_translation(key_id, &locale, new_value);
}

/// Insert a new translation into the domain model.
/// Returns `Err` when the bundle has no locale file for `locale`.
pub fn commit_cell_insert(dm: &mut DomainModel, key_id: KeyId, locale: String, new_value: String) -> Result<(), String> {
    let bundle = dm.bundle_name_for_key(key_id).to_string();
    if !bundle.is_empty() && !dm.bundle_has_locale(&bundle, &locale) {
        return Err(format!("No [{locale}] file in bundle '{bundle}' — create it first"));
    }
    dm.set_translation(key_id, &locale, new_value);
    Ok(())
}
