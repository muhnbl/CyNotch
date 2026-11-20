use std::path::Path;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::*;
use windows::core::*;

/// Formats and validates a Windows `Run` registry command line for `exe_path` (BH-13, BH-39).
/// Requires an absolute local path (rejecting relative, UNC, verbatim `\\?\`, trailing backslash,
/// quotes, and control characters) and wraps it in double quotes.
pub fn format_autostart_command(exe_path: &Path) -> Option<String> {
    let raw = exe_path.to_str()?;
    let trimmed = raw.trim();
    let bytes = trimmed.as_bytes();
    if trimmed.is_empty()
        || !exe_path.is_absolute()
        || trimmed.contains('"')
        || trimmed.chars().any(|c| c.is_control())
        || trimmed.ends_with('\\')
        || trimmed.ends_with('/')
    {
        return None;
    }
    // Reject UNC (`\\`, `//`, `\/`, `/\`) and verbatim `\\?\` / device `\\.\` paths.
    if bytes.len() >= 2
        && (bytes[0] == b'\\' || bytes[0] == b'/')
        && (bytes[1] == b'\\' || bytes[1] == b'/')
    {
        return None;
    }
    Some(format!("\"{}\"", trimmed))
}

pub fn is_autostart_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_READ, &mut hkey).is_ok() {
            let value_name = w!("CyNotch");
            let mut val_type = REG_VALUE_TYPE::default();
            let mut data_size = 0u32;
            let res = RegQueryValueExW(
                hkey,
                value_name,
                None,
                Some(&mut val_type),
                None,
                Some(&mut data_size),
            );
            let _ = RegCloseKey(hkey);
            res.is_ok()
        } else {
            false
        }
    }
}

pub fn set_autostart(enable: bool) -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        if enable {
            // BH-39: Use RegCreateKeyExW so fresh profiles without `Run` key succeed.
            if RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey,
                0,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut hkey,
                None,
            ) != ERROR_SUCCESS
            {
                return false;
            }
        } else if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_SET_VALUE, &mut hkey)
            != ERROR_SUCCESS
        {
            return false;
        }
        let value_name = w!("CyNotch");
        let ok = if enable {
            if let Ok(exe_path) = std::env::current_exe()
                && let Some(quoted_cmd) = format_autostart_command(&exe_path)
            {
                let wide_path: Vec<u16> = quoted_cmd
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let byte_slice = std::slice::from_raw_parts(
                    wide_path.as_ptr() as *const u8,
                    wide_path.len() * 2,
                );
                RegSetValueExW(hkey, value_name, 0, REG_SZ, Some(byte_slice)) == ERROR_SUCCESS
            } else {
                false
            }
        } else {
            let st = RegDeleteValueW(hkey, value_name);
            st == ERROR_SUCCESS || st == ERROR_FILE_NOT_FOUND
        };
        let _ = RegCloseKey(hkey);
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bh13_quotes_paths_with_spaces_and_special_chars() {
        let p = PathBuf::from(r"C:\Program Files\CyNotch [0.3.0]\cynotch.exe");
        assert_eq!(
            format_autostart_command(&p).as_deref(),
            Some("\"C:\\Program Files\\CyNotch [0.3.0]\\cynotch.exe\"")
        );
    }

    #[test]
    fn bh13_bh39_rejects_empty_relative_unc_trailing_slash_quotes_and_control_chars() {
        assert!(format_autostart_command(Path::new("")).is_none());
        assert!(format_autostart_command(Path::new("cynotch.exe")).is_none());
        assert!(format_autostart_command(Path::new(r"\\attacker\share\cynotch.exe")).is_none());
        assert!(format_autostart_command(Path::new(r"\\?\C:\CyNotch\cynotch.exe")).is_none());
        assert!(format_autostart_command(Path::new(r"C:\CyNotch\")).is_none());
        assert!(format_autostart_command(Path::new("C:\\foo\"bar\\cynotch.exe")).is_none());
        assert!(format_autostart_command(Path::new("C:\\foo\nbar\\cynotch.exe")).is_none());
        assert!(format_autostart_command(Path::new("C:\\foo\0bar\\cynotch.exe")).is_none());
    }
}
