pub mod menu;
pub mod navigation;
pub mod tabs;

use std::sync::atomic::{AtomicU32, Ordering};

/// Terminal rows above the page that the UI asks for: the navigation bar,
/// plus the tab strip when there are two or more tabs. The window reads it
/// when it sizes the page; the renderer sets it and then triggers a resize.
static UI_ROWS: AtomicU32 = AtomicU32::new(1);

pub fn ui_rows() -> u32 {
    UI_ROWS.load(Ordering::Relaxed)
}

/// Returns whether the value changed (a resize is then needed).
pub fn set_ui_rows(rows: u32) -> bool {
    UI_ROWS.swap(rows, Ordering::Relaxed) != rows
}
