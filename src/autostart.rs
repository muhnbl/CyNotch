use windows::core::*;
use windows::Win32::System::Registry::*;

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
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_SET_VALUE, &mut hkey).is_ok() {
            let value_name = w!("CyNotch");
            if enable {
                if let Ok(exe_path) = std::env::current_exe() {
                    let path_str = exe_path.to_string_lossy();
                    let wide_path: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
                    let byte_slice = std::slice::from_raw_parts(
                        wide_path.as_ptr() as *const u8,
                        wide_path.len() * 2,
                    );
                    let _ = RegSetValueExW(hkey, value_name, 0, REG_SZ, Some(byte_slice));
                }
            } else {
                let _ = RegDeleteValueW(hkey, value_name);
            }
            let _ = RegCloseKey(hkey);
            true
        } else {
            false
        }
    }
}
