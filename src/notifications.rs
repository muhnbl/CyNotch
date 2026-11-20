#![allow(unsafe_op_in_unsafe_fn)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use windows::Foundation::IReference;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds};
use windows::Win32::Foundation::{
    BOOL, CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, FILETIME, HWND,
    LPARAM, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_ENUMERATE_SUB_KEYS, KEY_READ, KEY_WRITE, REG_DWORD,
    REG_VALUE_TYPE, RegCloseKey, RegDeleteValueW, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{
    AO_NONE, ApplicationActivationManager, IApplicationActivationManager, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SPI_GETCLIENTAREAANIMATION, SW_RESTORE, SW_SHOWNORMAL, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SetForegroundWindow, ShowWindow, SystemParametersInfoW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

const PERSONALIZE_SUBKEY: PCWSTR =
    windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
const NOTIF_SETTINGS_SUBKEY: PCWSTR =
    windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings");

static SOUND_MUTE_LOCK: Mutex<()> = Mutex::new(());
static BANNER_TOGGLE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
struct BannerBackupEntry {
    show_banner: Option<u32>,
    show_in_action_center: Option<u32>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
struct BannerBackupStore {
    apps: BTreeMap<String, BannerBackupEntry>,
}

impl BannerBackupStore {
    fn is_valid(&self) -> bool {
        self.apps.keys().all(|k| !k.trim().is_empty())
    }
}

fn backup_file_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("CyNotch").join("notif_banner_backup.json")
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
struct SoundMuteBackup {
    prev_allow_sound: Option<u32>,
}

fn sound_mute_backup_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("CyNotch").join("notif_sound_backup.json")
}

const ALLOW_NOTIF_SOUND_VAL: PCWSTR =
    windows::core::w!("NOC_GLOBAL_SETTING_ALLOW_NOTIFICATION_SOUND");

/// Minimal abstraction over `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings`
/// so registry integrity, backup ordering, partial failures, and rollback can be exhaustively
/// unit-tested without ever mutating the host Windows registry.
trait NotifRegistry {
    fn query_root_dword(&self, value_name: &str) -> Result<Option<u32>, ()>;
    fn set_root_dword(&mut self, value_name: &str, value: u32) -> Result<(), ()>;
    fn delete_root_value(&mut self, value_name: &str) -> Result<(), ()>;
    fn enum_app_subkeys(&self) -> Result<Vec<String>, ()>;
    fn app_subkey_exists(&self, app: &str) -> Result<bool, ()>;
    fn query_app_dword(&self, app: &str, value_name: &str) -> Result<Option<u32>, ()>;
    fn set_app_dword(&mut self, app: &str, value_name: &str, value: u32) -> Result<(), ()>;
    fn delete_app_value(&mut self, app: &str, value_name: &str) -> Result<(), ()>;
}

struct WindowsNotifRegistry;

impl NotifRegistry for WindowsNotifRegistry {
    fn query_root_dword(&self, value_name: &str) -> Result<Option<u32>, ()> {
        unsafe {
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                NOTIF_SETTINGS_SUBKEY,
                0,
                KEY_READ,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return Err(());
            }
            let name_h = HSTRING::from(value_name);
            let val = query_reg_dword(root, PCWSTR(name_h.as_ptr()));
            let _ = RegCloseKey(root);
            Ok(val)
        }
    }

    fn set_root_dword(&mut self, value_name: &str, value: u32) -> Result<(), ()> {
        unsafe {
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                NOTIF_SETTINGS_SUBKEY,
                0,
                KEY_READ | KEY_WRITE,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return Err(());
            }
            let name_h = HSTRING::from(value_name);
            let ok = set_reg_dword(root, PCWSTR(name_h.as_ptr()), value);
            let _ = RegCloseKey(root);
            if ok { Ok(()) } else { Err(()) }
        }
    }

    fn delete_root_value(&mut self, value_name: &str) -> Result<(), ()> {
        unsafe {
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                NOTIF_SETTINGS_SUBKEY,
                0,
                KEY_READ | KEY_WRITE,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return Err(());
            }
            let name_h = HSTRING::from(value_name);
            let st = RegDeleteValueW(root, PCWSTR(name_h.as_ptr()));
            let _ = RegCloseKey(root);
            if st == ERROR_SUCCESS || st == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(())
            }
        }
    }

    fn enum_app_subkeys(&self) -> Result<Vec<String>, ()> {
        unsafe {
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
                return Err(());
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
            Ok(result)
        }
    }

    fn app_subkey_exists(&self, app: &str) -> Result<bool, ()> {
        unsafe {
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                NOTIF_SETTINGS_SUBKEY,
                0,
                KEY_READ,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return Err(());
            }
            let sub_h = HSTRING::from(app);
            let mut app_key = HKEY::default();
            let st = RegOpenKeyExW(root, &sub_h, 0, KEY_READ, &mut app_key);
            if st == ERROR_SUCCESS {
                let _ = RegCloseKey(app_key);
                let _ = RegCloseKey(root);
                Ok(true)
            } else {
                let _ = RegCloseKey(root);
                if st == ERROR_FILE_NOT_FOUND {
                    Ok(false)
                } else {
                    Err(())
                }
            }
        }
    }

    fn query_app_dword(&self, app: &str, value_name: &str) -> Result<Option<u32>, ()> {
        unsafe {
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                NOTIF_SETTINGS_SUBKEY,
                0,
                KEY_READ,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return Err(());
            }
            let sub_h = HSTRING::from(app);
            let mut app_key = HKEY::default();
            if RegOpenKeyExW(root, &sub_h, 0, KEY_READ, &mut app_key) != ERROR_SUCCESS {
                let _ = RegCloseKey(root);
                return Err(());
            }
            let name_h = HSTRING::from(value_name);
            let val = query_reg_dword(app_key, PCWSTR(name_h.as_ptr()));
            let _ = RegCloseKey(app_key);
            let _ = RegCloseKey(root);
            Ok(val)
        }
    }

    fn set_app_dword(&mut self, app: &str, value_name: &str, value: u32) -> Result<(), ()> {
        unsafe {
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                NOTIF_SETTINGS_SUBKEY,
                0,
                KEY_READ,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return Err(());
            }
            let sub_h = HSTRING::from(app);
            let mut app_key = HKEY::default();
            if RegOpenKeyExW(root, &sub_h, 0, KEY_READ | KEY_WRITE, &mut app_key) != ERROR_SUCCESS {
                let _ = RegCloseKey(root);
                return Err(());
            }
            let name_h = HSTRING::from(value_name);
            let ok = set_reg_dword(app_key, PCWSTR(name_h.as_ptr()), value);
            let _ = RegCloseKey(app_key);
            let _ = RegCloseKey(root);
            if ok { Ok(()) } else { Err(()) }
        }
    }

    fn delete_app_value(&mut self, app: &str, value_name: &str) -> Result<(), ()> {
        unsafe {
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                NOTIF_SETTINGS_SUBKEY,
                0,
                KEY_READ,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return Err(());
            }
            let sub_h = HSTRING::from(app);
            let mut app_key = HKEY::default();
            if RegOpenKeyExW(root, &sub_h, 0, KEY_READ | KEY_WRITE, &mut app_key) != ERROR_SUCCESS {
                let _ = RegCloseKey(root);
                return Err(());
            }
            let name_h = HSTRING::from(value_name);
            let st = RegDeleteValueW(app_key, PCWSTR(name_h.as_ptr()));
            let _ = RegCloseKey(app_key);
            let _ = RegCloseKey(root);
            if st == ERROR_SUCCESS || st == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}

const ALLOW_NOTIF_SOUND_NAME: &str = "NOC_GLOBAL_SETTING_ALLOW_NOTIFICATION_SOUND";
const SHOW_BANNER_NAME: &str = "ShowBanner";
const SHOW_IN_AC_NAME: &str = "ShowInActionCenter";

fn sync_windows_notification_sound_mute_with(
    reg: &mut dyn NotifRegistry,
    backup_path: &Path,
    mute: bool,
) -> bool {
    if mute {
        let mut created_backup = false;
        match crate::persistence::load_json_with_recovery::<SoundMuteBackup, _>(backup_path, |_| {
            true
        }) {
            crate::persistence::LoadOutcome::Corrupted { .. } => {
                // Never overwrite a corrupted backup or mutate registry without a valid backup.
                return false;
            }
            crate::persistence::LoadOutcome::Loaded(_)
            | crate::persistence::LoadOutcome::RecoveredFromBackup(_) => {
                // Already backed up original value — idempotent re-mute.
            }
            crate::persistence::LoadOutcome::Missing => {
                let Ok(prev) = reg.query_root_dword(ALLOW_NOTIF_SOUND_NAME) else {
                    return false;
                };
                let backup = SoundMuteBackup {
                    prev_allow_sound: prev,
                };
                if crate::persistence::atomic_write_json(backup_path, &backup, true).is_err() {
                    // BH-11: Never mute system notification sounds if backup creation failed.
                    return false;
                }
                created_backup = true;
            }
        }

        if reg.set_root_dword(ALLOW_NOTIF_SOUND_NAME, 0).is_ok() {
            true
        } else {
            if created_backup {
                let _ = crate::persistence::remove_with_backup(backup_path);
            }
            false
        }
    } else {
        let backup = match crate::persistence::load_json_with_recovery::<SoundMuteBackup, _>(
            backup_path,
            |_| true,
        ) {
            crate::persistence::LoadOutcome::Loaded(b)
            | crate::persistence::LoadOutcome::RecoveredFromBackup(b) => b,
            crate::persistence::LoadOutcome::Missing
            | crate::persistence::LoadOutcome::Corrupted { .. } => {
                // Keep any corrupted backup intact for manual recovery; do not mutate registry.
                return false;
            }
        };

        // Respect external changes: only restore if value is still muted (0) by CyNotch.
        let Ok(current) = reg.query_root_dword(ALLOW_NOTIF_SOUND_NAME) else {
            // BH-11: Keep backup intact so restoration remains retryable.
            return false;
        };
        if current == Some(0) {
            let restore_res = match backup.prev_allow_sound {
                Some(prev) => reg.set_root_dword(ALLOW_NOTIF_SOUND_NAME, prev),
                None => reg.delete_root_value(ALLOW_NOTIF_SOUND_NAME),
            };
            if restore_res.is_err() {
                // BH-11: Never delete backup if registry restoration failed.
                return false;
            }
        }
        let _ = crate::persistence::remove_with_backup(backup_path);
        true
    }
}

/// Mutes or restores native Windows toast notification sounds so CyNotch's selected chime
/// plays exclusively without double-beeping while CyNotch notifications are enabled.
/// Persists the user's original registry value to `%LOCALAPPDATA%\CyNotch\notif_sound_backup.json`
/// atomically before mutating the registry, and cleanly restores it when disabled or on shutdown.
pub fn sync_windows_notification_sound_mute(mute: bool) -> bool {
    let _guard = SOUND_MUTE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let backup_path = sound_mute_backup_path();
    let mut reg = WindowsNotifRegistry;
    let _ = ALLOW_NOTIF_SOUND_VAL;
    sync_windows_notification_sound_mute_with(&mut reg, &backup_path, mute)
}

/// Recovers user's original Windows notification sound setting on startup if a prior run
/// was abnormally terminated while muted.
pub fn recover_windows_sound_mute_on_startup(should_be_muted: bool) {
    let backup_path = sound_mute_backup_path();
    if !should_be_muted
        && (backup_path.exists() || crate::persistence::backup_path(&backup_path).exists())
    {
        let _ = sync_windows_notification_sound_mute(false);
    }
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

/// Reads Windows Settings -> Accessibility -> Visual effects -> Animation effects
/// (`SPI_GETCLIENTAREAANIMATION`). When disabled in Windows, CyNotch disables all
/// spring/choreographed motion and snaps transitions cleanly.
pub fn is_windows_animations_enabled() -> bool {
    unsafe {
        let mut enabled: i32 = 1;
        let ok = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut i32 as *mut std::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if ok.is_ok() {
            enabled != 0
        } else {
            true
        }
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

/// Invisible Unicode formatting / bidi / zero-width code points that render as
/// dotted-box sigla (`[FSI]`, `[PDI]`, `[LRI]`, `[RLI]`, `[LRM]`, `[RLM]`, `[ZWSP]`, `[BOM]`)
/// in `Segoe UI Variable Text` / DirectWrite when injected by apps like Discord, WhatsApp, or browsers.
#[inline]
pub fn is_invisible_format_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}' // Soft Hyphen (SHY)
        | '\u{034F}' // Combining Grapheme Joiner (CGJ)
        | '\u{061C}' // Arabic Letter Mark (ALM)
        | '\u{180E}' // Mongolian Vowel Separator
        | '\u{200B}'..='\u{200F}' // ZWSP, ZWNJ, ZWJ, LRM, RLM (note: ZWJ inside emoji sequences is rarely needed in single-glyph Segoe UI, but wait — keep ZWJ \u{200D} for multi-person/gender emoji ligatures!)
        | '\u{2028}'..='\u{202E}' // Line/Paragraph Separator, LRE, RLE, PDF, LRO, RLO
        | '\u{2060}'..='\u{206F}' // Word Joiner, Invisible operators, LRI (\u{2066}), RLI (\u{2067}), FSI (\u{2068}), PDI (\u{2069}), deprecated format chars
        | '\u{FEFF}' // Zero Width No-Break Space / BOM
        | '\u{FFF9}'..='\u{FFFB}' // Interlinear annotation anchors
    ) && ch != '\u{200D}' // Preserve Zero-Width Joiner (U+200D) so compound emoji (e.g. 👨‍👩‍👧, 🏳️‍🌈) stay joined
}

/// Strips control characters, invisible Unicode bidi/formatting markers (`FSI`/`PDI`/`LRM`/etc.),
/// and normalizes whitespace for single-line capsule display.
pub fn sanitize_notification_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_space = true;
    for ch in raw.chars() {
        if is_invisible_format_char(ch) {
            continue;
        }
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

fn count_suppressed_apps_with(reg: &dyn NotifRegistry) -> usize {
    let Ok(subkeys) = reg.enum_app_subkeys() else {
        return 0;
    };
    let mut count = 0usize;
    for name in subkeys {
        if reg.query_app_dword(&name, SHOW_BANNER_NAME).ok().flatten() == Some(0) {
            count += 1;
        }
    }
    count
}

/// Counts how many per-app Windows notification keys currently have native banners suppressed (`ShowBanner == 0`).
pub fn count_suppressed_apps() -> usize {
    let reg = WindowsNotifRegistry;
    count_suppressed_apps_with(&reg)
}

fn set_exclusive_windows_banners_with(
    reg: &mut dyn NotifRegistry,
    backup_path: &Path,
    enable: bool,
) -> usize {
    if enable {
        // BH-10: Never interpret a corrupted backup as empty — abort without mutating registry.
        let mut store = match crate::persistence::load_json_with_recovery::<BannerBackupStore, _>(
            backup_path,
            BannerBackupStore::is_valid,
        ) {
            crate::persistence::LoadOutcome::Loaded(s)
            | crate::persistence::LoadOutcome::RecoveredFromBackup(s) => s,
            crate::persistence::LoadOutcome::Missing => BannerBackupStore::default(),
            crate::persistence::LoadOutcome::Corrupted { .. } => {
                return count_suppressed_apps_with(reg);
            }
        };

        let Ok(subkeys) = reg.enum_app_subkeys() else {
            return count_suppressed_apps_with(reg);
        };

        // Snapshot all original values BEFORE modifying any registry entry.
        // Existing backup entries are preserved (idempotent re-enable).
        let mut targets: Vec<String> = Vec::new();
        for name in subkeys {
            if name.trim().is_empty() {
                continue;
            }
            if !store.apps.contains_key(&name) {
                let Ok(show_banner) = reg.query_app_dword(&name, SHOW_BANNER_NAME) else {
                    continue;
                };
                let Ok(show_in_ac) = reg.query_app_dword(&name, SHOW_IN_AC_NAME) else {
                    continue;
                };
                store.apps.insert(
                    name.clone(),
                    BannerBackupEntry {
                        show_banner,
                        show_in_action_center: show_in_ac,
                    },
                );
            }
            targets.push(name);
        }

        // Persist durable atomic backup FIRST. If backup fails, never mutate registry!
        if !store.apps.is_empty()
            && crate::persistence::atomic_write_json(backup_path, &store, true).is_err()
        {
            return count_suppressed_apps_with(reg);
        }

        for name in &targets {
            let _ = reg.set_app_dword(name, SHOW_BANNER_NAME, 0);
            let _ = reg.set_app_dword(name, SHOW_IN_AC_NAME, 1);
        }

        count_suppressed_apps_with(reg)
    } else {
        let store = match crate::persistence::load_json_with_recovery::<BannerBackupStore, _>(
            backup_path,
            BannerBackupStore::is_valid,
        ) {
            crate::persistence::LoadOutcome::Loaded(s)
            | crate::persistence::LoadOutcome::RecoveredFromBackup(s) => s,
            crate::persistence::LoadOutcome::Missing
            | crate::persistence::LoadOutcome::Corrupted { .. } => {
                // Keep corrupted backup intact; report actual suppressed count.
                return count_suppressed_apps_with(reg);
            }
        };

        let mut remaining = BannerBackupStore::default();
        for (name, entry) in store.apps {
            match reg.app_subkey_exists(&name) {
                Ok(false) => {
                    // App was uninstalled; nothing to restore.
                    continue;
                }
                Err(()) => {
                    remaining.apps.insert(name, entry);
                    continue;
                }
                Ok(true) => {}
            }

            let mut app_ok = true;

            // Only restore values still owned by CyNotch (ShowBanner == 0, ShowInActionCenter == 1)
            // so unrelated changes made by the user or other applications are never overwritten.
            match reg.query_app_dword(&name, SHOW_BANNER_NAME) {
                Ok(Some(0)) => {
                    let res = match entry.show_banner {
                        Some(prev) => reg.set_app_dword(&name, SHOW_BANNER_NAME, prev),
                        None => reg.delete_app_value(&name, SHOW_BANNER_NAME),
                    };
                    if res.is_err() {
                        app_ok = false;
                    }
                }
                Ok(_) => {}
                Err(()) => app_ok = false,
            }

            match reg.query_app_dword(&name, SHOW_IN_AC_NAME) {
                Ok(Some(1)) => {
                    let res = match entry.show_in_action_center {
                        Some(prev) => reg.set_app_dword(&name, SHOW_IN_AC_NAME, prev),
                        None => reg.delete_app_value(&name, SHOW_IN_AC_NAME),
                    };
                    if res.is_err() {
                        app_ok = false;
                    }
                }
                Ok(_) => {}
                Err(()) => app_ok = false,
            }

            if !app_ok {
                remaining.apps.insert(name, entry);
            }
        }

        if remaining.apps.is_empty() {
            let _ = crate::persistence::remove_with_backup(backup_path);
        } else {
            // Preserve un-restored apps for safe retry on next disable/recovery.
            let _ = crate::persistence::atomic_write_json(backup_path, &remaining, true);
        }

        count_suppressed_apps_with(reg)
    }
}

/// Snapshots per-app `ShowBanner` / `ShowInActionCenter` values under
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\*`
/// and toggles native Windows popup banners off (`enable == true`) or restores them (`enable == false`).
/// Returns the actual number of currently suppressed apps after the operation.
/// NOTE: Never invoke automatically on startup or inside unit tests.
pub fn set_exclusive_windows_banners(enable: bool) -> usize {
    let _guard = BANNER_TOGGLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let backup_path = backup_file_path();
    let mut reg = WindowsNotifRegistry;
    set_exclusive_windows_banners_with(&mut reg, &backup_path, enable)
}

/// Serialized, coalescing background worker for exclusive Windows banner toggling
/// so rapid UI clicks never spawn competing threads or race on registry/backup state.
pub fn enqueue_exclusive_windows_banners(
    enable: bool,
    on_complete: Box<dyn FnOnce(usize) + Send + 'static>,
) {
    use std::sync::{Arc, Condvar, OnceLock};
    struct PendingJob {
        enable: bool,
        callbacks: Vec<Box<dyn FnOnce(usize) + Send + 'static>>,
    }
    struct BannerWorker {
        pending: Mutex<Option<PendingJob>>,
        cvar: Condvar,
    }
    static WORKER: OnceLock<Arc<BannerWorker>> = OnceLock::new();

    let worker = WORKER.get_or_init(|| {
        let w = Arc::new(BannerWorker {
            pending: Mutex::new(None),
            cvar: Condvar::new(),
        });
        let bg = Arc::clone(&w);
        let _ = std::thread::Builder::new()
            .name("cynotch-notif-banners".to_string())
            .spawn(move || loop {
                let job = {
                    let mut guard = bg.pending.lock().unwrap_or_else(|e| e.into_inner());
                    while guard.is_none() {
                        guard = bg.cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
                    }
                    guard.take()
                };
                if let Some(job) = job {
                    let count = set_exclusive_windows_banners(job.enable);
                    for cb in job.callbacks {
                        cb(count);
                    }
                }
            });
        w
    });

    let mut guard = worker.pending.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(existing) => {
            existing.enable = enable;
            existing.callbacks.push(on_complete);
        }
        None => {
            *guard = Some(PendingJob {
                enable,
                callbacks: vec![on_complete],
            });
        }
    }
    worker.cvar.notify_one();
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
        self.poll_new_toasts_with_aumid()
            .into_iter()
            .map(|(id, app, title, sub, _)| (id, app, title, sub))
            .collect()
    }

    pub fn poll_new_toasts_with_aumid(
        &mut self,
    ) -> Vec<(u32, String, String, String, Option<String>)> {
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

            let app_info = notif.AppInfo().ok();
            let aumid = app_info
                .as_ref()
                .and_then(|ai| ai.AppUserModelId().ok())
                .map(|s| s.to_string_lossy())
                .filter(|s| !s.trim().is_empty());
            let app_name = app_info
                .as_ref()
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

            incoming.push((id, app_name, title, subtitle, aumid));
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

/// Brings the notification's source application to the foreground (or launches it via AUMID).
pub fn activate_source_app(app_name: &str, aumid: Option<&str>) -> bool {
    if let Some(id) = aumid.map(str::trim).filter(|s| !s.is_empty()) {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let need_uninit = hr == S_OK || hr == S_FALSE;
            let activated = (|| -> bool {
                let Ok(mgr) = CoCreateInstance::<_, IApplicationActivationManager>(
                    &ApplicationActivationManager,
                    None,
                    CLSCTX_LOCAL_SERVER,
                ) else {
                    return false;
                };
                mgr.ActivateApplication(&HSTRING::from(id), PCWSTR::null(), AO_NONE)
                    .is_ok()
            })();
            if need_uninit {
                CoUninitialize();
            }
            if activated {
                return true;
            }
        }
    }

    // Fallback: locate a visible top-level window matching the app name or AUMID tail.
    let needle_app = app_name.trim().to_lowercase();
    let needle_aumid = aumid
        .and_then(|s| s.rsplit(['.', '!', '\\', '/']).next())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| s.len() >= 3);

    if (needle_app.is_empty() || needle_app == "windows") && needle_aumid.is_none() {
        return false;
    }

    struct SearchCtx {
        self_pid: u32,
        needle_app: String,
        needle_aumid: Option<String>,
        found: Option<HWND>,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut SearchCtx);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == ctx.self_pid {
            return BOOL(1);
        }

        let mut title_buf = [0u16; 256];
        let title_len = GetWindowTextW(hwnd, &mut title_buf);
        if title_len <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&title_buf[..title_len as usize]).to_lowercase();

        let mut proc_stem = String::new();
        if let Ok(proc_h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut path_buf = [0u16; 512];
            let mut size = path_buf.len() as u32;
            if QueryFullProcessImageNameW(
                proc_h,
                PROCESS_NAME_FORMAT(0),
                PWSTR(path_buf.as_mut_ptr()),
                &mut size,
            )
            .is_ok()
            {
                let path_str = String::from_utf16_lossy(&path_buf[..size as usize]);
                if let Some(stem) = std::path::Path::new(&path_str)
                    .file_stem()
                    .and_then(|s| s.to_str())
                {
                    proc_stem = stem.to_lowercase();
                }
            }
            let _ = CloseHandle(proc_h);
        }

        let matches_app = !ctx.needle_app.is_empty()
            && ctx.needle_app != "windows"
            && ((!proc_stem.is_empty()
                && (proc_stem.contains(&ctx.needle_app) || ctx.needle_app.contains(&proc_stem)))
                || title.contains(&ctx.needle_app));
        let matches_aumid = ctx.needle_aumid.as_ref().is_some_and(|n| {
            (!proc_stem.is_empty() && (proc_stem.contains(n) || n.contains(&proc_stem)))
                || title.contains(n)
        });

        if matches_app || matches_aumid {
            ctx.found = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut ctx = SearchCtx {
        self_pid: unsafe { GetCurrentProcessId() },
        needle_app,
        needle_aumid,
        found: None,
    };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut SearchCtx as isize));
        if let Some(target_hwnd) = ctx.found {
            if IsIconic(target_hwnd).as_bool() {
                let _ = ShowWindow(target_hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(target_hwnd);
            return true;
        }
    }
    false
}

// Keep unused import check quiet in case PropertyValue helpers are needed elsewhere
#[allow(dead_code)]
fn _keep_winrt_ref_import(_: Option<IReference<f32>>) {}

#[cfg(test)]
mod registry_integrity_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(0);

    fn isolated_dir(tag: &str) -> PathBuf {
        let seq = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "cynotch_notif_reg_{}_{}_{}",
            tag,
            std::process::id(),
            seq
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[derive(Default)]
    struct MockRegistry {
        root: HashMap<String, u32>,
        apps: BTreeMap<String, HashMap<String, u32>>,
        fail_root_query: bool,
        fail_root_write: bool,
        fail_app_write: HashSet<String>,
    }

    impl NotifRegistry for MockRegistry {
        fn query_root_dword(&self, value_name: &str) -> Result<Option<u32>, ()> {
            if self.fail_root_query {
                return Err(());
            }
            Ok(self.root.get(value_name).copied())
        }
        fn set_root_dword(&mut self, value_name: &str, value: u32) -> Result<(), ()> {
            if self.fail_root_write {
                return Err(());
            }
            self.root.insert(value_name.to_string(), value);
            Ok(())
        }
        fn delete_root_value(&mut self, value_name: &str) -> Result<(), ()> {
            if self.fail_root_write {
                return Err(());
            }
            self.root.remove(value_name);
            Ok(())
        }
        fn enum_app_subkeys(&self) -> Result<Vec<String>, ()> {
            Ok(self.apps.keys().cloned().collect())
        }
        fn app_subkey_exists(&self, app: &str) -> Result<bool, ()> {
            Ok(self.apps.contains_key(app))
        }
        fn query_app_dword(&self, app: &str, value_name: &str) -> Result<Option<u32>, ()> {
            let m = self.apps.get(app).ok_or(())?;
            Ok(m.get(value_name).copied())
        }
        fn set_app_dword(&mut self, app: &str, value_name: &str, value: u32) -> Result<(), ()> {
            if self.fail_app_write.contains(app) {
                return Err(());
            }
            let m = self.apps.get_mut(app).ok_or(())?;
            m.insert(value_name.to_string(), value);
            Ok(())
        }
        fn delete_app_value(&mut self, app: &str, value_name: &str) -> Result<(), ()> {
            if self.fail_app_write.contains(app) {
                return Err(());
            }
            let m = self.apps.get_mut(app).ok_or(())?;
            m.remove(value_name);
            Ok(())
        }
    }

    #[test]
    fn bh11_sound_mute_aborts_if_backup_write_fails() {
        let dir = isolated_dir("bh11_fail_backup");
        // Passing a directory path as backup_path forces write failure
        let mut reg = MockRegistry::default();
        reg.root.insert(ALLOW_NOTIF_SOUND_NAME.to_string(), 1);

        let ok = sync_windows_notification_sound_mute_with(&mut reg, &dir, true);
        assert!(!ok);
        // Registry must remain untouched (1, not 0)
        assert_eq!(reg.root.get(ALLOW_NOTIF_SOUND_NAME), Some(&1));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bh11_sound_unmute_retains_backup_when_registry_restore_fails_and_retries() {
        let dir = isolated_dir("bh11_retry");
        let backup_path = dir.join("notif_sound_backup.json");
        let mut reg = MockRegistry::default();
        reg.root.insert(ALLOW_NOTIF_SOUND_NAME.to_string(), 1);

        assert!(sync_windows_notification_sound_mute_with(
            &mut reg,
            &backup_path,
            true
        ));
        assert_eq!(reg.root.get(ALLOW_NOTIF_SOUND_NAME), Some(&0));
        assert!(backup_path.exists());

        // Inject registry write failure on unmute
        reg.fail_root_write = true;
        assert!(!sync_windows_notification_sound_mute_with(
            &mut reg,
            &backup_path,
            false
        ));
        // Backup MUST be preserved so retry succeeds later
        assert!(backup_path.exists());
        assert_eq!(reg.root.get(ALLOW_NOTIF_SOUND_NAME), Some(&0));

        // Retry after failure clears
        reg.fail_root_write = false;
        assert!(sync_windows_notification_sound_mute_with(
            &mut reg,
            &backup_path,
            false
        ));
        assert_eq!(reg.root.get(ALLOW_NOTIF_SOUND_NAME), Some(&1));
        assert!(!backup_path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bh10_banners_corrupted_backup_never_mutates_registry() {
        let dir = isolated_dir("bh10_corrupt");
        let backup_path = dir.join("notif_banner_backup.json");
        std::fs::write(&backup_path, "{ broken json ").unwrap();

        let mut reg = MockRegistry::default();
        let mut app = HashMap::new();
        app.insert(SHOW_BANNER_NAME.to_string(), 1);
        reg.apps.insert("Discord".to_string(), app);

        let count = set_exclusive_windows_banners_with(&mut reg, &backup_path, true);
        assert_eq!(count, 0);
        // Discord must remain untouched!
        assert_eq!(
            reg.apps.get("Discord").unwrap().get(SHOW_BANNER_NAME),
            Some(&1)
        );
        // Corrupted backup preserved
        assert!(backup_path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bh10_banners_enable_disable_idempotent_ownership_and_partial_retry() {
        let dir = isolated_dir("bh10_lifecycle");
        let backup_path = dir.join("notif_banner_backup.json");

        let mut reg = MockRegistry::default();
        let mut discord = HashMap::new();
        discord.insert(SHOW_BANNER_NAME.to_string(), 1);
        reg.apps.insert("Discord".to_string(), discord);
        let slack = HashMap::new(); // originally absent values (None)
        reg.apps.insert("Slack".to_string(), slack);

        // Enable twice (idempotent)
        assert_eq!(
            set_exclusive_windows_banners_with(&mut reg, &backup_path, true),
            2
        );
        assert_eq!(
            set_exclusive_windows_banners_with(&mut reg, &backup_path, true),
            2
        );
        assert_eq!(reg.apps["Discord"].get(SHOW_BANNER_NAME), Some(&0));
        assert_eq!(reg.apps["Slack"].get(SHOW_BANNER_NAME), Some(&0));

        // Simulate user manually re-enabling ShowBanner on Discord via Windows Settings
        reg.apps
            .get_mut("Discord")
            .unwrap()
            .insert(SHOW_BANNER_NAME.to_string(), 1);
        // Inject write failure on Slack during first disable attempt
        reg.fail_app_write.insert("Slack".to_string());

        let remaining_suppressed =
            set_exclusive_windows_banners_with(&mut reg, &backup_path, false);
        assert_eq!(remaining_suppressed, 1); // Slack still suppressed
        assert!(backup_path.exists()); // Retained for Slack retry
        // Discord's external manual setting (1) was preserved
        assert_eq!(reg.apps["Discord"].get(SHOW_BANNER_NAME), Some(&1));

        // Clear fault and retry disable -> Slack restores to None and backup is removed
        reg.fail_app_write.clear();
        let after_retry = set_exclusive_windows_banners_with(&mut reg, &backup_path, false);
        assert_eq!(after_retry, 0);
        assert_eq!(reg.apps["Slack"].get(SHOW_BANNER_NAME), None);
        assert_eq!(reg.apps["Slack"].get(SHOW_IN_AC_NAME), None);
        assert!(!backup_path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
