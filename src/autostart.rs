use std::path::Path;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::*;
use windows::core::*;

/// Formats and validates a Windows `Run` registry command line for `exe_path`.
/// Always wraps the path in double quotes (mitigating CWE-428 unquoted paths)
/// and rejects empty paths or paths containing NUL, quotes, or control characters.
pub fn format_autostart_command(exe_path: &Path) -> Option<String> {
    let raw = exe_path.to_str()?;
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.contains('"')
        || trimmed.chars().any(|c| c.is_control())
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
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_SET_VALUE, &mut hkey) != ERROR_SUCCESS {
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
    fn bh13_rejects_empty_quotes_and_control_chars() {
        assert!(format_autostart_command(Path::new("")).is_none());
        assert!(format_autostart_command(Path::new("C:\\foo\"bar\\cynotch.exe")).is_none());
        assert!(format_autostart_command(Path::new("C:\\foo\nbar\\cynotch.exe")).is_none());
        assert!(format_autostart_command(Path::new("C:\\foo\0bar\\cynotch.exe")).is_none());
    }
}
