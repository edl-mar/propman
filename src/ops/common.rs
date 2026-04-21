use crate::domain::DomainModel;
use crate::store::KeyId;

/// Apply `value` to `(key_id, locale)` via the domain model.
/// No-ops silently when the key's bundle has no file for `locale`.
pub fn apply_cell_value(dm: &mut DomainModel, key_id: KeyId, locale: &str, value: String) {
    let bundle = dm.bundle_name_for_key(key_id).to_string();
    if !bundle.is_empty() && !dm.bundle_has_locale(&bundle, locale) {
        return;
    }
    dm.set_translation(key_id, locale, value);
}
