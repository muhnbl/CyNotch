use crate::memreduct::{CleanAreas, CleanResult};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Instant;

/// Per-user state directory (`%LOCALAPPDATA%\CyNotch`).
///
/// Resolved from the environment instead of a literal path so an install on any
/// account works. The fallback never names a specific user: if `LOCALAPPDATA` is
/// missing (only possible in a stripped-down service context) the temp directory
/// is used, so the app still starts and writes somewhere it owns.
pub fn appdata_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("CyNotch")
}

/// Path for a crash/diagnostic log inside [`appdata_dir`].
pub fn crash_log_path(name: &str) -> std::path::PathBuf {
    appdata_dir().join(name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotchStyle {
    Floating,
    EdgeFlush,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotchPlacement {
    Top,
    Left,
    Right,
}

impl NotchPlacement {
    pub fn is_vertical(self) -> bool {
        !matches!(self, Self::Top)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotchSizePreset {
    Compact,
    Standard,
    Wide,
}

impl NotchSizePreset {
    pub fn idle_width(self) -> f32 {
        match self {
            Self::Compact => 260.0,
            Self::Standard => 330.0,
            Self::Wide => 412.0,
        }
    }

    pub fn idle_height_horizontal(self) -> f32 {
        match self {
            Self::Compact => 40.0,
            Self::Standard => 42.0,
            Self::Wide => 46.0,
        }
    }

    pub fn idle_width_vertical(self) -> f32 {
        match self {
            Self::Compact => 46.0,
            Self::Standard => 52.0,
            Self::Wide => 60.0,
        }
    }

    pub fn idle_height_vertical(self) -> f32 {
        match self {
            Self::Compact => 156.0,
            Self::Standard => 204.0,
            Self::Wide => 256.0,
        }
    }

    pub fn minimized_length(self) -> f32 {
        match self {
            Self::Compact => 68.0,
            Self::Standard => 86.0,
            Self::Wide => 118.0,
        }
    }

    pub fn minimized_thickness(self) -> f32 {
        match self {
            Self::Compact => 16.0,
            Self::Standard => 18.0,
            Self::Wide => 20.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpandedSizePreset {
    Compact,
    Standard,
    Large,
}

impl ExpandedSizePreset {
    pub fn scale(self) -> f32 {
        match self {
            Self::Compact => 0.90,
            Self::Standard => 1.00,
            Self::Large => 1.15,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Standard => "Standard",
            Self::Large => "Large",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdleMetric {
    Ram,
    NetSpeed,
    Cpu,
    Clock,
    Minimal,
}

impl IdleMetric {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ram => "RAM",
            Self::NetSpeed => "Net",
            Self::Cpu => "CPU",
            Self::Clock => "Clock",
            Self::Minimal => "Min",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeScale {
    Compact,
    Standard,
    Large,
    ExtraLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HoverDwellSpeed {
    Quick,  // 120ms
    #[default]
    Normal, // 240ms
    Slow,   // 450ms
}

impl HoverDwellSpeed {
    pub fn ms(self) -> u64 {
        match self {
            Self::Quick => 120,
            Self::Normal => 240,
            Self::Slow => 450,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Quick => "Quick (120ms)",
            Self::Normal => "Normal (240ms)",
            Self::Slow => "Slow (450ms)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Quick => "Q",
            Self::Normal => "N",
            Self::Slow => "S",
        }
    }
}

impl TypeScale {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Standard => "Desktop",
            Self::Large => "Large",
            Self::ExtraLarge => "X-Large",
        }
    }

    /// Proportional UI container / button / control scale factor for each preset.
    /// Keeps buttons, spacing, and banners responsive alongside typography.
    pub fn ui_scale(self) -> f32 {
        match self {
            Self::Compact => 1.00,
            Self::Standard => 1.09,
            Self::Large => 1.22,
            Self::ExtraLarge => 1.38,
        }
    }

    /// Gentler scale factor for non-minimized idle capsules so the top/edge bar
    /// stays sleek while text remains crisp across desktop scales.
    pub fn idle_ui_scale(self) -> f32 {
        match self {
            Self::Compact => 1.00,
            Self::Standard => 1.05,
            Self::Large => 1.13,
            Self::ExtraLarge => 1.24,
        }
    }

    /// Pre-transform (`p.s`) desktop legibility curve: eliminates tiny `<10.8px`
    /// captions even in `Compact`, while giving `Large` / `X-Large` balanced
    /// additional breathing room on top of responsive `ui_scale`.
    pub fn caption_lift_px(self, raw_size: f32) -> f32 {
        if raw_size <= 9.0 {
            return (raw_size + 0.45).max(8.8);
        }
        let lifted = if raw_size < 14.0 {
            10.8 + (raw_size - 9.2) * (3.2 / 4.8)
        } else {
            raw_size
        };
        let tier_bonus = match self {
            Self::Compact => 1.00,
            Self::Standard => 1.015,
            Self::Large => 1.03,
            Self::ExtraLarge => 1.05,
        };
        lifted * tier_bonus
    }

    /// Net rendered pixel font size at the preset's responsive `ui_scale`.
    pub fn scale_px(self, raw_size: f32) -> f32 {
        self.caption_lift_px(raw_size) * self.ui_scale()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LyricSize {
    Compact,
    Standard,
    Large,
}

impl LyricSize {
    pub fn next(self) -> Self {
        match self {
            Self::Compact => Self::Standard,
            Self::Standard => Self::Large,
            Self::Large => Self::Compact,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Standard => "Standard",
            Self::Large => "Large",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Compact => "S",
            Self::Standard => "M",
            Self::Large => "L",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LyricStyle {
    ClassicList,
    KaraokeGlow,
    MinimalSingle,
    CompactStream,
}

impl LyricStyle {
    pub fn next(self) -> Self {
        match self {
            Self::ClassicList => Self::KaraokeGlow,
            Self::KaraokeGlow => Self::MinimalSingle,
            Self::MinimalSingle => Self::CompactStream,
            Self::CompactStream => Self::ClassicList,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ClassicList => "Classic",
            Self::KaraokeGlow => "Karaoke",
            Self::MinimalSingle => "Minimal",
            Self::CompactStream => "Stream",
        }
    }

    pub fn short_label(self) -> &'static str {
        self.label()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    General,
    Motion,
    Modules,
    Notifications,
    System,
}

impl SettingsCategory {
    pub const COUNT: usize = 5;
    pub const ALL: [SettingsCategory; 5] = [
        SettingsCategory::General,
        SettingsCategory::Motion,
        SettingsCategory::Modules,
        SettingsCategory::Notifications,
        SettingsCategory::System,
    ];

    pub fn index(self) -> usize {
        match self {
            Self::General => 0,
            Self::Motion => 1,
            Self::Modules => 2,
            Self::Notifications => 3,
            Self::System => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General & Notch",
            Self::Motion => "Motion & Perf",
            Self::Modules => "Dock & Modules",
            Self::Notifications => "Notifications",
            Self::System => "Memory & System",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleVisibility {
    pub music: bool,
    pub volume: bool,
    pub mem_reduct: bool,
    pub tasks: bool,
    pub shelf: bool,
    pub capture: bool,
    pub telemetry: bool,
}

impl Default for ModuleVisibility {
    fn default() -> Self {
        Self {
            music: true,
            volume: true,
            mem_reduct: true,
            tasks: true,
            shelf: true,
            capture: true,
            telemetry: true,
        }
    }
}

#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleId {
    Autostart = 0,
    Chime = 1,
    ClickToOpen = 2,
    HideFullscreen = 3,
    ShowMascot = 4,
    ProgressArc = 5,
    MicroInteractions = 6,
    Equalizer = 7,
    Notifications = 8,
    LyricsMonitor = 9,
    ClipboardWatch = 10,
    CaptureClipboard = 11,
    AutoClean = 12,
    AreaWs = 13,
    AreaSys = 14,
    AreaStandby = 15,
    ScreenTime = 16,
    ModMusic = 17,
    ModVolume = 18,
    ModMemReduct = 19,
    ModTasks = 21,
    ModShelf = 22,
    ModCapture = 23,
    ModTelemetry = 24,
    MinimizedIsland = 25,
    AreaModPage = 26,
    AreaStandbyLow = 27,
    AreaModFile = 28,
    AreaRegistry = 29,
    AreaCombine = 30,
    ShowCleanResults = 31,
    FrostedGlass = 33,
    NotifListener = 34,
    NotifExclusive = 35,
    NotifAutoClear = 36,
    NotifOverFullscreen = 37,
    RemoveWindowsBorders = 38,
}

impl ToggleId {
    pub const ALL: [ToggleId; 37] = [
        ToggleId::Autostart,
        ToggleId::Chime,
        ToggleId::ClickToOpen,
        ToggleId::HideFullscreen,
        ToggleId::ShowMascot,
        ToggleId::ProgressArc,
        ToggleId::MicroInteractions,
        ToggleId::Equalizer,
        ToggleId::Notifications,
        ToggleId::LyricsMonitor,
        ToggleId::ClipboardWatch,
        ToggleId::CaptureClipboard,
        ToggleId::AutoClean,
        ToggleId::AreaWs,
        ToggleId::AreaSys,
        ToggleId::AreaStandby,
        ToggleId::ScreenTime,
        ToggleId::ModMusic,
        ToggleId::ModVolume,
        ToggleId::ModMemReduct,
        ToggleId::ModTasks,
        ToggleId::ModShelf,
        ToggleId::ModCapture,
        ToggleId::ModTelemetry,
        ToggleId::MinimizedIsland,
        ToggleId::AreaModPage,
        ToggleId::AreaStandbyLow,
        ToggleId::AreaModFile,
        ToggleId::AreaRegistry,
        ToggleId::AreaCombine,
        ToggleId::ShowCleanResults,
        ToggleId::FrostedGlass,
        ToggleId::NotifListener,
        ToggleId::NotifExclusive,
        ToggleId::NotifAutoClear,
        ToggleId::NotifOverFullscreen,
        ToggleId::RemoveWindowsBorders,
    ];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingEffects {
    pub redraw_notch: bool,
    pub update_autostart: Option<bool>,
    pub reset_screentime: bool,
    pub trigger_purge: bool,
    pub quit_app: bool,
    pub close_settings: bool,
    pub open_win_notif_settings: bool,
    pub open_win_personalize_colors: bool,
    pub sync_frosted_window: bool,
    pub sync_windows_borders: Option<bool>,
    pub sync_exclusive_banners: Option<bool>,
    pub request_notif_access: bool,
    pub play_sound: Option<crate::sound::NotificationSound>,
    pub sync_win_notif_sound_mute: Option<bool>,
}

/// Pure exponential decay toward `target` with zero overshoot or bounce.
pub fn ease_exp(cur: f32, target: f32, speed: f32, dt: f32, eps: f32) -> (f32, bool) {
    let diff = target - cur;
    if diff.abs() <= eps {
        return (target, (target - cur).abs() > 1e-5);
    }
    let alpha = (1.0 - (-speed * dt.max(0.0)).exp()).clamp(0.0, 1.0);
    let next = cur + diff * alpha;
    if (target - next).abs() <= eps {
        (target, true)
    } else {
        (next, true)
    }
}

/// Framer-Motion-style damped spring integrator (semi-implicit Euler with substeps
/// for stability across 60Hz-240Hz refresh rates).
pub fn spring_step(
    cur: f32,
    vel: f32,
    target: f32,
    stiffness: f32,
    damping: f32,
    dt: f32,
    pos_eps: f32,
    vel_eps: f32,
) -> (f32, f32, bool) {
    if (target - cur).abs() <= pos_eps && vel.abs() <= vel_eps {
        return (target, 0.0, (target - cur).abs() > 1e-5 || vel.abs() > 1e-5);
    }
    let total_dt = dt.clamp(0.0, 0.05);
    if total_dt <= 1e-6 {
        return (cur, vel, false);
    }
    let steps = if total_dt > 0.014 { 3 } else if total_dt > 0.008 { 2 } else { 1 };
    let h = total_dt / steps as f32;
    let mut x = cur;
    let mut v = vel;
    for _ in 0..steps {
        let force = (target - x) * stiffness - v * damping;
        v += force * h;
        x += v * h;
    }
    if (target - x).abs() <= pos_eps && v.abs() <= vel_eps {
        (target, 0.0, true)
    } else {
        (x, v, true)
    }
}

/// Smooth cubic ease-out curve for directional view/pane glides.
pub fn ease_out_cubic(t: f32) -> f32 {
    let c = t.clamp(0.0, 1.0);
    1.0 - (1.0 - c).powi(3)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotchMode {
    Home,
    Music,
    Volume,
    MemReduct,
    Tasks,
    Shelf,
    Capture,
    Telemetry,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Pasted,
    MemoryPurge,
    Capture,
    General,
    SystemToast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MascotMood {
    Idle,
    Purging,
    Music,
    Capture,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    SetMode(NotchMode),
    TogglePin,
    StartGripDrag,
    ResetPosition,
    ToggleLyrics,
    ScrollLyrics(i32),
    CycleLyricSize,
    CycleLyricStyle,
    SetLyricSize(LyricSize),
    SetLyricStyle(LyricStyle),
    PrevTrack,
    TogglePlayPause,
    NextTrack,
    PurgeRam,
    ToggleAutoClean,
    ToggleArea(u8), // 0..=7 matching Mem Reduct areas
    CycleThreshold, // 70 -> 80 -> 90 -> 0 (Disable) -> 60 -> 70
    CycleInterval,  // 0 (Disable) -> 5 -> 15 -> 30 -> 60 -> 90 -> 0
    SetCleanThreshold(u32),
    SetCleanInterval(u32),
    ToggleShowCleanResults,
    ToggleTask(usize),
    DeleteTask(usize),
    ClearCompletedTasks,
    PasteAsTask,
    ActivateShelfItem(usize),
    DeleteShelfItem(usize),
    ClearShelf,
    CaptureScreen,
    CaptureWindow,
    OpenCaptureFolder,
    SeekMusic(f32),
    SetVolume(f32),
    ToggleMute,
    OpenSettings,
    CloseSettings,
    SetSettingsCategory(SettingsCategory),
    SetNotchStyle(NotchStyle),
    SetPlacement(NotchPlacement),
    SetSizePreset(NotchSizePreset),
    SetExpandedSize(ExpandedSizePreset),
    SetTypeScale(TypeScale),
    SetIdleMetric(IdleMetric),
    ToggleMinimizedIsland,
    ToggleClickToOpen,
    ToggleHideFullscreen,
    ToggleShowMascot,
    ToggleProgressArc,
    SetFpsCap(u32),
    ToggleMicroInteractions,
    ToggleEqualizer,
    ToggleNotifications,
    SetNotificationSecs(u32),
    SetNotificationSound(crate::sound::NotificationSound),
    SetHoverDwellSpeed(HoverDwellSpeed),
    TestNotificationSound,
    ToggleFrostedGlass,
    ToggleRemoveWindowsBorders,
    SetFrostedOpacity(u32),
    SetFrostedBlur(u32),
    ToggleNotifListener,
    ToggleNotifExclusive,
    ToggleNotifAutoClear,
    ToggleNotifOverFullscreen,
    OpenWinNotifSettings,
    OpenWinPersonalizeColors,
    SendTestNotification,
    DismissNotification,
    ActivateNotification,
    ClearNotificationQueue,
    ToggleNotifHistory,
    ActivateHistoryItem(usize),
    DismissHistoryItem(usize),
    ClearNotifHistory,
    ToggleModule(NotchMode),
    ToggleLyricsMonitor,
    ToggleClipboardWatch,
    ToggleCaptureClipboard,
    ToggleScreenTime,
    ResetScreenTimeToday,
    ResetAllSettings,
    ToggleAutostart,
    ToggleChime,
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskItem {
    pub text: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShelfKind {
    Text,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShelfItem {
    pub kind: ShelfKind,
    pub label: String,
    pub payload: String,
}

#[derive(Debug, Clone, Default)]
pub struct CaptureState {
    pub last_path: Option<String>,
    pub last_filename: String,
    pub last_dims: String,
    pub clipboard_ok: bool,
    pub total_captures: u32,
}

#[derive(Debug, Clone)]
pub struct ScreenTimeStats {
    pub today_secs: u64,
    pub top_app_name: String,
    pub top_app_secs: u64,
    pub top_apps: Vec<(String, u64)>,
    pub seven_day_avg_secs: u64,
    pub tracked_days: usize,
    pub peak_hour: usize,
    pub peak_hour_str: String,
    pub hourly_secs: [u64; 24],
}

impl Default for ScreenTimeStats {
    fn default() -> Self {
        Self {
            today_secs: 0,
            top_app_name: "--".to_string(),
            top_app_secs: 0,
            top_apps: Vec::new(),
            seven_day_avg_secs: 0,
            tracked_days: 1,
            peak_hour: 0,
            peak_hour_str: "--".to_string(),
            hourly_secs: [0u64; 24],
        }
    }
}

impl ScreenTimeStats {
    pub fn preview_sample() -> Self {
        let mut hourly = [0u64; 24];
        hourly[10] = 1400;
        hourly[11] = 2200;
        hourly[14] = 3800;
        hourly[15] = 2900;
        Self {
            today_secs: 2 * 3600 + 2 * 60,
            top_app_name: "Chrome".to_string(),
            top_app_secs: 3600 + 5 * 60,
            top_apps: vec![
                ("Chrome".to_string(), 3900),
                ("VS Code".to_string(), 2400),
                ("Terminal".to_string(), 1020),
            ],
            seven_day_avg_secs: 3600 + 48 * 60,
            tracked_days: 3,
            peak_hour: 14,
            peak_hour_str: "2 PM".to_string(),
            hourly_secs: hourly,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NotificationBanner {
    pub app_name: String,
    pub aumid: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub kind: NotificationKind,
    pub is_success: bool,
    pub total_secs: f32,
    pub expires_at: Instant,
    pub grouped_count: u32,
    pub received_at: Instant,
}

#[derive(Debug, Clone)]
pub struct NotifHistoryEntry {
    pub app_name: String,
    pub aumid: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub received_at: Instant,
}

#[derive(Debug, Clone)]
pub struct MediaTrack {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub is_playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
}

impl Default for MediaTrack {
    fn default() -> Self {
        Self::idle()
    }
}

impl MediaTrack {
    pub fn idle() -> Self {
        Self {
            title: "No Media Playing".to_string(),
            artist: "Open Spotify, YT, or player".to_string(),
            album: String::new(),
            is_playing: false,
            position_ms: 0,
            duration_ms: 0,
        }
    }

    pub fn preview_sample() -> Self {
        Self {
            title: "Golden Hour Drive".to_string(),
            artist: "Northbound".to_string(),
            album: String::new(),
            is_playing: true,
            position_ms: 63_000,
            duration_ms: 214_000,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.title == "No Media Playing" || self.title.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLine {
    pub seconds: f64,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct HardwareTelemetry {
    pub cpu_usage: f32,
    pub memory_used_gb: f32,
    pub memory_total_gb: f32,
    pub memory_percent: f32,
    pub pagefile_used_gb: f32,
    pub pagefile_total_gb: f32,
    pub pagefile_percent: f32,
    pub sys_cache_mb: f32,
    pub sys_cache_peak_mb: f32,
    pub sys_cache_percent: f32,
    pub net_down_kbps: f32,
    pub net_up_kbps: f32,
    pub has_admin_privileges: bool,
    pub on_ac: bool,
    pub battery_pct: Option<u8>,
}

impl HardwareTelemetry {
    pub fn initial_real() -> Self {
        let (pools, on_ac, battery_pct) = crate::telemetry::poll_memory_and_power();
        Self {
            cpu_usage: 0.0,
            memory_used_gb: pools.phys_used_gb,
            memory_total_gb: pools.phys_total_gb,
            memory_percent: pools.phys_percent,
            pagefile_used_gb: pools.page_used_gb,
            pagefile_total_gb: pools.page_total_gb,
            pagefile_percent: pools.page_percent,
            sys_cache_mb: pools.sys_cache_mb,
            sys_cache_peak_mb: pools.sys_cache_peak_mb,
            sys_cache_percent: pools.sys_cache_percent,
            net_down_kbps: 0.0,
            net_up_kbps: 0.0,
            has_admin_privileges: pools.is_elevated,
            on_ac,
            battery_pct,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemReductConfig {
    pub clean_working_sets: bool,
    pub clean_system_ws: bool,
    pub clean_standby_list: bool,
    pub clean_modified_page_list: bool,
    pub clean_standby_low: bool,
    pub clean_modified_file_cache: bool,
    pub clean_registry_cache: bool,
    pub clean_combine_memory_lists: bool,
    pub show_clean_results: bool,
    pub when_above_pct: u32,
    pub every_mins: u32,
    pub auto_clean_enabled: bool,
    pub last_cleaned_status: String,
    pub last_freed_mb: f64,
    pub is_purging: bool,
    pub cyno_quote: String,
}

impl Default for MemReductConfig {
    fn default() -> Self {
        Self {
            clean_working_sets: true,
            clean_system_ws: true,
            clean_standby_list: true,
            clean_modified_page_list: true,
            clean_standby_low: false,
            clean_modified_file_cache: false,
            clean_registry_cache: false,
            clean_combine_memory_lists: false,
            show_clean_results: true,
            when_above_pct: 70,
            every_mins: 0,
            auto_clean_enabled: true,
            last_cleaned_status: "Ready • Click to purge standby & WS".to_string(),
            last_freed_mb: 0.0,
            is_purging: false,
            cyno_quote: "Through me, the law is made manifest.".to_string(),
        }
    }
}

impl MemReductConfig {
    pub fn preview_sample() -> Self {
        Self {
            last_cleaned_status: "14 procs trimmed • 420 MB freed".to_string(),
            last_freed_mb: 420.0,
            ..Self::default()
        }
    }

    pub fn to_clean_areas(&self) -> CleanAreas {
        CleanAreas {
            working_sets: self.clean_working_sets,
            system_file_cache: self.clean_system_ws,
            modified_page_list: self.clean_modified_page_list,
            standby_list: self.clean_standby_list,
            standby_list_low: self.clean_standby_low,
            modified_file_cache: self.clean_modified_file_cache,
            registry_cache: self.clean_registry_cache,
            combine_memory_lists: self.clean_combine_memory_lists,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub mode: NotchMode,
    pub is_hovered: bool,
    pub is_hover_pending: bool,
    pub hover_dwell_t: f32,
    pub anim_edge_glow_t: f32,
    pub hover_dwell_start: Option<std::time::Instant>,
    pub is_pinned: bool,
    pub x_offset: i32,
    pub y_offset: i32,
    pub show_lyrics: bool,
    pub lyrics_offset: i32,

    // Advanced Shell & Customization Settings (Nusa / Layzee parity)
    pub notch_style: NotchStyle,
    pub placement: NotchPlacement,
    pub size_preset: NotchSizePreset,
    pub expanded_size: ExpandedSizePreset,
    pub type_scale: TypeScale,
    pub idle_metric: IdleMetric,
    pub minimized_island: bool,
    pub click_to_open: bool,
    pub hide_in_fullscreen: bool,
    pub show_mascot: bool,
    pub progress_arc_enabled: bool,
    pub fps_cap: u32,
    pub detected_hz: u32,
    pub micro_interactions: bool,
    pub equalizer_enabled: bool,
    pub notifications_enabled: bool,
    pub notification_secs: u32,
    pub notification_sound: crate::sound::NotificationSound,
    pub notification_volume: u32,
    pub hover_dwell_speed: HoverDwellSpeed,
    pub seen_hotkey_hint: bool,
    pub hotkey_hint_expand_count: u8,
    pub lyric_size: LyricSize,
    pub lyric_style: LyricStyle,
    pub frosted_glass_enabled: bool,
    pub frosted_opacity_pct: u32,
    pub frosted_blur_px: u32,
    pub windows_transparency_enabled: bool,
    pub windows_animations_enabled: bool,
    pub remove_windows_borders: bool,
    pub notif_listener_enabled: bool,
    pub notif_exclusive_mode: bool,
    pub notif_auto_clear_action_center: bool,
    pub notif_over_fullscreen: bool,
    pub notif_listener_status: String,
    pub notif_suppressed_apps: usize,
    pub modules: ModuleVisibility,
    pub lyrics_enabled: bool,
    pub clipboard_watch_enabled: bool,
    pub capture_to_clipboard: bool,
    pub screentime_enabled: bool,

    // Settings Window UI State
    pub settings_open: bool,
    pub settings_category: SettingsCategory,
    pub settings_scroll: [f32; SettingsCategory::COUNT],
    pub settings_scroll_target: [f32; SettingsCategory::COUNT],

    // Smooth Framer-Motion Spring & Choreography Animation parameters
    pub current_width: f32,
    pub current_height: f32,
    pub target_width: f32,
    pub target_height: f32,
    pub anim_width_vel: f32,
    pub anim_height_vel: f32,
    pub anim_expand_t: f32,
    pub anim_expand_vel: f32,
    pub anim_notif_t: f32,
    pub anim_mode_t: f32,
    pub anim_mode_dir: f32,
    pub target_anchor_x: f32,
    pub target_anchor_y: f32,
    pub anim_anchor_x: f32,
    pub anim_anchor_y: f32,
    pub anim_dock_x: f32,
    pub anim_dock_w: f32,
    pub anim_dock_x_vel: f32,
    pub anim_dock_w_vel: f32,
    pub anim_settings_cat_y: f32,
    pub anim_settings_cat_vel: f32,
    pub anim_settings_pane_t: f32,
    pub anim_settings_pane_dir: f32,
    pub anim_vol_pct: f32,
    pub anim_ram_pct: f32,
    pub hover_target_idx: Option<usize>,
    pub hover_slots: [f32; 48],
    pub settings_hover_idx: Option<usize>,
    pub settings_hover_slots: [f32; 48],
    pub switch_t: [f32; 48],

    // Core Modules
    pub media: MediaTrack,
    pub lyrics: Vec<LyricLine>,
    pub current_lyric_index: Option<usize>,
    pub current_lyric_text: String,
    pub telemetry: HardwareTelemetry,
    pub mem_reduct: MemReductConfig,
    pub capture: CaptureState,
    pub quick_note: String,
    pub screen_time: ScreenTimeStats,
    pub notification: Option<NotificationBanner>,
    pub notif_queue: VecDeque<NotificationBanner>,
    pub notif_history: VecDeque<NotifHistoryEntry>,
    pub show_notif_history: bool,
    pub purge_anim_until: Option<Instant>,
    pub shelf_items: Vec<ShelfItem>,
    pub shelf_scroll: usize,
    pub tasks: Vec<TaskItem>,
    pub tasks_scroll: usize,
    pub anim_time: f32,
    pub volume_pct: f32,
    pub pre_mute_volume: f32,
    pub autostart: bool,
    pub chime_enabled: bool,
    pub persistence_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedConfig {
    chime_enabled: bool,
    clean_working_sets: bool,
    clean_system_ws: bool,
    clean_standby_list: bool,
    #[serde(default = "default_true")]
    clean_modified_page_list: bool,
    #[serde(default)]
    clean_standby_low: bool,
    #[serde(default)]
    clean_modified_file_cache: bool,
    #[serde(default)]
    clean_registry_cache: bool,
    #[serde(default)]
    clean_combine_memory_lists: bool,
    #[serde(default = "default_true")]
    show_clean_results: bool,
    when_above_pct: u32,
    every_mins: u32,
    #[serde(default = "default_true")]
    auto_clean_enabled: bool,
    #[serde(default)]
    is_pinned: bool,
    #[serde(default)]
    x_offset: i32,
    #[serde(default)]
    y_offset: i32,
    #[serde(default)]
    last_freed_mb: f64,
    #[serde(default)]
    last_cleaned_status: String,
    #[serde(default = "default_notch_style")]
    notch_style: NotchStyle,
    #[serde(default = "default_placement")]
    placement: NotchPlacement,
    #[serde(default = "default_size_preset")]
    size_preset: NotchSizePreset,
    #[serde(default = "default_expanded_size")]
    expanded_size: ExpandedSizePreset,
    #[serde(default = "default_type_scale")]
    type_scale: TypeScale,
        #[serde(default = "default_idle_metric")]
        idle_metric: IdleMetric,
        #[serde(default)]
        minimized_island: bool,
        #[serde(default)]
        click_to_open: bool,
        #[serde(default = "default_true")]
        hide_in_fullscreen: bool,
        #[serde(default = "default_true")]
        show_mascot: bool,
        #[serde(default = "default_true", alias = "status_ring_enabled")]
        progress_arc_enabled: bool,
        #[serde(default)]
        fps_cap: u32,
        #[serde(default = "default_true")]
        micro_interactions: bool,
        #[serde(default = "default_true")]
        equalizer_enabled: bool,
        #[serde(default = "default_true")]
        notifications_enabled: bool,
        #[serde(default = "default_notif_secs")]
        notification_secs: u32,
        #[serde(default)]
        notification_sound: crate::sound::NotificationSound,
        #[serde(default = "default_notif_volume")]
        notification_volume: u32,
        #[serde(default)]
        hover_dwell_speed: HoverDwellSpeed,
        #[serde(default)]
        seen_hotkey_hint: bool,
        #[serde(default)]
        modules: ModuleVisibility,
        #[serde(default = "default_true")]
        lyrics_enabled: bool,
        #[serde(default = "default_true")]
        clipboard_watch_enabled: bool,
        #[serde(default = "default_true")]
        capture_to_clipboard: bool,
        #[serde(default = "default_true")]
        screentime_enabled: bool,
        #[serde(default = "default_lyric_size")]
        lyric_size: LyricSize,
        #[serde(default = "default_lyric_style")]
        lyric_style: LyricStyle,
        #[serde(default = "default_true")]
        frosted_glass_enabled: bool,
        #[serde(default = "default_frosted_opacity")]
        frosted_opacity_pct: u32,
        #[serde(default = "default_frosted_blur")]
        frosted_blur_px: u32,
        #[serde(default)]
        remove_windows_borders: bool,
        #[serde(default)]
        anchor_x: Option<f32>,
        #[serde(default)]
        anchor_y: Option<f32>,
        #[serde(default = "default_true")]
        notif_listener_enabled: bool,
        #[serde(default)]
        notif_exclusive_mode: bool,
        #[serde(default)]
        notif_auto_clear_action_center: bool,
        #[serde(default)]
        notif_over_fullscreen: bool,
        /// Config schema revision; < 2 means saved before the bridge became default-on.
        #[serde(default)]
        config_rev: u32,
    }

fn default_true() -> bool {
    true
}

/// v2: Windows Notification Bridge is ON by default (configs saved earlier are migrated to ON once).
const CONFIG_REV: u32 = 2;

impl PersistedConfig {
    fn listener_effective(&self) -> bool {
        self.notif_listener_enabled || self.config_rev < 2
    }
}
fn default_notch_style() -> NotchStyle {
    NotchStyle::Floating
}
fn default_placement() -> NotchPlacement {
    NotchPlacement::Top
}
fn default_size_preset() -> NotchSizePreset {
    NotchSizePreset::Standard
}
fn default_expanded_size() -> ExpandedSizePreset {
    ExpandedSizePreset::Standard
}
fn default_type_scale() -> TypeScale {
    TypeScale::Standard
}
fn default_idle_metric() -> IdleMetric {
    IdleMetric::Ram
}
fn default_lyric_size() -> LyricSize {
    LyricSize::Standard
}
fn default_lyric_style() -> LyricStyle {
    LyricStyle::ClassicList
}
fn default_notif_secs() -> u32 {
    4
}

fn default_notif_volume() -> u32 {
    80
}
fn default_frosted_opacity() -> u32 {
    68
}
fn default_frosted_blur() -> u32 {
    20
}

impl AppState {
    pub const MINIMIZED_WIDTH: f32 = 86.0;
    pub const MINIMIZED_HEIGHT: f32 = 18.0;
    pub const MINIMIZED_VERT_WIDTH: f32 = 18.0;
    pub const MINIMIZED_VERT_HEIGHT: f32 = 86.0;

    pub const IDLE_WIDTH: f32 = 330.0;
    pub const IDLE_HEIGHT: f32 = 42.0;
    pub const VERT_IDLE_WIDTH: f32 = 52.0;
    pub const VERT_IDLE_HEIGHT: f32 = 204.0;

    pub const BANNER_WIDTH: f32 = 424.0;
    pub const BANNER_HEIGHT: f32 = 80.0;
    pub const VERT_BANNER_WIDTH: f32 = 356.0;
    pub const VERT_BANNER_HEIGHT: f32 = 90.0;

    pub const EXPANDED_WIDTH: f32 = 620.0;
    pub const EXPANDED_HEIGHT: f32 = 232.0;
    pub const VERT_EXPANDED_WIDTH: f32 = 316.0;
    pub const VERT_EXPANDED_HEIGHT: f32 = 480.0;

    pub const SETTINGS_WIDTH: f32 = 860.0;
    pub const SETTINGS_HEIGHT: f32 = 720.0;

    pub const VOLUME_SLIDER_X: f32 = 200.0;
    pub const VOLUME_SLIDER_W: f32 = 380.0;
    pub const VERT_VOLUME_SLIDER_X: f32 = 32.0;
    pub const VERT_VOLUME_SLIDER_W: f32 = 252.0;

    pub const MUSIC_SCRUBBER_X: f32 = 206.0;
    pub const MUSIC_SCRUBBER_W: f32 = 376.0;
    pub const VERT_MUSIC_SCRUBBER_X: f32 = 32.0;
    pub const VERT_MUSIC_SCRUBBER_W: f32 = 252.0;

    pub fn base_expanded_width(&self) -> f32 {
        if self.placement.is_vertical() {
            Self::VERT_EXPANDED_WIDTH
        } else {
            Self::EXPANDED_WIDTH
        }
    }

    pub fn base_expanded_height(&self) -> f32 {
        if self.placement.is_vertical() {
            Self::VERT_EXPANDED_HEIGHT
        } else {
            Self::EXPANDED_HEIGHT
        }
    }

    pub fn expanded_scale(&self) -> f32 {
        self.expanded_size.scale() * self.type_scale.ui_scale()
    }

    pub fn banner_scale(&self) -> f32 {
        self.type_scale.ui_scale()
    }

    pub fn settings_width(&self) -> f32 {
        (Self::SETTINGS_WIDTH * self.type_scale.ui_scale()).round()
    }

    pub fn settings_height(&self) -> f32 {
        (Self::SETTINGS_HEIGHT * self.type_scale.ui_scale()).round()
    }

    pub fn expanded_width(&self) -> f32 {
        (self.base_expanded_width() * self.expanded_scale()).round()
    }

    pub fn expanded_height(&self) -> f32 {
        (self.base_expanded_height() * self.expanded_scale()).round()
    }

    pub fn canvas_width(&self) -> u32 {
        self.expanded_width()
            .max(self.collapsed_width())
            .max(self.idle_width())
            .max(self.current_width)
            .ceil()
            .max(1.0) as u32
    }

    pub fn canvas_height(&self) -> u32 {
        self.expanded_height()
            .max(self.collapsed_height())
            .max(self.idle_height())
            .max(self.current_height)
            .ceil()
            .max(1.0) as u32
    }

    /// Collapsed target width: pops out to a readable toast banner whenever an active notification is shown.
    pub fn collapsed_width(&self) -> f32 {
        if self.notification.is_some() {
            let base = if self.placement.is_vertical() {
                Self::VERT_BANNER_WIDTH
            } else {
                Self::BANNER_WIDTH
            };
            (base * self.banner_scale()).round()
        } else {
            self.idle_width()
        }
    }

    /// Collapsed target height: pops out to a multi-line toast height when a notification is active.
    pub fn collapsed_height(&self) -> f32 {
        if self.notification.is_some() {
            let base = if self.placement.is_vertical() {
                Self::VERT_BANNER_HEIGHT
            } else {
                Self::BANNER_HEIGHT
            };
            (base * self.banner_scale()).round()
        } else {
            self.idle_height()
        }
    }

    pub fn sync_collapsed_targets(&mut self) {
        if !self.is_hovered {
            self.target_width = self.collapsed_width();
            self.target_height = self.collapsed_height();
        }
    }

    pub fn idle_width(&self) -> f32 {
        if self.placement.is_vertical() {
            if self.minimized_island {
                self.size_preset.minimized_thickness()
            } else {
                (self.size_preset.idle_width_vertical() * self.type_scale.idle_ui_scale()).round()
            }
        } else if self.minimized_island {
            self.size_preset.minimized_length()
        } else {
            (self.size_preset.idle_width() * self.type_scale.idle_ui_scale()).round()
        }
    }

    pub fn idle_height(&self) -> f32 {
        if self.placement.is_vertical() {
            if self.minimized_island {
                self.size_preset.minimized_length()
            } else {
                (self.size_preset.idle_height_vertical() * self.type_scale.idle_ui_scale()).round()
            }
        } else if self.minimized_island {
            self.size_preset.minimized_thickness()
        } else {
            (self.size_preset.idle_height_horizontal() * self.type_scale.idle_ui_scale()).round()
        }
    }

    /// Unified rounded corner radius shared by Renderer, DirectComposition clip, SetWindowRgn, and WM_NCHITTEST.
    pub fn notch_corner_radius(&self) -> f32 {
        let w = self.current_width.max(1.0);
        let h = self.current_height.max(1.0);
        let exp_w = self.expanded_width();
        let exp_h = self.expanded_height();
        let settled_expanded =
            self.is_hovered && (w - exp_w).abs() < 0.5 && (h - exp_h).abs() < 0.5;
        let t = if settled_expanded {
            1.0
        } else {
            self.anim_expand_t.clamp(0.0, 1.0)
        };
        let idle_cap_r = if self.notification.is_some() {
            21.0 * self.banner_scale()
        } else if self.minimized_island {
            21.0
        } else {
            21.0 * self.type_scale.idle_ui_scale()
        };
        let idle_r = (w.min(h) * 0.5).min(idle_cap_r);
        let exp_r = match self.expanded_size {
            ExpandedSizePreset::Compact => 23.5,
            ExpandedSizePreset::Standard => 26.0,
            ExpandedSizePreset::Large => 28.5,
        } * self.type_scale.ui_scale();
        let r = idle_r + (exp_r - idle_r) * t;
        if self.is_frosted_active() {
            r.min(10.0)
        } else {
            r
        }
    }

    pub fn default_anchor_for_placement(_placement: NotchPlacement) -> (f32, f32) {
        (0.5, 0.5)
    }

    pub fn reset_anchor_for_placement(&mut self) {
        let (ax, ay) = Self::default_anchor_for_placement(self.placement);
        self.target_anchor_x = ax;
        self.target_anchor_y = ay;
    }

    pub fn is_top_left_corner_docked(&self) -> bool {
        match self.placement {
            NotchPlacement::Top => self.target_anchor_x <= 0.05,
            NotchPlacement::Left => self.target_anchor_y <= 0.05,
            NotchPlacement::Right => false,
        }
    }

    pub fn is_top_right_corner_docked(&self) -> bool {
        match self.placement {
            NotchPlacement::Top => self.target_anchor_x >= 0.95,
            NotchPlacement::Right => self.target_anchor_y <= 0.05,
            NotchPlacement::Left => false,
        }
    }

    pub fn is_corner_docked(&self) -> bool {
        self.is_top_left_corner_docked() || self.is_top_right_corner_docked()
    }

    pub fn corner_radii(&self) -> (f32, f32, f32, f32) {
        let r_cur = self.notch_corner_radius();
        let (base_tl, base_tr, base_br, base_bl) = match (self.notch_style, self.placement) {
            (NotchStyle::Floating, _) => (r_cur, r_cur, r_cur, r_cur),
            (NotchStyle::EdgeFlush, NotchPlacement::Top) => (4.0, 4.0, r_cur, r_cur),
            (NotchStyle::EdgeFlush, NotchPlacement::Left) => (4.0, r_cur, r_cur, 4.0),
            (NotchStyle::EdgeFlush, NotchPlacement::Right) => (r_cur, 4.0, 4.0, r_cur),
        };
        let (tl_t, tr_t) = match self.placement {
            NotchPlacement::Top => (
                1.0 - (self.anim_anchor_x / 0.18).clamp(0.0, 1.0),
                ((self.anim_anchor_x - 0.82) / 0.18).clamp(0.0, 1.0),
            ),
            NotchPlacement::Left => (1.0 - (self.anim_anchor_y / 0.18).clamp(0.0, 1.0), 0.0),
            NotchPlacement::Right => (0.0, 1.0 - (self.anim_anchor_y / 0.18).clamp(0.0, 1.0)),
        };
        if tl_t > 0.0 {
            (
                base_tl * (1.0 - tl_t),
                base_tr * (1.0 - tl_t),
                base_br,
                base_bl * (1.0 - tl_t),
            )
        } else if tr_t > 0.0 {
            (
                base_tl * (1.0 - tr_t),
                base_tr * (1.0 - tr_t),
                base_br * (1.0 - tr_t),
                base_bl,
            )
        } else {
            (base_tl, base_tr, base_br, base_bl)
        }
    }

    pub fn settings_cat_pill_y(cat: SettingsCategory) -> f32 {
        124.0 + (cat.index() as f32) * 52.0
    }

    /// True when the translucent "frosted" shell look is in use: the user has
    /// Frosted Glass on AND Windows "Transparency effects" is on. When Windows
    /// transparency is off the shell stays opaque, matching system behaviour.
    pub fn is_frosted_active(&self) -> bool {
        self.frosted_glass_enabled && self.windows_transparency_enabled
    }

    /// True when Windows Settings -> Accessibility -> Visual effects -> Animation
    /// effects is enabled. Motion springs and choreographed transitions run
    /// exclusively when this is true.
    pub fn are_animations_active(&self) -> bool {
        self.windows_animations_enabled
    }

    /// Live-updates frosted glass opacity during continuous slider drag without
    /// hitting disk until the drag completes. Automatically enables `frosted_glass_enabled`
    /// if the user drags the slider while the toggle was off.
    pub fn set_frosted_opacity_live(&mut self, pct: u32) -> bool {
        let clamped = pct.clamp(25, 95);
        let mut changed = false;
        if !self.frosted_glass_enabled {
            self.frosted_glass_enabled = true;
            self.switch_t[ToggleId::FrostedGlass as usize] = 1.0;
            changed = true;
        }
        if self.frosted_opacity_pct != clamped {
            self.frosted_opacity_pct = clamped;
            changed = true;
        }
        changed
    }

    /// Live-updates frosted blur radius during continuous slider drag without
    /// hitting disk until the drag completes. Automatically enables `frosted_glass_enabled`
    /// if the user drags the slider while the toggle was off.
    pub fn set_frosted_blur_live(&mut self, px: u32) -> bool {
        let clamped = px.clamp(4, 40);
        let mut changed = false;
        if !self.frosted_glass_enabled {
            self.frosted_glass_enabled = true;
            self.switch_t[ToggleId::FrostedGlass as usize] = 1.0;
            changed = true;
        }
        if self.frosted_blur_px != clamped {
            self.frosted_blur_px = clamped;
            changed = true;
        }
        changed
    }

    /// Tint opacity actually used for the frosted shell. With a live blur
    /// backdrop the user's value applies; without one (blur unavailable or
    /// disabled) the glass is made denser so text stays readable.
    pub fn frosted_alpha_pct(&self) -> u32 {
        if crate::backdrop::blur_available() {
            self.frosted_opacity_pct
        } else {
            self.frosted_opacity_pct
                .max(crate::backdrop::GLASS_FALLBACK_OPACITY_PCT)
        }
    }

    /// True when an active banner should punch through fullscreen / borderless apps (games, video).
    pub fn notification_overlays_fullscreen(&self) -> bool {
        self.notif_over_fullscreen && self.notifications_enabled && self.notification.is_some()
    }

    pub fn settings_scroll(&self, cat: SettingsCategory) -> f32 {
        self.settings_scroll[cat.index()]
    }

    pub fn settings_scroll_target(&self, cat: SettingsCategory) -> f32 {
        self.settings_scroll_target[cat.index()]
    }

    pub fn set_settings_scroll(&mut self, cat: SettingsCategory, scroll: f32) {
        let v = scroll.max(0.0);
        let idx = cat.index();
        self.settings_scroll_target[idx] = v;
        if !self.are_animations_active() {
            self.settings_scroll[idx] = v;
        }
    }

    pub fn set_settings_scroll_immediate(&mut self, cat: SettingsCategory, scroll: f32) {
        let v = scroll.max(0.0);
        let idx = cat.index();
        self.settings_scroll[idx] = v;
        self.settings_scroll_target[idx] = v;
    }

    pub fn dock_metrics(tab_count: usize) -> (f32, f32) {
        Self::dock_metrics_for_placement(tab_count, NotchPlacement::Top)
    }

    pub fn dock_metrics_for_placement(tab_count: usize, placement: NotchPlacement) -> (f32, f32) {
        let count = tab_count.max(1) as f32;
        if placement.is_vertical() {
            let dock_w = (8.0 + count * 30.6).min(284.0);
            let item_w = (dock_w - 8.0) / count;
            (dock_w, item_w)
        } else {
            let dock_w = (8.0 + count * 32.0).min(302.0);
            let item_w = (dock_w - 8.0) / count;
            (dock_w, item_w)
        }
    }

    pub fn visible_dock_modes(&self) -> Vec<NotchMode> {
        let mut modes = Vec::with_capacity(9);
        modes.push(NotchMode::Home);
        if self.modules.music {
            modes.push(NotchMode::Music);
        }
        if self.modules.volume {
            modes.push(NotchMode::Volume);
        }
        if self.modules.mem_reduct {
            modes.push(NotchMode::MemReduct);
        }
        if self.modules.tasks {
            modes.push(NotchMode::Tasks);
        }
        if self.modules.shelf {
            modes.push(NotchMode::Shelf);
        }
        if self.modules.capture {
            modes.push(NotchMode::Capture);
        }
        if self.modules.telemetry {
            modes.push(NotchMode::Telemetry);
        }
        modes
    }

    pub fn active_dock_target(&self) -> (f32, f32) {
        let visible = self.visible_dock_modes();
        let (dock_w, item_w) = Self::dock_metrics_for_placement(visible.len(), self.placement);
        let idx = visible.iter().position(|&m| m == self.mode).unwrap_or(0);
        let dock_x = if self.placement.is_vertical() {
            (Self::VERT_EXPANDED_WIDTH - dock_w) * 0.5
        } else {
            20.0
        };
        (dock_x + 4.0 + (idx as f32) * item_w, item_w)
    }

    pub fn desired_active_interval_ms(&self) -> u32 {
        match self.fps_cap {
            120 => 8,
            90 => 11,
            60 => 16,
            _ => {
                let hz = self.detected_hz.max(60);
                if hz >= 115 {
                    8
                } else if hz >= 85 {
                    11
                } else {
                    (1000 / hz).clamp(8, 16)
                }
            }
        }
    }

    pub fn toggle_target(&self, id: ToggleId) -> bool {
        match id {
            ToggleId::Autostart => self.autostart,
            ToggleId::Chime => self.chime_enabled,
            ToggleId::ClickToOpen => self.click_to_open,
            ToggleId::HideFullscreen => self.hide_in_fullscreen,
            ToggleId::ShowMascot => self.show_mascot,
            ToggleId::ProgressArc => self.progress_arc_enabled,
            ToggleId::MicroInteractions => self.micro_interactions,
            ToggleId::Equalizer => self.equalizer_enabled,
            ToggleId::Notifications => self.notifications_enabled,
            ToggleId::LyricsMonitor => self.lyrics_enabled,
            ToggleId::ClipboardWatch => self.clipboard_watch_enabled,
            ToggleId::CaptureClipboard => self.capture_to_clipboard,
            ToggleId::AutoClean => self.mem_reduct.auto_clean_enabled,
            ToggleId::AreaWs => self.mem_reduct.clean_working_sets,
            ToggleId::AreaSys => self.mem_reduct.clean_system_ws,
            ToggleId::AreaStandby => self.mem_reduct.clean_standby_list,
            ToggleId::ScreenTime => self.screentime_enabled,
            ToggleId::ModMusic => self.modules.music,
            ToggleId::ModVolume => self.modules.volume,
            ToggleId::ModMemReduct => self.modules.mem_reduct,
            ToggleId::ModTasks => self.modules.tasks,
            ToggleId::ModShelf => self.modules.shelf,
            ToggleId::ModCapture => self.modules.capture,
            ToggleId::ModTelemetry => self.modules.telemetry,
            ToggleId::MinimizedIsland => self.minimized_island,
            ToggleId::AreaModPage => self.mem_reduct.clean_modified_page_list,
            ToggleId::AreaStandbyLow => self.mem_reduct.clean_standby_low,
            ToggleId::AreaModFile => self.mem_reduct.clean_modified_file_cache,
            ToggleId::AreaRegistry => self.mem_reduct.clean_registry_cache,
            ToggleId::AreaCombine => self.mem_reduct.clean_combine_memory_lists,
            ToggleId::ShowCleanResults => self.mem_reduct.show_clean_results,
            ToggleId::FrostedGlass => self.frosted_glass_enabled,
            ToggleId::NotifListener => self.notif_listener_enabled,
            ToggleId::NotifExclusive => self.notif_exclusive_mode,
            ToggleId::NotifAutoClear => self.notif_auto_clear_action_center,
            ToggleId::NotifOverFullscreen => self.notif_over_fullscreen,
            ToggleId::RemoveWindowsBorders => self.remove_windows_borders,
        }
    }

    pub fn sync_switches_immediate(&mut self) {
        for id in ToggleId::ALL {
            self.switch_t[id as usize] = if self.toggle_target(id) { 1.0 } else { 0.0 };
        }
    }

    pub fn apply_setting_action(&mut self, action: Action) -> Option<SettingEffects> {
        let mut eff = SettingEffects {
            redraw_notch: true,
            ..Default::default()
        };
        match action {
            Action::SetSettingsCategory(cat) => {
                self.set_settings_category(cat);
                eff.redraw_notch = false;
            }
            Action::CloseSettings => {
                eff.close_settings = true;
            }
            Action::TogglePin => {
                self.is_pinned = !self.is_pinned;
                self.set_hovered(self.is_pinned);
                self.save_config();
            }
            Action::SetNotchStyle(style) => {
                self.notch_style = style;
                self.save_config();
            }
            Action::SetPlacement(pl) => {
                if self.placement != pl {
                    let axis_changed = self.placement.is_vertical() != pl.is_vertical();
                    self.placement = pl;
                    self.x_offset = 0;
                    self.y_offset = 0;
                    self.reset_anchor_for_placement();
                    if self.is_hovered {
                        self.target_width = self.expanded_width();
                        self.target_height = self.expanded_height();
                    } else {
                        self.sync_collapsed_targets();
                    }
                    if axis_changed {
                        self.current_width = self.target_width;
                        self.current_height = self.target_height;
                        self.anim_anchor_x = self.target_anchor_x;
                        self.anim_anchor_y = self.target_anchor_y;
                    }
                    let (dx, dw) = self.active_dock_target();
                    self.anim_dock_x = dx;
                    self.anim_dock_w = dw;
                    self.save_config();
                }
            }
            Action::SetSizePreset(preset) => {
                self.size_preset = preset;
                self.sync_collapsed_targets();
                self.save_config();
            }
            Action::SetExpandedSize(preset) => {
                self.expanded_size = preset;
                if self.is_hovered {
                    self.target_width = self.expanded_width();
                    self.target_height = self.expanded_height();
                }
                self.save_config();
            }
            Action::SetTypeScale(scale) => {
                self.type_scale = scale;
                if self.is_hovered {
                    self.target_width = self.expanded_width();
                    self.target_height = self.expanded_height();
                } else {
                    self.sync_collapsed_targets();
                }
                self.save_config();
            }
            Action::SetIdleMetric(metric) => {
                self.idle_metric = metric;
                self.save_config();
            }
            Action::ToggleMinimizedIsland => {
                self.minimized_island = !self.minimized_island;
                self.sync_collapsed_targets();
                self.save_config();
            }
            Action::ToggleClickToOpen => {
                self.click_to_open = !self.click_to_open;
                self.save_config();
            }
            Action::ToggleHideFullscreen => {
                self.hide_in_fullscreen = !self.hide_in_fullscreen;
                self.save_config();
            }
            Action::ToggleShowMascot => {
                self.show_mascot = !self.show_mascot;
                self.save_config();
            }
            Action::ToggleProgressArc => {
                self.progress_arc_enabled = !self.progress_arc_enabled;
                self.save_config();
            }
            Action::SetFpsCap(cap) => {
                self.fps_cap = cap;
                self.save_config();
            }
            Action::ToggleMicroInteractions => {
                self.micro_interactions = !self.micro_interactions;
                self.save_config();
            }
            Action::ToggleEqualizer => {
                self.equalizer_enabled = !self.equalizer_enabled;
                self.save_config();
            }
            Action::ToggleNotifications => {
                self.notifications_enabled = !self.notifications_enabled;
                if !self.notifications_enabled {
                    self.notif_queue.clear();
                    self.notification = None;
                    self.sync_collapsed_targets();
                }
                eff.sync_win_notif_sound_mute = Some(self.should_mute_windows_notif_sound());
                self.save_config();
            }
            Action::SetNotificationSecs(secs) => {
                self.notification_secs = secs.clamp(2, 10);
                self.save_config();
            }
            Action::SetHoverDwellSpeed(spd) => {
                self.hover_dwell_speed = spd;
                self.save_config();
                eff.redraw_notch = false;
            }
            Action::SetNotificationSound(sound) => {
                self.notification_sound = sound;
                self.save_config();
                if sound != crate::sound::NotificationSound::Mute {
                    eff.play_sound = Some(sound);
                }
                eff.sync_win_notif_sound_mute = Some(self.should_mute_windows_notif_sound());
                eff.redraw_notch = false;
            }
            Action::TestNotificationSound => {
                eff.play_sound = Some(crate::sound::NotificationSound::Snapchat);
                eff.redraw_notch = false;
            }
            Action::ToggleFrostedGlass => {
                self.frosted_glass_enabled = !self.frosted_glass_enabled;
                self.save_config();
                eff.sync_frosted_window = true;
            }
            Action::ToggleRemoveWindowsBorders => {
                self.remove_windows_borders = !self.remove_windows_borders;
                self.save_config();
                eff.sync_windows_borders = Some(self.remove_windows_borders);
                eff.redraw_notch = false;
            }
            Action::SetFrostedOpacity(pct) => {
                self.frosted_glass_enabled = true;
                self.frosted_opacity_pct = pct.clamp(25, 95);
                self.save_config();
                eff.sync_frosted_window = true;
            }
            Action::SetFrostedBlur(px) => {
                self.frosted_glass_enabled = true;
                self.frosted_blur_px = px.clamp(4, 40);
                self.save_config();
                eff.sync_frosted_window = true;
            }
            Action::ToggleNotifListener => {
                self.notif_listener_enabled = !self.notif_listener_enabled;
                if self.notif_listener_enabled {
                    self.notif_listener_status = "Connecting...".to_string();
                    eff.request_notif_access = true;
                } else {
                    self.notif_listener_status = "Off".to_string();
                    if self.notif_exclusive_mode {
                        self.notif_exclusive_mode = false;
                        eff.sync_exclusive_banners = Some(false);
                    }
                }
                eff.sync_win_notif_sound_mute = Some(self.should_mute_windows_notif_sound());
                self.save_config();
            }
            Action::ToggleNotifExclusive => {
                self.notif_exclusive_mode = !self.notif_exclusive_mode;
                if self.notif_exclusive_mode {
                    if !self.notif_listener_enabled {
                        self.notif_listener_enabled = true;
                        self.notif_listener_status = "Connecting...".to_string();
                        eff.request_notif_access = true;
                    }
                    eff.sync_exclusive_banners = Some(true);
                } else {
                    eff.sync_exclusive_banners = Some(false);
                }
                eff.sync_win_notif_sound_mute = Some(self.should_mute_windows_notif_sound());
                self.save_config();
            }
            Action::ToggleNotifAutoClear => {
                self.notif_auto_clear_action_center = !self.notif_auto_clear_action_center;
                self.save_config();
            }
            Action::ToggleNotifOverFullscreen => {
                self.notif_over_fullscreen = !self.notif_over_fullscreen;
                self.save_config();
            }
            Action::OpenWinNotifSettings => {
                eff.open_win_notif_settings = true;
                eff.redraw_notch = false;
            }
            Action::OpenWinPersonalizeColors => {
                eff.open_win_personalize_colors = true;
                eff.redraw_notch = false;
            }
            Action::SendTestNotification => {
                let now = Instant::now();
                let secs = self.notification_secs.clamp(2, 10).max(4);
                self.notification = Some(NotificationBanner {
                    app_name: "Discord".to_string(),
                    aumid: None,
                    title: "Kai (#dev-sync)".to_string(),
                    subtitle:
                        "Notification bridge verified — messages from apps will pop here cleanly"
                            .to_string(),
                    kind: NotificationKind::SystemToast,
                    is_success: true,
                    total_secs: secs as f32,
                    expires_at: now + std::time::Duration::from_secs(secs as u64),
                    grouped_count: 1,
                    received_at: now,
                });
                self.record_notif_history(
                    "Discord".to_string(),
                    None,
                    "Kai (#dev-sync)".to_string(),
                    "Notification bridge verified — messages from apps will pop here cleanly"
                        .to_string(),
                );
                self.sync_collapsed_targets();
                eff.redraw_notch = true;
                if self.notifications_enabled && self.notification_sound != crate::sound::NotificationSound::Mute {
                    eff.play_sound = Some(self.notification_sound);
                }
            }
            Action::DismissNotification => {
                self.advance_notification_queue();
                eff.redraw_notch = true;
            }
            Action::ClearNotificationQueue => {
                self.notif_queue.clear();
                self.notification = None;
                self.sync_collapsed_targets();
                eff.redraw_notch = true;
            }
            Action::ToggleNotifHistory => {
                self.show_notif_history = !self.show_notif_history;
                self.hover_target_idx = None;
                self.hover_slots = [0.0; 48];
                eff.redraw_notch = true;
            }
            Action::DismissHistoryItem(idx) => {
                if idx < self.notif_history.len() {
                    self.notif_history.remove(idx);
                    self.hover_target_idx = None;
                    eff.redraw_notch = true;
                }
            }
            Action::ClearNotifHistory => {
                self.notif_history.clear();
                self.hover_target_idx = None;
                eff.redraw_notch = true;
            }
            Action::CycleLyricSize => {
                self.lyric_size = self.lyric_size.next();
                self.save_config();
            }
            Action::CycleLyricStyle => {
                self.lyric_style = self.lyric_style.next();
                self.save_config();
            }
            Action::SetLyricSize(sz) => {
                self.lyric_size = sz;
                self.save_config();
            }
            Action::SetLyricStyle(st) => {
                self.lyric_style = st;
                self.save_config();
            }
            Action::ToggleModule(m) => {
                match m {
                    NotchMode::Music => self.modules.music = !self.modules.music,
                    NotchMode::Volume => self.modules.volume = !self.modules.volume,
                    NotchMode::MemReduct => self.modules.mem_reduct = !self.modules.mem_reduct,
                    NotchMode::Tasks => self.modules.tasks = !self.modules.tasks,
                    NotchMode::Shelf => self.modules.shelf = !self.modules.shelf,
                    NotchMode::Capture => self.modules.capture = !self.modules.capture,
                    NotchMode::Telemetry => self.modules.telemetry = !self.modules.telemetry,
                    _ => {}
                }
                if !self.visible_dock_modes().contains(&self.mode) {
                    self.set_mode(NotchMode::Home);
                }
                self.save_config();
            }
            Action::ToggleLyricsMonitor => {
                self.lyrics_enabled = !self.lyrics_enabled;
                self.save_config();
            }
            Action::ToggleClipboardWatch => {
                self.clipboard_watch_enabled = !self.clipboard_watch_enabled;
                self.save_config();
            }
            Action::ToggleCaptureClipboard => {
                self.capture_to_clipboard = !self.capture_to_clipboard;
                self.save_config();
            }
            Action::ToggleAutoClean => {
                self.mem_reduct.auto_clean_enabled = !self.mem_reduct.auto_clean_enabled;
                self.save_config();
            }
            Action::ToggleArea(idx) => {
                self.toggle_mem_area(idx);
            }
            Action::CycleThreshold => {
                self.cycle_threshold();
            }
            Action::CycleInterval => {
                self.cycle_interval();
            }
            Action::SetCleanThreshold(pct) => {
                self.mem_reduct.when_above_pct = if pct == 0 { 0 } else { pct.clamp(50, 98) };
                self.save_config();
            }
            Action::SetCleanInterval(mins) => {
                self.mem_reduct.every_mins = if mins == 0 { 0 } else { mins.clamp(1, 180) };
                self.save_config();
            }
            Action::ToggleShowCleanResults => {
                self.mem_reduct.show_clean_results = !self.mem_reduct.show_clean_results;
                self.save_config();
            }
            Action::PurgeRam => {
                eff.trigger_purge = true;
            }
            Action::ToggleAutostart => {
                self.autostart = !self.autostart;
                eff.update_autostart = Some(self.autostart);
            }
            Action::ToggleChime => {
                self.chime_enabled = !self.chime_enabled;
                self.save_config();
            }
            Action::ToggleScreenTime => {
                self.screentime_enabled = !self.screentime_enabled;
                self.save_config();
            }
            Action::ResetScreenTimeToday => {
                self.screen_time = ScreenTimeStats::default();
                eff.reset_screentime = true;
            }
            Action::ResetPosition => {
                self.x_offset = 0;
                self.y_offset = 0;
                self.reset_anchor_for_placement();
                self.save_config();
            }
            Action::ResetAllSettings => {
                let prev_frosted = self.is_frosted_active();
                let was_exclusive = self.notif_exclusive_mode;
                let was_listening = self.notif_listener_enabled;
                self.reset_all_settings();
                if !was_listening && self.notif_listener_enabled {
                    eff.request_notif_access = true;
                }
                if self.is_frosted_active() != prev_frosted {
                    eff.sync_frosted_window = true;
                }
                if was_exclusive {
                    eff.sync_exclusive_banners = Some(false);
                }
                eff.sync_win_notif_sound_mute = Some(self.should_mute_windows_notif_sound());
            }
            Action::Quit => {
                eff.quit_app = true;
            }
            _ => return None,
        }
        Some(eff)
    }

    fn appdata_dir() -> std::path::PathBuf {
        crate::models::appdata_dir()
    }

    fn config_path() -> std::path::PathBuf {
        Self::appdata_dir().join("config.json")
    }
    fn tasks_path() -> std::path::PathBuf {
        Self::appdata_dir().join("tasks.json")
    }
    fn shelf_path() -> std::path::PathBuf {
        Self::appdata_dir().join("shelf.json")
    }

    pub fn to_config_json(&self) -> String {
        let cfg = PersistedConfig {
            chime_enabled: self.chime_enabled,
            clean_working_sets: self.mem_reduct.clean_working_sets,
            clean_system_ws: self.mem_reduct.clean_system_ws,
            clean_standby_list: self.mem_reduct.clean_standby_list,
            clean_modified_page_list: self.mem_reduct.clean_modified_page_list,
            clean_standby_low: self.mem_reduct.clean_standby_low,
            clean_modified_file_cache: self.mem_reduct.clean_modified_file_cache,
            clean_registry_cache: self.mem_reduct.clean_registry_cache,
            clean_combine_memory_lists: self.mem_reduct.clean_combine_memory_lists,
            show_clean_results: self.mem_reduct.show_clean_results,
            when_above_pct: self.mem_reduct.when_above_pct,
            every_mins: self.mem_reduct.every_mins,
            auto_clean_enabled: self.mem_reduct.auto_clean_enabled,
            is_pinned: self.is_pinned,
            x_offset: self.x_offset,
            y_offset: self.y_offset,
            last_freed_mb: self.mem_reduct.last_freed_mb,
            last_cleaned_status: self.mem_reduct.last_cleaned_status.clone(),
            notch_style: self.notch_style,
            placement: self.placement,
            size_preset: self.size_preset,
            expanded_size: self.expanded_size,
            type_scale: self.type_scale,
            idle_metric: self.idle_metric,
            minimized_island: self.minimized_island,
            click_to_open: self.click_to_open,
            hide_in_fullscreen: self.hide_in_fullscreen,
            show_mascot: self.show_mascot,
            progress_arc_enabled: self.progress_arc_enabled,
            fps_cap: self.fps_cap,
            micro_interactions: self.micro_interactions,
            equalizer_enabled: self.equalizer_enabled,
            notifications_enabled: self.notifications_enabled,
            notification_secs: self.notification_secs,
            notification_sound: self.notification_sound,
            notification_volume: self.notification_volume,
            hover_dwell_speed: self.hover_dwell_speed,
            seen_hotkey_hint: self.seen_hotkey_hint,
            modules: self.modules,
            lyrics_enabled: self.lyrics_enabled,
            clipboard_watch_enabled: self.clipboard_watch_enabled,
            capture_to_clipboard: self.capture_to_clipboard,
            screentime_enabled: self.screentime_enabled,
            lyric_size: self.lyric_size,
            lyric_style: self.lyric_style,
            frosted_glass_enabled: self.frosted_glass_enabled,
            frosted_opacity_pct: self.frosted_opacity_pct,
            frosted_blur_px: self.frosted_blur_px,
            remove_windows_borders: self.remove_windows_borders,
            anchor_x: Some(self.target_anchor_x),
            anchor_y: Some(self.target_anchor_y),
            notif_listener_enabled: self.notif_listener_enabled,
            notif_exclusive_mode: self.notif_exclusive_mode,
            notif_auto_clear_action_center: self.notif_auto_clear_action_center,
            notif_over_fullscreen: self.notif_over_fullscreen,
            config_rev: CONFIG_REV,
        };
        serde_json::to_string_pretty(&cfg).unwrap_or_default()
    }

    pub fn save_config(&self) {
        if !self.persistence_enabled {
            return;
        }
        let json = self.to_config_json();
        if json.is_empty() {
            return;
        }
        let _ = crate::persistence::atomic_write_json_str(&Self::config_path(), &json, true);
    }

    fn load_config() -> Option<PersistedConfig> {
        crate::persistence::load_json_with_recovery::<PersistedConfig, _>(
            &Self::config_path(),
            |_| true,
        )
        .into_option()
    }

    pub fn apply_config_json(&mut self, json: &str) -> bool {
        let Ok(cfg) = serde_json::from_str::<PersistedConfig>(json) else {
            return false;
        };
        let listener_eff = cfg.listener_effective();
        self.chime_enabled = cfg.chime_enabled;
        self.mem_reduct.clean_working_sets = cfg.clean_working_sets;
        self.mem_reduct.clean_system_ws = cfg.clean_system_ws;
        self.mem_reduct.clean_standby_list = cfg.clean_standby_list;
        self.mem_reduct.clean_modified_page_list = cfg.clean_modified_page_list;
        self.mem_reduct.clean_standby_low = cfg.clean_standby_low;
        self.mem_reduct.clean_modified_file_cache = cfg.clean_modified_file_cache;
        self.mem_reduct.clean_registry_cache = cfg.clean_registry_cache;
        self.mem_reduct.clean_combine_memory_lists = cfg.clean_combine_memory_lists;
        self.mem_reduct.show_clean_results = cfg.show_clean_results;
        self.mem_reduct.when_above_pct = cfg.when_above_pct;
        self.mem_reduct.every_mins = cfg.every_mins;
        self.mem_reduct.auto_clean_enabled = cfg.auto_clean_enabled;
        self.is_pinned = cfg.is_pinned;
        self.is_hovered = cfg.is_pinned;
        self.x_offset = cfg.x_offset;
        self.y_offset = cfg.y_offset;
        if cfg.last_freed_mb > 0.0 || !cfg.last_cleaned_status.is_empty() {
            self.mem_reduct.last_freed_mb = cfg.last_freed_mb;
            if !cfg.last_cleaned_status.is_empty()
                && !cfg.last_cleaned_status.starts_with("Cleaning")
            {
                self.mem_reduct.last_cleaned_status = cfg.last_cleaned_status;
            }
        }
        self.notch_style = cfg.notch_style;
        self.placement = cfg.placement;
        self.size_preset = cfg.size_preset;
        self.expanded_size = cfg.expanded_size;
        self.type_scale = cfg.type_scale;
        self.idle_metric = cfg.idle_metric;
        self.minimized_island = cfg.minimized_island;
        self.click_to_open = cfg.click_to_open;
        self.hide_in_fullscreen = cfg.hide_in_fullscreen;
        self.show_mascot = cfg.show_mascot;
        self.progress_arc_enabled = cfg.progress_arc_enabled;
        self.fps_cap = cfg.fps_cap;
        self.micro_interactions = cfg.micro_interactions;
        self.equalizer_enabled = cfg.equalizer_enabled;
        self.notifications_enabled = cfg.notifications_enabled;
        self.notification_secs = cfg.notification_secs.clamp(2, 10);
        self.notification_sound = cfg.notification_sound;
        self.notification_volume = cfg.notification_volume.clamp(0, 100);
        self.hover_dwell_speed = cfg.hover_dwell_speed;
        self.seen_hotkey_hint = cfg.seen_hotkey_hint;
        self.modules = cfg.modules;
        self.lyrics_enabled = cfg.lyrics_enabled;
        self.clipboard_watch_enabled = cfg.clipboard_watch_enabled;
        self.capture_to_clipboard = cfg.capture_to_clipboard;
        self.screentime_enabled = cfg.screentime_enabled;
        self.lyric_size = cfg.lyric_size;
        self.lyric_style = cfg.lyric_style;
        self.frosted_glass_enabled = cfg.frosted_glass_enabled;
        self.frosted_opacity_pct = cfg.frosted_opacity_pct.clamp(25, 95);
        self.frosted_blur_px = cfg.frosted_blur_px.clamp(4, 40);
        self.remove_windows_borders = cfg.remove_windows_borders;
        let (def_ax, def_ay) = Self::default_anchor_for_placement(self.placement);
        self.target_anchor_x = cfg.anchor_x.unwrap_or(def_ax).clamp(0.0, 1.0);
        self.target_anchor_y = cfg.anchor_y.unwrap_or(def_ay).clamp(0.0, 1.0);
        self.notif_listener_enabled = listener_eff;
        self.notif_exclusive_mode = cfg.notif_exclusive_mode;
        self.notif_auto_clear_action_center = cfg.notif_auto_clear_action_center;
        self.notif_over_fullscreen = cfg.notif_over_fullscreen;
        if self.is_hovered {
            self.target_width = self.expanded_width();
            self.target_height = self.expanded_height();
        } else {
            self.sync_collapsed_targets();
        }
        self.sync_preview_settled();
        true
    }

    pub fn save_tasks(&self) {
        if !self.persistence_enabled {
            return;
        }
        let _ = crate::persistence::atomic_write_json(&Self::tasks_path(), &self.tasks, true);
        // Mirror TSV backup for interoperability with Nusa if present
        let mut lines = Vec::new();
        for t in &self.tasks {
            let flag = if t.completed { "1" } else { "0" };
            lines.push(format!("{}\t{}", flag, t.text));
        }
        let content = lines.join("\n");
        let nusa_path = "C:\\Users\\muhna\\AppData\\Local\\Packages\\citoxstd.Nusa_r9cmnhct3ptgm\\LocalCache\\Roaming\\Nusa\\tasks.txt";
        if std::path::Path::new(nusa_path).exists() {
            let _ = crate::persistence::atomic_write_bytes(
                std::path::Path::new(nusa_path),
                content.as_bytes(),
                false,
            );
        }
    }

    fn parse_tsv_tasks(content: &str) -> Vec<TaskItem> {
        let mut list = Vec::new();
        for line in content.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if let Some(text) = parts.last() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let completed = parts.first().map(|s| s.trim() == "1").unwrap_or(false);
                    list.push(TaskItem {
                        text: trimmed.to_string(),
                        completed,
                    });
                }
            }
        }
        list
    }

    fn load_initial_tasks() -> Vec<TaskItem> {
        if let Some(items) = crate::persistence::load_json_with_recovery::<Vec<TaskItem>, _>(
            &Self::tasks_path(),
            |_| true,
        )
        .into_option()
        {
            return items;
        }
        let nusa_path = "C:\\Users\\muhna\\AppData\\Local\\Packages\\citoxstd.Nusa_r9cmnhct3ptgm\\LocalCache\\Roaming\\Nusa\\tasks.txt";
        if let Ok(content) = std::fs::read_to_string(nusa_path) {
            let parsed = Self::parse_tsv_tasks(&content);
            if !parsed.is_empty() {
                return parsed;
            }
        }
        Vec::new()
    }

    pub fn save_shelf(&self) {
        if !self.persistence_enabled {
            return;
        }
        let _ = crate::persistence::atomic_write_json(&Self::shelf_path(), &self.shelf_items, true);
    }

    fn load_initial_shelf() -> Vec<ShelfItem> {
        crate::persistence::load_json_with_recovery::<Vec<ShelfItem>, _>(
            &Self::shelf_path(),
            |_| true,
        )
        .into_option()
        .unwrap_or_default()
    }

    pub fn new() -> Self {
        let mut mem_reduct = MemReductConfig::default();
        let mut chime_enabled = true;
        let mut is_pinned = false;
        let mut x_offset = 0;
        let mut y_offset = 0;
        let mut notch_style = NotchStyle::Floating;
        let mut placement = NotchPlacement::Top;
        let mut size_preset = NotchSizePreset::Standard;
        let mut expanded_size = ExpandedSizePreset::Standard;
        let mut type_scale = TypeScale::Standard;
        let mut idle_metric = IdleMetric::Ram;
        let mut minimized_island = false;
        let mut click_to_open = false;
        let mut hide_in_fullscreen = true;
        let mut show_mascot = true;
        let mut progress_arc_enabled = true;
        let mut fps_cap = 0;
        let mut micro_interactions = true;
        let mut equalizer_enabled = true;
        let mut notifications_enabled = true;
        let mut notification_secs = 4;
        let mut notification_sound = crate::sound::NotificationSound::default();
        let mut notification_volume = 80;
                let mut hover_dwell_speed = HoverDwellSpeed::Normal;
                let mut seen_hotkey_hint = false;
        let mut modules = ModuleVisibility::default();
        let mut lyrics_enabled = true;
        let mut clipboard_watch_enabled = true;
        let mut capture_to_clipboard = true;
        let mut screentime_enabled = true;
        let mut lyric_size = LyricSize::Standard;
        let mut lyric_style = LyricStyle::ClassicList;
        let mut frosted_glass_enabled = true;
        let mut frosted_opacity_pct = default_frosted_opacity();
        let mut frosted_blur_px = default_frosted_blur();
        let mut remove_windows_borders = false;
        let mut loaded_anchor_x: Option<f32> = None;
        let mut loaded_anchor_y: Option<f32> = None;
        let mut notif_listener_enabled = true;
        let mut notif_exclusive_mode = false;
        let mut notif_auto_clear_action_center = false;
        let mut notif_over_fullscreen = false;
        let mut needs_migration_save = false;

        if let Some(cfg) = Self::load_config() {
            let listener_eff = cfg.listener_effective();
            chime_enabled = cfg.chime_enabled;
            mem_reduct.clean_working_sets = cfg.clean_working_sets;
            mem_reduct.clean_system_ws = cfg.clean_system_ws;
            mem_reduct.clean_standby_list = cfg.clean_standby_list;
            mem_reduct.clean_modified_page_list = cfg.clean_modified_page_list;
            mem_reduct.clean_standby_low = cfg.clean_standby_low;
            mem_reduct.clean_modified_file_cache = cfg.clean_modified_file_cache;
            mem_reduct.clean_registry_cache = cfg.clean_registry_cache;
            mem_reduct.clean_combine_memory_lists = cfg.clean_combine_memory_lists;
            mem_reduct.show_clean_results = cfg.show_clean_results;
            mem_reduct.when_above_pct = cfg.when_above_pct;
            mem_reduct.every_mins = cfg.every_mins;
            mem_reduct.auto_clean_enabled = cfg.auto_clean_enabled;
            is_pinned = cfg.is_pinned;
            x_offset = cfg.x_offset;
            y_offset = cfg.y_offset;
            if cfg.last_freed_mb > 0.0 || !cfg.last_cleaned_status.is_empty() {
                mem_reduct.last_freed_mb = cfg.last_freed_mb;
                if !cfg.last_cleaned_status.is_empty()
                    && !cfg.last_cleaned_status.starts_with("Cleaning")
                {
                    mem_reduct.last_cleaned_status = cfg.last_cleaned_status;
                }
            }
            notch_style = cfg.notch_style;
            placement = cfg.placement;
            size_preset = cfg.size_preset;
            expanded_size = cfg.expanded_size;
            type_scale = cfg.type_scale;
            idle_metric = cfg.idle_metric;
            minimized_island = cfg.minimized_island;
            click_to_open = cfg.click_to_open;
            hide_in_fullscreen = cfg.hide_in_fullscreen;
            show_mascot = cfg.show_mascot;
            progress_arc_enabled = cfg.progress_arc_enabled;
            fps_cap = cfg.fps_cap;
            micro_interactions = cfg.micro_interactions;
            equalizer_enabled = cfg.equalizer_enabled;
            notifications_enabled = cfg.notifications_enabled;
            notification_secs = cfg.notification_secs.clamp(2, 10);
            notification_sound = cfg.notification_sound;
            notification_volume = cfg.notification_volume.clamp(0, 100);
                        hover_dwell_speed = cfg.hover_dwell_speed;
                        seen_hotkey_hint = cfg.seen_hotkey_hint;
            modules = cfg.modules;
            lyrics_enabled = cfg.lyrics_enabled;
            clipboard_watch_enabled = cfg.clipboard_watch_enabled;
            capture_to_clipboard = cfg.capture_to_clipboard;
            screentime_enabled = cfg.screentime_enabled;
            lyric_size = cfg.lyric_size;
            lyric_style = cfg.lyric_style;
            frosted_glass_enabled = cfg.frosted_glass_enabled;
            frosted_opacity_pct = cfg.frosted_opacity_pct.clamp(25, 95);
            frosted_blur_px = cfg.frosted_blur_px.clamp(4, 40);
            remove_windows_borders = cfg.remove_windows_borders;
            loaded_anchor_x = cfg.anchor_x;
            loaded_anchor_y = cfg.anchor_y;
            notif_listener_enabled = listener_eff;
            notif_exclusive_mode = cfg.notif_exclusive_mode;
            notif_auto_clear_action_center = cfg.notif_auto_clear_action_center;
            notif_over_fullscreen = cfg.notif_over_fullscreen;
            needs_migration_save = cfg.config_rev < CONFIG_REV;
        }

        let windows_transparency_enabled = crate::notifications::is_windows_transparency_enabled();
        let windows_animations_enabled = crate::notifications::is_windows_animations_enabled();
        let notif_suppressed_apps = crate::notifications::count_suppressed_apps();
        let notif_listener_status = if notif_listener_enabled {
            "Connecting...".to_string()
        } else {
            "Off".to_string()
        };

        let vol = crate::audio::get_master_volume().unwrap_or(0.50);
        let pre_mute = if vol > 0.05 { vol } else { 0.50 };
        let initial_clip =
            crate::capture::read_clipboard_text(windows::Win32::Foundation::HWND::default());
        let quick_note = if let Some(ref clip_text) = initial_clip {
            format!(
                "{} words · Clipboard",
                clip_text.split_whitespace().count()
            )
        } else {
            "Clipboard empty".to_string()
        };

        let telemetry = HardwareTelemetry::initial_real();
        let ram_pct = (telemetry.memory_percent / 100.0).clamp(0.0, 1.0);
        let idle_s = type_scale.idle_ui_scale();
        let idle_w = if placement.is_vertical() {
            if minimized_island {
                Self::MINIMIZED_VERT_WIDTH
            } else {
                (Self::VERT_IDLE_WIDTH * idle_s).round()
            }
        } else if minimized_island {
            Self::MINIMIZED_WIDTH
        } else {
            (size_preset.idle_width() * idle_s).round()
        };
        let idle_h = if placement.is_vertical() {
            if minimized_island {
                Self::MINIMIZED_VERT_HEIGHT
            } else {
                (size_preset.idle_height_vertical() * idle_s).round()
            }
        } else if minimized_island {
            Self::MINIMIZED_HEIGHT
        } else {
            (Self::IDLE_HEIGHT * idle_s).round()
        };
        let exp_s = expanded_size.scale() * type_scale.ui_scale();
        let exp_w = (if placement.is_vertical() {
            Self::VERT_EXPANDED_WIDTH
        } else {
            Self::EXPANDED_WIDTH
        } * exp_s)
            .round();
        let exp_h = (if placement.is_vertical() {
            Self::VERT_EXPANDED_HEIGHT
        } else {
            Self::EXPANDED_HEIGHT
        } * exp_s)
            .round();

        let (def_ax, def_ay) = Self::default_anchor_for_placement(placement);
        let target_anchor_x = loaded_anchor_x.unwrap_or(def_ax).clamp(0.0, 1.0);
        let target_anchor_y = loaded_anchor_y.unwrap_or(def_ay).clamp(0.0, 1.0);

        let mut st = Self {
                    mode: NotchMode::Home,
                    is_hovered: is_pinned,
                    is_hover_pending: false,
                    hover_dwell_t: 0.0,
                    anim_edge_glow_t: 0.0,
                    hover_dwell_start: None,
                    is_pinned,
            x_offset,
            y_offset,
            show_lyrics: false,
            lyrics_offset: 0,
            notch_style,
            placement,
            size_preset,
            expanded_size,
            type_scale,
            idle_metric,
            minimized_island,
            click_to_open,
            hide_in_fullscreen,
            show_mascot,
            progress_arc_enabled,
            fps_cap,
            detected_hz: 60,
            micro_interactions,
            equalizer_enabled,
            notifications_enabled,
            notification_secs,
                        notification_sound,
                        notification_volume,
                        hover_dwell_speed,
                        seen_hotkey_hint,
                        hotkey_hint_expand_count: 0,
            lyric_size,
            lyric_style,
            frosted_glass_enabled,
            frosted_opacity_pct,
            frosted_blur_px,
            windows_transparency_enabled,
            windows_animations_enabled,
            remove_windows_borders,
            notif_listener_enabled,
            notif_exclusive_mode,
            notif_auto_clear_action_center,
            notif_over_fullscreen,
            notif_listener_status,
            notif_suppressed_apps,
            modules,
            lyrics_enabled,
            clipboard_watch_enabled,
            capture_to_clipboard,
            screentime_enabled,
            settings_open: false,
            settings_category: SettingsCategory::General,
            settings_scroll: [0.0; SettingsCategory::COUNT],
            settings_scroll_target: [0.0; SettingsCategory::COUNT],
            current_width: if is_pinned { exp_w } else { idle_w },
            current_height: if is_pinned { exp_h } else { idle_h },
            target_width: if is_pinned { exp_w } else { idle_w },
            target_height: if is_pinned { exp_h } else { idle_h },
            anim_width_vel: 0.0,
            anim_height_vel: 0.0,
            anim_expand_t: if is_pinned { 1.0 } else { 0.0 },
            anim_expand_vel: 0.0,
            anim_notif_t: 0.0,
            anim_mode_t: 1.0,
            anim_mode_dir: 1.0,
            target_anchor_x,
            target_anchor_y,
            anim_anchor_x: target_anchor_x,
            anim_anchor_y: target_anchor_y,
            anim_dock_x: 24.0,
            anim_dock_w: 32.0,
            anim_dock_x_vel: 0.0,
            anim_dock_w_vel: 0.0,
            anim_settings_cat_y: Self::settings_cat_pill_y(SettingsCategory::General),
            anim_settings_cat_vel: 0.0,
            anim_settings_pane_t: 1.0,
            anim_settings_pane_dir: 1.0,
            anim_vol_pct: vol.clamp(0.0, 1.0),
            anim_ram_pct: ram_pct,
            hover_target_idx: None,
            hover_slots: [0.0; 48],
            settings_hover_idx: None,
            settings_hover_slots: [0.0; 48],
            switch_t: [0.0; 48],
            media: MediaTrack::idle(),
            lyrics: Vec::new(),
            current_lyric_index: None,
            current_lyric_text: "Play a track to sync lyrics".to_string(),
            telemetry,
            mem_reduct,
            capture: CaptureState::default(),
            quick_note,
            screen_time: ScreenTimeStats::default(),
            notification: None,
            notif_queue: VecDeque::new(),
            notif_history: VecDeque::new(),
            show_notif_history: false,
            purge_anim_until: None,
            shelf_items: Self::load_initial_shelf(),
            shelf_scroll: 0,
            tasks: Self::load_initial_tasks(),
            tasks_scroll: 0,
            anim_time: 0.0,
            volume_pct: vol,
            pre_mute_volume: pre_mute,
            autostart: crate::autostart::is_autostart_enabled(),
            chime_enabled,
            persistence_enabled: true,
        };
        let (dx, dw) = st.active_dock_target();
        st.anim_dock_x = dx;
        st.anim_dock_w = dw;
        st.sync_switches_immediate();
        if needs_migration_save {
            st.save_config();
        }
        st
    }

    /// Creates an isolated in-memory AppState for tests and visual snapshots
    /// so user files under %LOCALAPPDATA%\CyNotch are never mutated.
    pub fn new_for_preview() -> Self {
        let demo_lyrics = crate::media::default_demo_lyrics();
        let mut st = Self {
            mode: NotchMode::Home,
            is_hovered: false,
            is_hover_pending: false,
            hover_dwell_t: 0.0,
            anim_edge_glow_t: 0.0,
            hover_dwell_start: None,
            is_pinned: false,
            x_offset: 0,
            y_offset: 0,
            show_lyrics: false,
            lyrics_offset: 0,
            notch_style: NotchStyle::Floating,
            placement: NotchPlacement::Top,
            size_preset: NotchSizePreset::Standard,
            expanded_size: ExpandedSizePreset::Standard,
            type_scale: TypeScale::Standard,
            idle_metric: IdleMetric::Ram,
            minimized_island: false,
            click_to_open: false,
            hide_in_fullscreen: true,
            show_mascot: true,
            progress_arc_enabled: true,
            fps_cap: 0,
            detected_hz: 60,
            micro_interactions: true,
            equalizer_enabled: true,
            notifications_enabled: true,
            notification_secs: 4,
            notification_sound: crate::sound::NotificationSound::default(),
            notification_volume: 80,
            hover_dwell_speed: HoverDwellSpeed::Normal,
            seen_hotkey_hint: true,
            hotkey_hint_expand_count: 0,
            lyric_size: LyricSize::Standard,
            lyric_style: LyricStyle::ClassicList,
            frosted_glass_enabled: true,
            frosted_opacity_pct: 68,
            frosted_blur_px: 20,
            windows_transparency_enabled: false,
            windows_animations_enabled: true,
            remove_windows_borders: false,
            notif_listener_enabled: true,
            notif_exclusive_mode: false,
            notif_auto_clear_action_center: false,
            notif_over_fullscreen: false,
            notif_listener_status: "Listening".to_string(),
            notif_suppressed_apps: 0,
            modules: ModuleVisibility::default(),
            lyrics_enabled: true,
            clipboard_watch_enabled: true,
            capture_to_clipboard: true,
            screentime_enabled: true,
            settings_open: false,
            settings_category: SettingsCategory::General,
            settings_scroll: [0.0; SettingsCategory::COUNT],
            settings_scroll_target: [0.0; SettingsCategory::COUNT],
            current_width: Self::IDLE_WIDTH,
            current_height: Self::IDLE_HEIGHT,
            target_width: Self::IDLE_WIDTH,
            target_height: Self::IDLE_HEIGHT,
            anim_width_vel: 0.0,
            anim_height_vel: 0.0,
            anim_expand_t: 0.0,
            anim_expand_vel: 0.0,
            anim_notif_t: 0.0,
            anim_mode_t: 1.0,
            anim_mode_dir: 1.0,
            target_anchor_x: 0.5,
            target_anchor_y: 0.5,
            anim_anchor_x: 0.5,
            anim_anchor_y: 0.5,
            anim_dock_x: 24.0,
            anim_dock_w: 32.0,
            anim_dock_x_vel: 0.0,
            anim_dock_w_vel: 0.0,
            anim_settings_cat_y: Self::settings_cat_pill_y(SettingsCategory::General),
            anim_settings_cat_vel: 0.0,
            anim_settings_pane_t: 1.0,
            anim_settings_pane_dir: 1.0,
            anim_vol_pct: 0.70,
            anim_ram_pct: 0.57,
            hover_target_idx: None,
            hover_slots: [0.0; 48],
            settings_hover_idx: None,
            settings_hover_slots: [0.0; 48],
            switch_t: [0.0; 48],
            media: MediaTrack::preview_sample(),
            lyrics: demo_lyrics,
            current_lyric_index: Some(2),
            current_lyric_text: "Nowhere else we need to go".to_string(),
            telemetry: HardwareTelemetry::default(),
            mem_reduct: MemReductConfig::preview_sample(),
            capture: CaptureState::default(),
            quick_note: "14 words · Chrome".to_string(),
            screen_time: ScreenTimeStats::preview_sample(),
            notification: None,
            notif_queue: VecDeque::new(),
            notif_history: VecDeque::new(),
            show_notif_history: false,
            purge_anim_until: None,
            shelf_items: vec![
                ShelfItem {
                    kind: ShelfKind::Text,
                    label: "https://iamlayzee.com/".to_string(),
                    payload: "https://iamlayzee.com/".to_string(),
                },
                ShelfItem {
                    kind: ShelfKind::Text,
                    label: "cargo build --release".to_string(),
                    payload: "cargo build --release".to_string(),
                },
                ShelfItem {
                    kind: ShelfKind::File,
                    label: "CyNotch_Design_Spec.pdf".to_string(),
                    payload: "C:\\Users\\muhna\\Documents\\CyNotch_Design_Spec.pdf".to_string(),
                },
            ],
            shelf_scroll: 0,
            tasks: vec![
                TaskItem {
                    text: "Update CSM tns kalo sempet".to_string(),
                    completed: false,
                },
                TaskItem {
                    text: "Sketsa Logo Edu".to_string(),
                    completed: false,
                },
                TaskItem {
                    text: "CyNotch Mahamatra Memory Purge".to_string(),
                    completed: true,
                },
            ],
            tasks_scroll: 0,
            anim_time: 0.0,
            volume_pct: 0.70,
            pre_mute_volume: 0.70,
            autostart: false,
            chime_enabled: true,
            persistence_enabled: false,
        };
        st.sync_switches_immediate();
        st.sync_collapsed_targets();
        st.current_width = st.target_width;
        st.current_height = st.target_height;
        st
    }

    pub fn should_mute_windows_notif_sound(&self) -> bool {
        self.notifications_enabled
            && self.notif_listener_enabled
            && self.notification_sound != crate::sound::NotificationSound::Mute
    }

    pub const HOVER_DWELL_MS: u64 = 240;

    pub fn hover_dwell_ms(&self) -> u64 {
        self.hover_dwell_speed.ms()
    }

        pub fn begin_hover_dwell(&mut self, now: std::time::Instant) {
        self.is_hover_pending = true;
        self.hover_dwell_start = Some(now);
        self.hover_dwell_t = 0.0;
        // Start smoothly from 0.0 without any snap or pop-in
    }

    pub fn cancel_hover_dwell(&mut self) {
        self.is_hover_pending = false;
        self.hover_dwell_start = None;
        self.hover_dwell_t = 0.0;
    }

    /// Advances `hover_dwell_t` (`0.0..=1.0`, smoothstep) over `HOVER_DWELL_MS`.
    /// Returns `true` when `hover_dwell_t` changes and requires a redraw.
        pub fn update_hover_dwell(&mut self, now: std::time::Instant) -> bool {
            if self.is_hover_pending && !self.is_hovered {
                let start = *self.hover_dwell_start.get_or_insert(now);
                let raw = (now.saturating_duration_since(start).as_secs_f32()
                    / (self.hover_dwell_ms() as f32 / 1000.0))
                    .clamp(0.0, 1.0);
                let smooth = raw * raw * (3.0 - 2.0 * raw);
                let changed = (smooth - self.hover_dwell_t).abs() > 0.001
                    || (raw >= 1.0 && self.hover_dwell_t < 1.0);
                self.hover_dwell_t = smooth;
                changed
            } else if self.hover_dwell_t > 0.0 || self.hover_dwell_start.is_some() {
                self.hover_dwell_t = 0.0;
                self.hover_dwell_start = None;
                true
            } else {
                false
            }
        }

    pub fn record_notch_expand_for_hint(&mut self) -> bool {
        if !self.seen_hotkey_hint {
            self.hotkey_hint_expand_count = self.hotkey_hint_expand_count.saturating_add(1);
            if self.hotkey_hint_expand_count >= 3 {
                self.seen_hotkey_hint = true;
                return true;
            }
        }
        false
    }

    pub fn dismiss_hotkey_hint(&mut self) -> bool {
        if !self.seen_hotkey_hint {
            self.seen_hotkey_hint = true;
            return true;
        }
        false
    }

    pub fn set_hovered(&mut self, hovered: bool) {
        if hovered {
            self.cancel_hover_dwell();
        }
        if self.is_hovered == hovered {
            return;
        }
        self.is_hovered = hovered;
        if hovered {
            self.target_width = self.expanded_width();
            self.target_height = self.expanded_height();
        } else {
            self.target_width = self.collapsed_width();
            self.target_height = self.collapsed_height();
        }
    }

    pub fn set_mode(&mut self, mode: NotchMode) {
        if self.mode != mode || self.show_notif_history {
            let prev_idx = self.mode as isize;
            let next_idx = mode as isize;
            self.mode = mode;
            self.show_notif_history = false;
            self.hover_target_idx = None;
            self.hover_slots = [0.0; 48];
            if self.are_animations_active() {
                self.anim_mode_t = 0.0;
                self.anim_mode_dir = if next_idx >= prev_idx { 1.0 } else { -1.0 };
            } else {
                self.anim_mode_t = 1.0;
                self.anim_mode_dir = 1.0;
            }
        }
    }

    pub fn set_settings_category(&mut self, cat: SettingsCategory) {
        if self.settings_category != cat {
            let prev_idx = self.settings_category.index() as isize;
            let next_idx = cat.index() as isize;
            self.settings_category = cat;
            self.settings_hover_idx = None;
            self.settings_hover_slots = [0.0; 48];
            if self.are_animations_active() {
                self.anim_settings_pane_t = 0.0;
                self.anim_settings_pane_dir = if next_idx >= prev_idx { 1.0 } else { -1.0 };
            } else {
                self.anim_settings_pane_t = 1.0;
                self.anim_settings_cat_y = Self::settings_cat_pill_y(cat);
                self.anim_settings_cat_vel = 0.0;
            }
        }
    }

    pub fn toggle_mem_area(&mut self, idx: u8) {
        match idx {
            0 => self.mem_reduct.clean_working_sets = !self.mem_reduct.clean_working_sets,
            1 => self.mem_reduct.clean_system_ws = !self.mem_reduct.clean_system_ws,
            2 => self.mem_reduct.clean_standby_list = !self.mem_reduct.clean_standby_list,
            3 => {
                self.mem_reduct.clean_modified_page_list = !self.mem_reduct.clean_modified_page_list
            }
            4 => self.mem_reduct.clean_standby_low = !self.mem_reduct.clean_standby_low,
            5 => {
                self.mem_reduct.clean_modified_file_cache =
                    !self.mem_reduct.clean_modified_file_cache
            }
            6 => self.mem_reduct.clean_registry_cache = !self.mem_reduct.clean_registry_cache,
            7 => {
                self.mem_reduct.clean_combine_memory_lists =
                    !self.mem_reduct.clean_combine_memory_lists
            }
            _ => {}
        }
        self.save_config();
    }

    pub fn cycle_threshold(&mut self) {
        self.mem_reduct.when_above_pct = match self.mem_reduct.when_above_pct {
            70 => 80,
            80 => 90,
            90 => 0,
            0 => 60,
            _ => 70,
        };
        self.save_config();
    }

    pub fn cycle_interval(&mut self) {
        self.mem_reduct.every_mins = match self.mem_reduct.every_mins {
            0 => 5,
            5 => 15,
            15 => 30,
            30 => 60,
            60 => 90,
            _ => 0,
        };
        self.save_config();
    }

    pub fn should_trigger_threshold_clean(&self, secs_since_last: u64) -> bool {
        if !self.mem_reduct.auto_clean_enabled || self.mem_reduct.when_above_pct == 0 {
            return false;
        }
        self.telemetry.memory_percent >= (self.mem_reduct.when_above_pct as f32)
            && secs_since_last >= 60
    }

    pub fn should_trigger_interval_clean(&self, secs_since_last: u64) -> bool {
        if !self.mem_reduct.auto_clean_enabled || self.mem_reduct.every_mins == 0 {
            return false;
        }
        secs_since_last >= (self.mem_reduct.every_mins as u64) * 60
    }

    pub fn reset_all_settings(&mut self) {
        self.notch_style = NotchStyle::Floating;
        self.placement = NotchPlacement::Top;
        self.size_preset = NotchSizePreset::Standard;
        self.expanded_size = ExpandedSizePreset::Standard;
        self.type_scale = TypeScale::Standard;
        self.idle_metric = IdleMetric::Ram;
        self.minimized_island = false;
        self.click_to_open = false;
        self.hide_in_fullscreen = true;
        self.show_mascot = true;
        self.progress_arc_enabled = true;
        self.fps_cap = 0;
        self.micro_interactions = true;
        self.equalizer_enabled = true;
        self.notifications_enabled = true;
        self.notification_secs = 4;
        self.notification_sound = crate::sound::NotificationSound::default();
        self.notification_volume = 80;
        self.hover_dwell_speed = HoverDwellSpeed::Normal;
        self.lyric_size = LyricSize::Standard;
        self.lyric_style = LyricStyle::ClassicList;
        self.frosted_glass_enabled = true;
        self.frosted_opacity_pct = 68;
        self.frosted_blur_px = 20;
        self.notif_listener_enabled = true;
        self.notif_exclusive_mode = false;
        self.notif_auto_clear_action_center = false;
        self.notif_over_fullscreen = false;
        if self.notif_listener_status == "Off" {
            self.notif_listener_status = "Connecting...".to_string();
        }
        self.settings_scroll = [0.0; SettingsCategory::COUNT];
        self.settings_scroll_target = [0.0; SettingsCategory::COUNT];
        self.modules = ModuleVisibility::default();
        self.lyrics_enabled = true;
        self.clipboard_watch_enabled = true;
        self.capture_to_clipboard = true;
        self.screentime_enabled = true;
        self.chime_enabled = true;
        self.mem_reduct.clean_working_sets = true;
        self.mem_reduct.clean_system_ws = true;
        self.mem_reduct.clean_standby_list = true;
        self.mem_reduct.clean_modified_page_list = true;
        self.mem_reduct.clean_standby_low = false;
        self.mem_reduct.clean_modified_file_cache = false;
        self.mem_reduct.clean_registry_cache = false;
        self.mem_reduct.clean_combine_memory_lists = false;
        self.mem_reduct.show_clean_results = true;
        self.mem_reduct.when_above_pct = 70;
        self.mem_reduct.every_mins = 0;
        self.mem_reduct.auto_clean_enabled = true;
        self.x_offset = 0;
        self.y_offset = 0;
        self.reset_anchor_for_placement();
        if self.is_hovered {
            self.target_width = self.expanded_width();
            self.target_height = self.expanded_height();
        } else {
            self.target_width = self.idle_width();
            self.target_height = self.idle_height();
        }
        self.save_config();
    }

    pub fn set_notification(
        &mut self,
        title: String,
        subtitle: String,
        kind: NotificationKind,
        is_success: bool,
    ) {
        if !self.notifications_enabled {
            return;
        }
        let app_name = match kind {
            NotificationKind::SystemToast => "Windows".to_string(),
            NotificationKind::MemoryPurge => "MemReduct".to_string(),
            NotificationKind::Capture => "Capture".to_string(),
            NotificationKind::Pasted => "Shelf".to_string(),
            NotificationKind::General => "CyNotch".to_string(),
        };
        let secs = self.notification_secs.clamp(2, 10);
        let now = Instant::now();
        self.notification = Some(NotificationBanner {
            app_name,
            aumid: None,
            title,
            subtitle,
            kind,
            is_success,
            total_secs: secs as f32,
            expires_at: now + std::time::Duration::from_secs(secs as u64),
            grouped_count: 1,
            received_at: now,
        });
        self.anim_notif_t = 0.0;
        self.sync_collapsed_targets();
    }

    pub fn set_system_toast(&mut self, app_name: String, title: String, subtitle: String) {
        self.set_system_toast_with_aumid(app_name, title, subtitle, None);
    }

    pub fn set_system_toast_with_aumid(
        &mut self,
        app_name: String,
        title: String,
        subtitle: String,
        aumid: Option<String>,
    ) {
        if !self.notifications_enabled {
            return;
        }
        let now = Instant::now();
        let secs = self.notification_secs.clamp(2, 10).max(4);
        self.notification = Some(NotificationBanner {
            app_name,
            aumid,
            title,
            subtitle,
            kind: NotificationKind::SystemToast,
            is_success: true,
            total_secs: secs as f32,
            expires_at: now + std::time::Duration::from_secs(secs as u64),
            grouped_count: 1,
            received_at: now,
        });
        self.anim_notif_t = 0.0;
        self.sync_collapsed_targets();
    }

    fn same_notif_source(
        a_app: &str,
        a_aumid: Option<&str>,
        b_app: &str,
        b_aumid: Option<&str>,
    ) -> bool {
        if let (Some(aa), Some(ba)) = (a_aumid, b_aumid)
            && !aa.is_empty()
            && !ba.is_empty()
        {
            return aa.eq_ignore_ascii_case(ba);
        }
        a_app.eq_ignore_ascii_case(b_app)
    }

    pub fn record_notif_history(
        &mut self,
        app_name: String,
        aumid: Option<String>,
        title: String,
        subtitle: String,
    ) {
        self.notif_history.push_front(NotifHistoryEntry {
            app_name,
            aumid,
            title,
            subtitle,
            received_at: Instant::now(),
        });
        while self.notif_history.len() > 20 {
            self.notif_history.pop_back();
        }
    }

    /// Enqueues a system toast with per-app coalescing and history recording.
    pub fn push_system_toast(
        &mut self,
        app_name: String,
        title: String,
        subtitle: String,
        aumid: Option<String>,
    ) {
        self.record_notif_history(
            app_name.clone(),
            aumid.clone(),
            title.clone(),
            subtitle.clone(),
        );
        if !self.notifications_enabled {
            return;
        }
        let now = Instant::now();
        let secs = self.notification_secs.clamp(2, 10).max(4);
        let expires_at = now + std::time::Duration::from_secs(secs as u64);

        // Coalesce with the currently displayed banner if it came from the same source app.
        if let Some(ref mut active) = self.notification
            && active.kind == NotificationKind::SystemToast
            && Self::same_notif_source(
                &active.app_name,
                active.aumid.as_deref(),
                &app_name,
                aumid.as_deref(),
            )
        {
            active.title = title;
            active.subtitle = subtitle;
            active.grouped_count = active.grouped_count.saturating_add(1);
            active.total_secs = secs as f32;
            active.expires_at = expires_at;
            active.received_at = now;
            self.anim_notif_t = self.anim_notif_t.min(0.86);
            return;
        }

        // If no banner is currently visible, show immediately with full slide-in.
        if self.notification.is_none() {
            self.notification = Some(NotificationBanner {
                app_name,
                aumid,
                title,
                subtitle,
                kind: NotificationKind::SystemToast,
                is_success: true,
                total_secs: secs as f32,
                expires_at,
                grouped_count: 1,
                received_at: now,
            });
            self.anim_notif_t = 0.0;
            self.sync_collapsed_targets();
            return;
        }

        // Otherwise coalesce into an existing queued entry for the same app, or enqueue.
        if let Some(queued) = self.notif_queue.iter_mut().find(|q| {
            Self::same_notif_source(&q.app_name, q.aumid.as_deref(), &app_name, aumid.as_deref())
        }) {
            queued.title = title;
            queued.subtitle = subtitle;
            queued.grouped_count = queued.grouped_count.saturating_add(1);
            queued.total_secs = secs as f32;
            queued.expires_at = expires_at;
            queued.received_at = now;
            return;
        }

        if self.notif_queue.len() >= 12 {
            self.notif_queue.pop_front();
        }
        self.notif_queue.push_back(NotificationBanner {
            app_name,
            aumid,
            title,
            subtitle,
            kind: NotificationKind::SystemToast,
            is_success: true,
            total_secs: secs as f32,
            expires_at,
            grouped_count: 1,
            received_at: now,
        });
    }

    /// Advances to the next queued notification (keeping the banner seamlessly open),
    /// or collapses back to idle if the queue is empty. Returns true if a queued banner became active.
    pub fn advance_notification_queue(&mut self) -> bool {
        if let Some(mut next) = self.notif_queue.pop_front() {
            let secs = self.notification_secs.clamp(2, 10).max(4);
            let now = Instant::now();
            next.total_secs = secs as f32;
            next.expires_at = now + std::time::Duration::from_secs(secs as u64);
            let was_open = self.notification.is_some();
            self.notification = Some(next);
            self.anim_notif_t = if was_open { 0.78 } else { 0.0 };
            self.sync_collapsed_targets();
            true
        } else {
            self.notification = None;
            self.sync_collapsed_targets();
            false
        }
    }

    pub fn push_clipboard(&mut self, text: String, app_name: &str) {
        let words = text.split_whitespace().count();
        self.quick_note = format!("{} words · {}", words, app_name);
        let single_line = text.lines().next().unwrap_or(&text).trim().to_string();
        if !self
            .shelf_items
            .iter()
            .any(|s| s.kind == ShelfKind::Text && s.payload == text)
        {
            self.shelf_items.insert(
                0,
                ShelfItem {
                    kind: ShelfKind::Text,
                    label: single_line,
                    payload: text,
                },
            );
            if self.shelf_items.len() > 24 {
                self.shelf_items.pop();
            }
            self.shelf_scroll = 0;
            self.save_shelf();
        }
        self.set_notification(
            "Pasted".to_string(),
            self.quick_note.clone(),
            NotificationKind::Pasted,
            true,
        );
    }

    pub fn push_dropped_file(&mut self, full_path: String) {
        let label = std::path::Path::new(&full_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&full_path)
            .to_string();
        if !self
            .shelf_items
            .iter()
            .any(|s| s.kind == ShelfKind::File && s.payload == full_path)
        {
            self.shelf_items.insert(
                0,
                ShelfItem {
                    kind: ShelfKind::File,
                    label: label.clone(),
                    payload: full_path,
                },
            );
            if self.shelf_items.len() > 24 {
                self.shelf_items.pop();
            }
            self.shelf_scroll = 0;
            self.save_shelf();
        }
        self.set_notification(
            "Parked on Shelf".to_string(),
            label,
            NotificationKind::Pasted,
            true,
        );
    }

    pub fn expire_notification(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        if let Some(ref notif) = self.notification
            && now >= notif.expires_at
        {
            self.advance_notification_queue();
            changed = true;
        }
        if let Some(until) = self.purge_anim_until
            && now >= until
        {
            self.purge_anim_until = None;
            changed = true;
        }
        changed
    }

    pub fn mascot_mood(&self) -> MascotMood {
        if self.mem_reduct.is_purging
            || matches!(&self.notification, Some(n) if n.kind == NotificationKind::MemoryPurge)
            || matches!(self.purge_anim_until, Some(until) if Instant::now() < until)
        {
            MascotMood::Purging
        } else if matches!(&self.notification, Some(n) if n.kind == NotificationKind::Capture) {
            MascotMood::Capture
        } else if self.media.is_playing {
            MascotMood::Music
        } else {
            MascotMood::Idle
        }
    }

    pub fn is_mascot_visible(&self) -> bool {
        if !self.is_hovered {
            self.show_mascot
        } else {
            match self.mode {
                NotchMode::Home => true,
                NotchMode::Music => self.show_mascot && self.media.is_playing,
                NotchMode::MemReduct => {
                    self.show_mascot && self.mascot_mood() == MascotMood::Purging
                }
                _ => false,
            }
        }
    }

    pub fn is_size_moving(&self) -> bool {
        let expand_target = if self.is_hovered { 1.0 } else { 0.0 };
        (self.target_width - self.current_width).abs() >= 0.5
            || (self.target_height - self.current_height).abs() >= 0.5
            || (self.are_animations_active()
                && (self.anim_width_vel.abs() >= 4.0 || self.anim_height_vel.abs() >= 4.0))
            || (expand_target - self.anim_expand_t).abs() >= 0.01
            || (self.are_animations_active() && self.anim_mode_t < 0.992)
            || (self.notification.is_some()
                && !self.is_hovered
                && (1.0 - self.anim_notif_t).abs() >= 0.01)
            || (self.target_anchor_x - self.anim_anchor_x).abs() >= 0.01
            || (self.target_anchor_y - self.anim_anchor_y).abs() >= 0.01
    }

    pub fn is_micro_animating(&self) -> bool {
        if !self.micro_interactions || !self.are_animations_active() {
            return false;
        }
        let (dx, dw) = self.active_dock_target();
        if (dx - self.anim_dock_x).abs() >= 0.35
            || (dw - self.anim_dock_w).abs() >= 0.35
            || self.anim_dock_x_vel.abs() >= 3.0
            || self.anim_dock_w_vel.abs() >= 3.0
        {
            return true;
        }
        let ram_t = (self.telemetry.memory_percent / 100.0).clamp(0.0, 1.0);
        if (self.volume_pct.clamp(0.0, 1.0) - self.anim_vol_pct).abs() >= 0.005
            || (ram_t - self.anim_ram_pct).abs() >= 0.005
        {
            return true;
        }
        for (i, &val) in self.hover_slots.iter().enumerate() {
            let tgt = if self.hover_target_idx == Some(i) {
                1.0
            } else {
                0.0
            };
            if (tgt - val).abs() >= 0.015 {
                return true;
            }
        }
        false
    }

    pub fn is_settings_animating(&self) -> bool {
        if !self.are_animations_active() {
            return false;
        }
        let cat_tgt = Self::settings_cat_pill_y(self.settings_category);
        if (cat_tgt - self.anim_settings_cat_y).abs() >= 0.35
            || self.anim_settings_cat_vel.abs() >= 3.0
            || self.anim_settings_pane_t < 0.992
        {
            return true;
        }
        let cidx = self.settings_category.index();
        if (self.settings_scroll_target[cidx] - self.settings_scroll[cidx]).abs() >= 0.4 {
            return true;
        }
        for id in ToggleId::ALL {
            let tgt = if self.toggle_target(id) { 1.0 } else { 0.0 };
            if (tgt - self.switch_t[id as usize]).abs() >= 0.01 {
                return true;
            }
        }
        if self.micro_interactions {
            for (i, &val) in self.settings_hover_slots.iter().enumerate() {
                let tgt = if self.settings_hover_idx == Some(i) {
                    1.0
                } else {
                    0.0
                };
                if (tgt - val).abs() >= 0.015 {
                    return true;
                }
            }
        }
        false
    }

    pub fn needs_settings_timer_ms(&self) -> Option<u32> {
        if self.settings_open && self.is_settings_animating() {
            Some(self.desired_active_interval_ms())
        } else {
            None
        }
    }

    pub fn is_equalizer_visible(&self) -> bool {
        if !self.equalizer_enabled || !self.media.is_playing || !self.are_animations_active() {
            return false;
        }
        if !self.is_hovered {
            self.notification.is_none()
        } else {
            self.mode == NotchMode::Music && !self.show_lyrics
        }
    }

    /// Returns desired high-frequency timer interval (ms) or None when static.
    pub fn needs_animation_timer_ms(&self) -> Option<u32> {
        if !self.are_animations_active() {
            // Even with Windows Animation Effects disabled, keep a lightweight timer
            // while hover dwell is counting down or RAM purge is running.
            if (self.is_hover_pending && !self.is_hovered) || self.mem_reduct.is_purging {
                return Some(25);
            }
            return None;
        }
        let act = self.desired_active_interval_ms();
        if self.is_size_moving() || self.is_micro_animating() || self.mem_reduct.is_purging {
            Some(act)
        } else if (self.is_hover_pending && !self.is_hovered) || self.anim_edge_glow_t > 0.003 {
            Some(16)
        } else {
            let mascot_vis = self.is_mascot_visible();
            let mood = self.mascot_mood();
            if mascot_vis && mood == MascotMood::Purging {
                Some(20)
            } else if mascot_vis && mood == MascotMood::Capture {
                Some(25)
            } else if self.is_equalizer_visible() || (mascot_vis && mood == MascotMood::Music) {
                Some(25)
            } else if mascot_vis {
                Some(33)
            } else {
                None
            }
        }
    }

    pub fn update_animation(&mut self, dt: f32) -> bool {
        let anim_on = self.are_animations_active();
        if anim_on {
            self.anim_time += dt;
        }
        let expired = self.expire_notification();

        let mut moved = self.update_hover_dwell(std::time::Instant::now());
        let expand_tgt = if self.is_hovered { 1.0 } else { 0.0 };
        let (dx_tgt, dw_tgt) = self.active_dock_target();

        if !anim_on {
            // Exclusive Windows Animation Effects gate: snap every motion cleanly with zero animation.
            let target_glow = if self.is_hover_pending && !self.is_hovered {
                1.0
            } else {
                0.0
            };
            let snapped = (self.anim_edge_glow_t - target_glow).abs() > 0.001
                || (self.current_width - self.target_width).abs() > 0.5
                || (self.current_height - self.target_height).abs() > 0.5
                || (self.anim_expand_t - expand_tgt).abs() > 0.005
                || (self.anim_mode_t - 1.0).abs() > 0.005
                || (self.anim_anchor_x - self.target_anchor_x).abs() > 0.005
                || (self.anim_anchor_y - self.target_anchor_y).abs() > 0.005
                || (self.anim_dock_x - dx_tgt).abs() > 0.35
                || (self.anim_dock_w - dw_tgt).abs() > 0.35;
            self.anim_edge_glow_t = target_glow;
            self.current_width = self.target_width;
            self.current_height = self.target_height;
            self.anim_width_vel = 0.0;
            self.anim_height_vel = 0.0;
            self.anim_expand_t = expand_tgt;
            self.anim_expand_vel = 0.0;
            self.anim_mode_t = 1.0;
            self.anim_notif_t = if self.notification.is_some() { 1.0 } else { 0.0 };
            self.anim_anchor_x = self.target_anchor_x;
            self.anim_anchor_y = self.target_anchor_y;
            self.anim_dock_x = dx_tgt;
            self.anim_dock_w = dw_tgt;
            self.anim_dock_x_vel = 0.0;
            self.anim_dock_w_vel = 0.0;
            self.anim_vol_pct = self.volume_pct.clamp(0.0, 1.0);
            self.anim_ram_pct = (self.telemetry.memory_percent / 100.0).clamp(0.0, 1.0);
            for (i, slot) in self.hover_slots.iter_mut().enumerate() {
                *slot = if self.hover_target_idx == Some(i) { 1.0 } else { 0.0 };
            }
            return moved || snapped || expired || self.mem_reduct.is_purging;
        }

        let k_fast = 26.0;

        if self.is_hover_pending && !self.is_hovered {
            let t = self.hover_dwell_t.clamp(0.0, 1.0);
            // Smootherstep: t^3 * (t * (t * 6 - 15) + 10) for portal opening effect
            let smooth_t = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
            let (ng, mg) = ease_exp(self.anim_edge_glow_t, smooth_t, 10.5, dt, 0.002);
            self.anim_edge_glow_t = ng;
            moved |= mg;
        } else if self.anim_edge_glow_t > 0.0 {
            // Slow, silky, atmospheric fade-out (~1.2s luxurious dissipation)
            let (ng, mg) = ease_exp(self.anim_edge_glow_t, 0.0, 1.6, dt, 0.002);
            self.anim_edge_glow_t = if ng <= 0.002 { 0.0 } else { ng };
            moved |= mg;
        }

        let banner_sliding = self.notification.is_some() && !self.is_hovered;
        let (nw, mw, nh, mh) = if banner_sliding {
            // Snap to full banner size immediately so the banner slides in from the
            // notch's edge instead of scaling from small to large.
            let w_chg = (self.target_width - self.current_width).abs() > 0.5;
            let h_chg = (self.target_height - self.current_height).abs() > 0.5;
            self.anim_width_vel = 0.0;
            self.anim_height_vel = 0.0;
            (self.target_width, w_chg, self.target_height, h_chg)
        } else {
            // Framer-Motion critically/slightly-underdamped spring for silky organic morphs
            let (nw, vw, mw) = spring_step(
                self.current_width,
                self.anim_width_vel,
                self.target_width,
                360.0,
                33.0,
                dt,
                0.45,
                4.0,
            );
            let (nh, vh, mh) = spring_step(
                self.current_height,
                self.anim_height_vel,
                self.target_height,
                360.0,
                33.0,
                dt,
                0.45,
                4.0,
            );
            self.anim_width_vel = vw;
            self.anim_height_vel = vh;
            (nw, mw, nh, mh)
        };
        self.current_width = nw;
        self.current_height = nh;

        let mut m_notif = false;
        if banner_sliding {
            let (nn, mn) = ease_exp(self.anim_notif_t, 1.0, 20.0, dt, 0.005);
            self.anim_notif_t = nn;
            m_notif = mn;
        } else if self.notification.is_none() {
            self.anim_notif_t = 0.0;
        }

        let (nt, vt, mt) = spring_step(
            self.anim_expand_t,
            self.anim_expand_vel,
            expand_tgt,
            340.0,
            32.0,
            dt,
            0.006,
            0.05,
        );
        self.anim_expand_t = nt.clamp(0.0, 1.0);
        self.anim_expand_vel = vt;

        let (nmt, mmt) = ease_exp(self.anim_mode_t, 1.0, 14.5, dt, 0.006);
        self.anim_mode_t = nmt;

        let (nax, max_chg) = ease_exp(self.anim_anchor_x, self.target_anchor_x, k_fast, dt, 0.005);
        let (nay, may_chg) = ease_exp(self.anim_anchor_y, self.target_anchor_y, k_fast, dt, 0.005);
        self.anim_anchor_x = nax;
        self.anim_anchor_y = nay;
        moved |= mw || mh || mt || mmt || m_notif || max_chg || may_chg;

        if self.micro_interactions {
            let (ndx, vdx, mdx) = spring_step(
                self.anim_dock_x,
                self.anim_dock_x_vel,
                dx_tgt,
                420.0,
                34.0,
                dt,
                0.35,
                3.0,
            );
            let (ndw, vdw, mdw) = spring_step(
                self.anim_dock_w,
                self.anim_dock_w_vel,
                dw_tgt,
                420.0,
                34.0,
                dt,
                0.35,
                3.0,
            );
            self.anim_dock_x = ndx;
            self.anim_dock_w = ndw;
            self.anim_dock_x_vel = vdx;
            self.anim_dock_w_vel = vdw;

            let (nv, mv) = ease_exp(
                self.anim_vol_pct,
                self.volume_pct.clamp(0.0, 1.0),
                k_fast,
                dt,
                0.004,
            );
            let rt = (self.telemetry.memory_percent / 100.0).clamp(0.0, 1.0);
            let (nr, mr) = ease_exp(self.anim_ram_pct, rt, k_fast * 0.85, dt, 0.004);
            self.anim_vol_pct = nv;
            self.anim_ram_pct = nr;
            moved |= mdx || mdw || mv || mr;

            for (i, slot) in self.hover_slots.iter_mut().enumerate() {
                let tgt = if self.hover_target_idx == Some(i) {
                    1.0
                } else {
                    0.0
                };
                let (next, chg) = ease_exp(*slot, tgt, k_fast * 1.1, dt, 0.012);
                *slot = next;
                moved |= chg;
            }
        } else {
            self.anim_dock_x = dx_tgt;
            self.anim_dock_w = dw_tgt;
            self.anim_dock_x_vel = 0.0;
            self.anim_dock_w_vel = 0.0;
            self.anim_vol_pct = self.volume_pct.clamp(0.0, 1.0);
            self.anim_ram_pct = (self.telemetry.memory_percent / 100.0).clamp(0.0, 1.0);
        }

        // Redraw on active morphs, equalizer, expiration, or continuous mascot animations.
        let mascot_anim = self.is_mascot_visible();
        moved || self.is_equalizer_visible() || mascot_anim || self.mem_reduct.is_purging || expired
    }

    pub fn update_settings_animation(&mut self, dt: f32) -> bool {
        let cat_tgt = Self::settings_cat_pill_y(self.settings_category);
        let cidx = self.settings_category.index();

        if !self.are_animations_active() {
            let mut changed = (self.anim_settings_cat_y - cat_tgt).abs() > 0.25
                || (self.anim_settings_pane_t - 1.0).abs() > 0.005
                || (self.settings_scroll[cidx] - self.settings_scroll_target[cidx]).abs() > 0.25;
            self.anim_settings_cat_y = cat_tgt;
            self.anim_settings_cat_vel = 0.0;
            self.anim_settings_pane_t = 1.0;
            self.settings_scroll[cidx] = self.settings_scroll_target[cidx];
            for id in ToggleId::ALL {
                let tgt = if self.toggle_target(id) { 1.0 } else { 0.0 };
                if (self.switch_t[id as usize] - tgt).abs() > 0.005 {
                    self.switch_t[id as usize] = tgt;
                    changed = true;
                }
            }
            for (i, slot) in self.settings_hover_slots.iter_mut().enumerate() {
                let tgt = if self.settings_hover_idx == Some(i) { 1.0 } else { 0.0 };
                if (*slot - tgt).abs() > 0.005 {
                    *slot = tgt;
                    changed = true;
                }
            }
            return changed;
        }

        let k = 25.0;
        let mut changed = false;

        // Shared-element spring glide for sidebar category highlight pill
        let (ncy, vcy, mcy) = spring_step(
            self.anim_settings_cat_y,
            self.anim_settings_cat_vel,
            cat_tgt,
            440.0,
            34.0,
            dt,
            0.35,
            3.0,
        );
        self.anim_settings_cat_y = ncy;
        self.anim_settings_cat_vel = vcy;
        changed |= mcy;

        // Smooth directional fade/glide for settings detail pane
        let (npt, mpt) = ease_exp(self.anim_settings_pane_t, 1.0, 14.0, dt, 0.006);
        self.anim_settings_pane_t = npt;
        changed |= mpt;

        // Smooth wheel scroll interpolation
        let (nsc, msc) = ease_exp(
            self.settings_scroll[cidx],
            self.settings_scroll_target[cidx],
            18.5,
            dt,
            0.35,
        );
        self.settings_scroll[cidx] = nsc;
        changed |= msc;

        for id in ToggleId::ALL {
            let tgt = if self.toggle_target(id) { 1.0 } else { 0.0 };
            let (nt, mt) = ease_exp(self.switch_t[id as usize], tgt, k * 1.25, dt, 0.008);
            self.switch_t[id as usize] = nt;
            changed |= mt;
        }

        if self.micro_interactions {
            for (i, slot) in self.settings_hover_slots.iter_mut().enumerate() {
                let tgt = if self.settings_hover_idx == Some(i) {
                    1.0
                } else {
                    0.0
                };
                let (next, chg) = ease_exp(*slot, tgt, k * 1.35, dt, 0.012);
                *slot = next;
                changed |= chg;
            }
        }
        changed
    }

    pub fn sync_preview_settled(&mut self) {
        self.target_width = if self.is_hovered {
            self.expanded_width()
        } else {
            self.collapsed_width()
        };
        self.target_height = if self.is_hovered {
            self.expanded_height()
        } else {
            self.collapsed_height()
        };
        self.current_width = self.target_width;
        self.current_height = self.target_height;
        self.anim_width_vel = 0.0;
        self.anim_height_vel = 0.0;
        self.anim_expand_t = if self.is_hovered { 1.0 } else { 0.0 };
        self.anim_expand_vel = 0.0;
        self.anim_mode_t = 1.0;
        self.anim_notif_t = if self.notification.is_some() {
            1.0
        } else {
            0.0
        };
        self.anim_anchor_x = self.target_anchor_x;
        self.anim_anchor_y = self.target_anchor_y;
        let (dx, dw) = self.active_dock_target();
        self.anim_dock_x = dx;
        self.anim_dock_w = dw;
        self.anim_dock_x_vel = 0.0;
        self.anim_dock_w_vel = 0.0;
        self.anim_settings_cat_y = Self::settings_cat_pill_y(self.settings_category);
        self.anim_settings_cat_vel = 0.0;
        self.anim_settings_pane_t = 1.0;
        self.settings_scroll_target = self.settings_scroll;
        self.anim_vol_pct = self.volume_pct.clamp(0.0, 1.0);
        self.anim_ram_pct = (self.telemetry.memory_percent / 100.0).clamp(0.0, 1.0);
        self.sync_switches_immediate();
    }

    pub fn apply_clean_result(&mut self, res: CleanResult) {
        self.mem_reduct.last_freed_mb = res.freed_mb;
        let priv_lbl = if res.is_elevated { "Admin" } else { "User" };
        self.mem_reduct.last_cleaned_status = if res.kernel_caches_purged {
            format!(
                "{} procs + kernel • {:.0} MB ({}%->{}%)",
                res.trimmed_processes, res.freed_mb, res.old_load_pct, res.new_load_pct
            )
        } else {
            format!(
                "{} procs ({}) • {:.0} MB ({}%->{}%)",
                res.trimmed_processes, priv_lbl, res.freed_mb, res.old_load_pct, res.new_load_pct
            )
        };
        self.mem_reduct.cyno_quote = if res.freed_mb > 300.0 {
            "Judgment is passed! Excess RAM purged.".to_string()
        } else {
            "Processes comply with Mahamatra doctrine.".to_string()
        };
        self.purge_anim_until = Some(Instant::now() + std::time::Duration::from_millis(3600));
        self.save_config();
        if self.mem_reduct.show_clean_results {
            self.set_notification(
                "Memory Cleaned".to_string(),
                format!(
                    "{:.0} MB freed ({}% -> {}%) • {}",
                    res.freed_mb, res.old_load_pct, res.new_load_pct, priv_lbl
                ),
                NotificationKind::MemoryPurge,
                true,
            );
        }
    }
    }

    impl Default for AppState {
        fn default() -> Self {
            Self::new()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_hover_pending_cleared_on_set_hovered_true() {
            let mut state = AppState::default();
            state.is_hover_pending = true;
            assert!(state.is_hover_pending);
        
            state.set_hovered(true);
        
            assert!(state.is_hovered);
            assert!(!state.is_hover_pending, "is_hover_pending should be cleared when set_hovered(true) is called");
        }

        #[test]
        fn test_hover_pending_not_cleared_on_set_hovered_false() {
            let mut state = AppState::default();
            state.is_hover_pending = true;
        
            state.set_hovered(false);
        
            assert!(!state.is_hovered);
            assert!(state.is_hover_pending, "is_hover_pending should remain true when set_hovered(false) is called");
        }

        #[test]
        fn test_hotkey_hint_dismisses_after_three_expands_or_hotkey() {
            let mut state = AppState::new_for_preview();
            state.seen_hotkey_hint = false;
            assert!(!state.record_notch_expand_for_hint());
            assert!(!state.seen_hotkey_hint);
            assert!(!state.record_notch_expand_for_hint());
            assert!(!state.seen_hotkey_hint);
            assert!(state.record_notch_expand_for_hint());
            assert!(state.seen_hotkey_hint);
            assert!(!state.record_notch_expand_for_hint());

            let mut state2 = AppState::new_for_preview();
            state2.seen_hotkey_hint = false;
            assert!(state2.dismiss_hotkey_hint());
            assert!(state2.seen_hotkey_hint);
            assert!(!state2.dismiss_hotkey_hint());
        }
    }
