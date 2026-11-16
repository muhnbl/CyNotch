#![allow(unsafe_op_in_unsafe_fn)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Foundation::IReference;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds};
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, FILETIME};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_ENUMERATE_SUB_KEYS, KEY_READ, KEY_WRITE, REG_DWORD,
    REG_VALUE_TYPE,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const PERSONALIZE_SUBKEY: PCWSTR =
    windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
const NOTIF_SETTINGS_SUBKEY: PCWSTR =
    windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings");

#[derive(Serialize, Deserialize, Default)]
struct BannerBackupEntry {
    show_banner: Option<u32>,
    show_in_action_center: Option<u32>,
}

#[derive(Serialize, Deserialize, Default)]
struct BannerBackupStore {
    apps: BTreeMap<String, BannerBackupEntry>,
}

fn backup_file_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("CyNotch").join("notif_banner_backup.json")
}

/// Reads Windows Settings -> Personalization -> Colors -> Transparency effects.
pub fn is_windows_transparency_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PERSONALIZE_SUBKEY,
            0,
            KEY_READ,
            &mut hkey,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let val = query_reg_dword(hkey, windows::core::w!("EnableTransparency"));
        let _ = RegCloseKey(hkey);
        val.map(|v| v != 0).unwrap_or(false)
    }
}

pub fn open_windows_notification_settings() {
    unsafe {
        let _ = ShellExecuteW(
            None,
            windows::core::w!("open"),
            windows::core::w!("ms-settings:notifications"),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

pub fn open_windows_notification_privacy_settings() {
    unsafe {
        let _ = ShellExecuteW(
            None,
            windows::core::w!("open"),
            windows::core::w!("ms-settings:privacy-notifications"),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

pub fn open_windows_personalize_colors() {
    unsafe {
        let _ = ShellExecuteW(
            None,
            windows::core::w!("open"),
            windows::core::w!("ms-settings:colors"),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Strips control characters and normalizes whitespace for single-line capsule display.
pub fn sanitize_notification_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_space = true;
    for ch in raw.chars() {
        if ch.is_control() || ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

unsafe fn query_reg_dword(hkey: HKEY, name: PCWSTR) -> Option<u32> {
    let mut ty = REG_VALUE_TYPE(0);
    let mut val: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let status = RegQueryValueExW(
        hkey,
        name,
        None,
        Some(&mut ty),
        Some(&mut val as *mut u32 as *mut u8),
        Some(&mut size),
    );
    if status == ERROR_SUCCESS && ty == REG_DWORD && size == 4 {
        Some(val)
    } else {
        None
    }
}

unsafe fn set_reg_dword(hkey: HKEY, name: PCWSTR, value: u32) -> bool {
    let bytes = value.to_le_bytes();
    RegSetValueExW(hkey, name, 0, REG_DWORD, Some(&bytes)) == ERROR_SUCCESS
}

unsafe fn enum_notif_app_subkeys() -> Vec<String> {
    let mut result = Vec::new();
    let mut root = HKEY::default();
    if RegOpenKeyExW(
        HKEY_CURRENT_USER,
        NOTIF_SETTINGS_SUBKEY,
        0,
        KEY_READ | KEY_ENUMERATE_SUB_KEYS,
        &mut root,
    ) != ERROR_SUCCESS
    {
        return result;
    }

    let mut idx = 0u32;
    let mut name_buf = [0u16; 512];
    loop {
        let mut name_len = name_buf.len() as u32;
        let mut ft = FILETIME::default();
        let st = RegEnumKeyExW(
            root,
            idx,
            PWSTR(name_buf.as_mut_ptr()),
            &mut name_len,
            None,
            PWSTR::null(),
            None,
            Some(&mut ft),
        );
        if st == ERROR_NO_MORE_ITEMS {
            break;
        }
        if st == ERROR_SUCCESS && name_len > 0 {
            result.push(String::from_utf16_lossy(&name_buf[..name_len as usize]));
        }
        idx += 1;
    }
    let _ = RegCloseKey(root);
    result
}

/// Counts how many per-app Windows notification keys currently have native banners suppressed (`ShowBanner == 0`).
pub fn count_suppressed_apps() -> usize {
    unsafe {
        let subkeys = enum_notif_app_subkeys();
        let mut root = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            NOTIF_SETTINGS_SUBKEY,
            0,
            KEY_READ,
            &mut root,
        ) != ERROR_SUCCESS
        {
            return 0;
        }
        let mut count = 0usize;
        for name in subkeys {
            let sub_h = HSTRING::from(&name);
            let mut app_key = HKEY::default();
            if RegOpenKeyExW(root, &sub_h, 0, KEY_READ, &mut app_key) == ERROR_SUCCESS {
                if query_reg_dword(app_key, windows::core::w!("ShowBanner")) == Some(0) {
                    count += 1;
                }
                let _ = RegCloseKey(app_key);
            }
        }
        let _ = RegCloseKey(root);
        count
    }
}

/// Snapshots per-app `ShowBanner` / `ShowInActionCenter` values under
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\*`
/// and toggles native Windows popup banners off (`enable == true`) or restores them (`enable == false`).
/// NOTE: Never invoke automatically on startup or inside unit tests.
pub fn set_exclusive_windows_banners(enable: bool) -> usize {
    let backup_path = backup_file_path();

    if enable {
        let subkeys = unsafe { enum_notif_app_subkeys() };
        let mut store = if backup_path.exists() {
            std::fs::read_to_string(&backup_path)
                .ok()
                .and_then(|s| serde_json::from_str::<BannerBackupStore>(&s).ok())
                .unwrap_or_default()
        } else {
            BannerBackupStore::default()
        };

        let mut modified = 0usize;
        unsafe {
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                NOTIF_SETTINGS_SUBKEY,
                0,
                KEY_READ,
                &mut root,
            ) == ERROR_SUCCESS
            {
                for name in &subkeys {
                    let sub_h = HSTRING::from(name);
                    let mut app_key = HKEY::default();
                    if RegOpenKeyExW(root, &sub_h, 0, KEY_READ | KEY_WRITE, &mut app_key)
                        == ERROR_SUCCESS
                    {
                        store.apps.entry(name.clone()).or_insert_with(|| BannerBackupEntry {
                            show_banner: query_reg_dword(app_key, windows::core::w!("ShowBanner")),
                            show_in_action_center: query_reg_dword(
                                app_key,
                                windows::core::w!("ShowInActionCenter"),
                            ),
                        });
                        if set_reg_dword(app_key, windows::core::w!("ShowBanner"), 0) {
                            modified += 1;
                        }
                        let _ = set_reg_dword(app_key, windows::core::w!("ShowInActionCenter"), 1);
                        let _ = RegCloseKey(app_key);
                    }
                }
                let _ = RegCloseKey(root);
            }
        }

        if let Some(parent) = backup_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&store) {
            let _ = std::fs::write(&backup_path, json);
        }
        modified
    } else {
        let store_opt = std::fs::read_to_string(&backup_path)
            .ok()
            .and_then(|s| serde_json::from_str::<BannerBackupStore>(&s).ok());
        let mut restored = 0usize;
        if let Some(store) = store_opt {
            unsafe {
                let mut root = HKEY::default();
                if RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    NOTIF_SETTINGS_SUBKEY,
                    0,
                    KEY_READ,
                    &mut root,
                ) == ERROR_SUCCESS
                {
                    for (name, entry) in store.apps {
                        let sub_h = HSTRING::from(&name);
                        let mut app_key = HKEY::default();
                        if RegOpenKeyExW(root, &sub_h, 0, KEY_READ | KEY_WRITE, &mut app_key)
                            == ERROR_SUCCESS
                        {
                            match entry.show_banner {
                                Some(prev) => {
                                    let _ = set_reg_dword(
                                        app_key,
                                        windows::core::w!("ShowBanner"),
                                        prev,
                                    );
                                }
                                None => {
                                    let _ = RegDeleteValueW(
                                        app_key,
                                        windows::core::w!("ShowBanner"),
                                    );
                                }
                            }
                            match entry.show_in_action_center {
                                Some(prev) => {
                                    let _ = set_reg_dword(
                                        app_key,
                                        windows::core::w!("ShowInActionCenter"),
                                        prev,
                                    );
                                }
                                None => {
                                    let _ = RegDeleteValueW(
                                        app_key,
                                        windows::core::w!("ShowInActionCenter"),
                                    );
                                }
                            }
                            restored += 1;
                            let _ = RegCloseKey(app_key);
                        }
                    }
                    let _ = RegCloseKey(root);
                }
            }
            let _ = std::fs::remove_file(&backup_path);
        }
        restored
    }
}

/// Signature of the Action Center database files: `(mtime_ns, size)` per file.
pub type NotifDbSignature = [Option<(u64, u64)>; 3];

const NOTIF_DB_FILES: [&str; 3] = ["wpndatabase.db", "wpndatabase.db-wal", "wpndatabase.db-shm"];
/// Upper bound on how long banners may wait for the DB signature to move.
const FORCED_POLL_SECS: u64 = 5;

/// Cheap change signal for the Action Center database.
///
/// `GetNotificationsAsync` marshals every toast across RPC (~1 ms CPU per toast,
/// measured 25-35 ms with 21 toasts), so calling it at 1 Hz is the largest idle
/// cost in the app. The platform persists toast changes to this SQLite WAL, so an
/// unchanged signature means "nothing new to poll". Measured: zero writes in 60 s
/// of an idle desktop, and a write on toast activity.
///
/// shortcut: relies on the platform touching these files; if a future build stops
/// doing that, `should_poll_toasts` still forces a poll every `FORCED_POLL_SECS`.
pub fn notifications_db_signature() -> NotifDbSignature {
    static DB_PATHS: std::sync::OnceLock<Option<[PathBuf; 3]>> = std::sync::OnceLock::new();
    let mut out: NotifDbSignature = [None; 3];
    let Some(paths) = DB_PATHS
        .get_or_init(|| {
            let base = std::env::var_os("LOCALAPPDATA")?;
            let dir = PathBuf::from(base)
                .join("Microsoft")
                .join("Windows")
                .join("Notifications");
            Some(NOTIF_DB_FILES.map(|name| dir.join(name)))
        })
        .as_ref()
    else {
        return out;
    };
    for (slot, path) in out.iter_mut().zip(paths.iter()) {
        let Ok(md) = std::fs::metadata(path) else {
            continue;
        };
        let Some(mtime) = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        else {
            continue;
        };
        *slot = Some((mtime.as_nanos() as u64, md.len()));
    }
    out
}

/// Decides whether the expensive toast poll is worth running this tick.
///
/// `prev` is the signature recorded before the last poll (`None` right after
/// connecting) and `secs_since_full` is the time since the last real poll. A
/// missing database (non-standard install) keeps the old always-poll behaviour.
pub fn should_poll_toasts(
    prev: Option<NotifDbSignature>,
    cur: &NotifDbSignature,
    secs_since_full: u64,
) -> bool {
    if cur.iter().all(|f| f.is_none()) {
        return true;
    }
    match prev {
        None => true,
        Some(prev) => prev != *cur || secs_since_full >= FORCED_POLL_SECS,
    }
}

pub struct SystemNotificationBridge {
    listener: Option<UserNotificationListener>,
    seen_ids: HashSet<u32>,
    status: String,
    initialized: bool,
}

impl Default for SystemNotificationBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemNotificationBridge {
    pub fn new() -> Self {
        Self {
            listener: None,
            seen_ids: HashSet::new(),
            status: "Off".to_string(),
            initialized: false,
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn is_connected(&self) -> bool {
        self.listener.is_some()
    }

    pub fn reset(&mut self) {
        self.listener = None;
        self.seen_ids.clear();
        self.status = "Off".to_string();
        self.initialized = false;
    }

    /// Connects to Windows `UserNotificationListener`, requests access, and seeds
    /// existing toast IDs so historical Action Center notifications don't replay.
    pub fn try_init(&mut self) -> String {
        if self.initialized && self.listener.is_some() {
            return self.status.clone();
        }
        self.initialized = true;
        let Ok(listener) = UserNotificationListener::Current() else {
            self.status = "Unsupported".to_string();
            return self.status.clone();
        };

        let access = match listener.RequestAccessAsync() {
            Ok(op) => match op.get() {
                Ok(st) => st,
                Err(_) => {
                    self.status = "Access Error".to_string();
                    return self.status.clone();
                }
            },
            Err(_) => {
                self.status = "Unsupported".to_string();
                return self.status.clone();
            }
        };

        if access != UserNotificationListenerAccessStatus::Allowed {
            self.listener = None;
            self.status = match access {
                UserNotificationListenerAccessStatus::Denied => "Access Denied".to_string(),
                _ => "Permission Needed".to_string(),
            };
            return self.status.clone();
        }

        // Seed currently present Action Center toast IDs so only new incoming notifications pop.
        self.seen_ids.clear();
        if let Ok(op) = listener.GetNotificationsAsync(NotificationKinds::Toast)
            && let Ok(list) = op.get()
        {
            for n in list {
                if let Ok(id) = n.Id() {
                    self.seen_ids.insert(id);
                }
            }
        }
        self.listener = Some(listener);
        self.status = "Connected".to_string();
        self.status.clone()
    }

    /// Polls Windows for newly arrived toast notifications since the last tick.
    /// Returns `(notification_id, app_name, title, subtitle)` tuples.
    pub fn poll_new_toasts(&mut self) -> Vec<(u32, String, String, String)> {
        let Some(ref listener) = self.listener else {
            return Vec::new();
        };
        let list = match listener.GetNotificationsAsync(NotificationKinds::Toast) {
            Ok(op) => match op.get() {
                Ok(l) => l,
                Err(_) => {
                    self.listener = None;
                    self.initialized = false;
                    self.status = "Reconnecting...".to_string();
                    return Vec::new();
                }
            },
            Err(_) => {
                self.listener = None;
                self.initialized = false;
                self.status = "Reconnecting...".to_string();
                return Vec::new();
            }
        };

        let mut active_ids = HashSet::new();
        let mut incoming = Vec::new();
        let template_binding = KnownNotificationBindings::ToastGeneric().ok();

        for notif in list {
            let Ok(id) = notif.Id() else {
                continue;
            };
            active_ids.insert(id);
            if self.seen_ids.contains(&id) {
                continue;
            }
            self.seen_ids.insert(id);

            let app_name = notif
                .AppInfo()
                .ok()
                .and_then(|ai| ai.DisplayInfo().ok())
                .and_then(|di| di.DisplayName().ok())
                .map(|s| sanitize_notification_text(&s.to_string_lossy()))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Windows".to_string());

            let mut lines: Vec<String> = Vec::new();
            if let Ok(n_data) = notif.Notification()
                && let Ok(visual) = n_data.Visual()
            {
                if let Some(ref tb) = template_binding
                    && let Ok(binding) = visual.GetBinding(tb)
                    && let Ok(elems) = binding.GetTextElements()
                {
                    for el in elems {
                        if let Ok(txt) = el.Text() {
                            let clean = sanitize_notification_text(&txt.to_string_lossy());
                            if !clean.is_empty() {
                                lines.push(clean);
                            }
                        }
                    }
                }
                if lines.is_empty()
                    && let Ok(bindings) = visual.Bindings()
                {
                    for binding in bindings {
                        if let Ok(elems) = binding.GetTextElements() {
                            for el in elems {
                                if let Ok(txt) = el.Text() {
                                    let clean = sanitize_notification_text(&txt.to_string_lossy());
                                    if !clean.is_empty() {
                                        lines.push(clean);
                                    }
                                }
                            }
                        }
                        if !lines.is_empty() {
                            break;
                        }
                    }
                }
            }

            let title = if let Some(first) = lines.first() {
                first.clone()
            } else {
                app_name.clone()
            };
            let subtitle = if lines.len() > 1 {
                lines[1..].join(" • ")
            } else {
                format!("New notification from {}", app_name)
            };

            incoming.push((id, app_name, title, subtitle));
        }

        // Prevent unbounded growth of seen_ids once notifications are dismissed.
        self.seen_ids.retain(|id| active_ids.contains(id));

        incoming
    }

    pub fn remove_toast(&self, id: u32) {
        if let Some(ref listener) = self.listener {
            let _ = listener.RemoveNotification(id);
        }
    }
}

// Keep unused import check quiet in case PropertyValue helpers are needed elsewhere
#[allow(dead_code)]
fn _keep_winrt_ref_import(_: Option<IReference<f32>>) {}
