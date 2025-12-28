use crate::config::Config;
use crate::gui::settings_ui::node_graph::ChainNode;
use crate::gui::settings_ui::ViewMode;
use crate::updater::{UpdateStatus, Updater};
use auto_launch::AutoLaunch;
use egui_snarl::Snarl;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem},
    TrayIcon, TrayIconEvent,
};

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

lazy_static::lazy_static! {
    pub static ref RESTORE_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

pub enum UserEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

pub struct SettingsApp {
    pub(crate) config: Config,
    pub(crate) app_state_ref: Arc<Mutex<crate::AppState>>,
    pub(crate) search_query: String,
    pub(crate) tray_icon: Option<TrayIcon>,
    pub(crate) _tray_menu: Menu,

    pub(crate) tray_settings_item: MenuItem, // Store for dynamic i18n update
    pub(crate) tray_quit_item: MenuItem,     // Store for dynamic i18n update
    pub(crate) tray_favorite_bubble_item: CheckMenuItem, // Store for favorite bubble toggle
    pub(crate) last_ui_language: String,     // Track language to detect changes
    pub(crate) tray_retry_timer: f64,        // Timer for lazy tray icon creation
    pub(crate) event_rx: Receiver<UserEvent>,
    pub(crate) is_quitting: bool,
    pub(crate) run_at_startup: bool,
    pub(crate) auto_launcher: Option<AutoLaunch>,
    pub(crate) show_api_key: bool,
    pub(crate) show_gemini_api_key: bool,
    pub(crate) show_openrouter_api_key: bool,

    pub(crate) view_mode: ViewMode,
    pub(crate) recording_hotkey_for_preset: Option<usize>,
    pub(crate) hotkey_conflict_msg: Option<String>,
    pub(crate) splash: Option<crate::gui::splash::SplashScreen>,
    pub(crate) fade_in_start: Option<f64>,

    // 0 = Init/Offscreen, 1 = Move Sent, 2 = Visible Sent
    pub(crate) startup_stage: u8,

    pub(crate) cached_monitors: Vec<String>,
    pub(crate) cached_audio_devices: Arc<Mutex<Vec<(String, String)>>>,

    pub(crate) updater: Option<Updater>,
    pub(crate) update_rx: Receiver<UpdateStatus>,
    pub(crate) update_status: UpdateStatus,

    // --- NEW FIELDS ---
    pub(crate) current_admin_state: bool, // Track runtime admin status
    pub(crate) last_effective_theme_dark: bool, // Effective dark mode (considering System/Dark/Light)
    pub(crate) last_system_theme_dark: bool,    // Track Windows system theme for icon switching
    pub(crate) theme_check_timer: f64,          // Timer for polling system theme
    // ------------------

    // --- TIP UI STATE ---
    pub(crate) current_tip_idx: usize,
    pub(crate) tip_timer: f64, // Time when the current tip started showing
    pub(crate) tip_fade_state: f32, // 0.0 (Invisible) -> 1.0 (Visible)
    pub(crate) tip_is_fading_in: bool,
    pub(crate) show_tips_modal: bool,
    pub(crate) rng_seed: u32,

    // --- NODE GRAPH STATE ---
    pub(crate) snarl: Option<Snarl<ChainNode>>,
    pub(crate) last_edited_preset_idx: Option<usize>,
    // ------------------------

    // --- USAGE MODAL STATE ---
    pub(crate) show_usage_modal: bool,
    // --- TTS SETTINGS MODAL STATE ---
    pub(crate) show_tts_modal: bool,
    // --------------------

    // --- FAVORITE BUBBLE STATE TRACKING ---
    pub(crate) last_bubble_enabled: bool,
    pub(crate) last_has_favorites: bool,
    // --------------------------------------
}
