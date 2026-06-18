//! System font enumeration for the terminal font picker.
//!
//! Used by the global terminal-font setting and the per-connection font
//! override so the user picks from real installed families instead of typing
//! blindly.

use font_kit::source::SystemSource;

/// Return installed font family names on the host (sorted + deduped) for the
/// font picker. Returns an empty list on failure — the picker then degrades
/// to free-text entry, so a font-enumeration error never blocks the UI.
#[tauri::command]
pub fn list_system_fonts() -> Vec<String> {
    match SystemSource::new().all_families() {
        Ok(mut families) => {
            families.sort();
            families.dedup();
            families
        }
        Err(e) => {
            log::warn!("[fonts] failed to enumerate system fonts: {:?}", e);
            Vec::new()
        }
    }
}
