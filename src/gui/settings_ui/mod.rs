mod sidebar;
mod global;
mod history;
mod preset;
mod footer;

pub use sidebar::render_sidebar;
pub use sidebar::get_localized_preset_name;
pub use global::render_global_settings;
pub use history::render_history_panel;
pub use preset::render_preset_editor;
pub use footer::render_footer;

#[derive(PartialEq, Clone, Copy)]
pub enum ViewMode {
    Global,
    History,
    Preset(usize),
}
