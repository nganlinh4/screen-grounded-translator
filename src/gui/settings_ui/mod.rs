mod footer;
mod global;
pub mod help_assistant;
mod history;
pub mod node_graph;
mod preset;
mod sidebar;

pub use footer::render_footer;
pub use global::render_global_settings;
pub use history::render_history_panel;
pub use preset::render_preset_editor;
pub use sidebar::get_localized_preset_name;
pub use sidebar::render_sidebar;

#[derive(PartialEq, Clone, Copy)]
pub enum ViewMode {
    Global,
    History,
    Preset(usize),
}
