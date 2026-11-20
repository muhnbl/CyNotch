use crate::localtime::{format_ymd, local_now};
use crate::models::ScreenTimeStats;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub static RESET_SCREENTIME_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize)]
struct SavedScreenTimeV2 {
    #[serde(default)]
    current_date: String,
    #[serde(default)]
    app_seconds: HashMap<String, u64>,
    #[serde(default)]
    total_seconds: u64,
    #[serde(default = "default_hours")]
    hourly_seconds: Vec<u64>,
    #[serde(default)]
    daily_totals: HashMap<String, u64>,
}

fn default_hours() -> Vec<u64> {
    vec![0u64; 24]
}

#[repr(C)]
struct LastInputInfo {
    cb_size: u32,
    dw_time: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(hwnd: *mut c_void, lpdw_process_id: *mut u32) -> u32;
    fn GetLastInputInfo(plii: *mut LastInputInfo) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(
        dw_desired_access: u32,
        b_inherit_handle: i32,
        dw_process_id: u32,
    ) -> *mut c_void;
    fn QueryFullProcessImageNameW(
        h_process: *mut c_void,
        dw_flags: u32,
        lp_exe_name: *mut u16,
        lpdw_size: *mut u32,
    ) -> i32;
    fn CloseHandle(h_object: *mut c_void) -> i32;
    fn GetTickCount() -> u32;
}

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const IDLE_THRESHOLD_MS: u32 = 300_000; // Pause Screen Time after 5 min AFK

pub fn is_user_active_recently() -> bool {
    unsafe {
        let mut lii = LastInputInfo {
            cb_size: std::mem::size_of::<LastInputInfo>() as u32,
            dw_time: 0,
        };
        if GetLastInputInfo(&mut lii) != 0 {
            let now = GetTickCount();
            now.wrapping_sub(lii.dw_time) < IDLE_THRESHOLD_MS
        } else {
            true
        }
    }
}

pub fn format_hour_label(hour: usize) -> String {
    match hour % 24 {
        0 => "12 AM".to_string(),
        1..=11 => format!("{} AM", hour),
        12 => "12 PM".to_string(),
        h => format!("{} PM", h - 12),
    }
}

pub fn compute_peak_hour(hourly: &[u64; 24], fallback_hour: usize) -> (usize, String) {
    let mut best_h = fallback_hour.min(23);
    let mut best_val = 0u64;
    for (h, &v) in hourly.iter().enumerate() {
        if v > best_val {
            best_val = v;
            best_h = h;
        }
    }
    if best_val == 0 {
        (fallback_hour.min(23), "--".to_string())
    } else {
        (best_h, format_hour_label(best_h))
    }
}

pub fn compute_seven_day_avg(
    daily_totals: &HashMap<String, u64>,
    today_date: &str,
    today_secs: u64,
) -> (u64, usize) {
    let mut vals: Vec<(&str, u64)> = daily_totals
        .iter()
        .filter(|(d, _)| d.as_str() != today_date)
        .map(|(d, &v)| (d.as_str(), v))
        .collect();
    vals.sort_by(|a, b| b.0.cmp(a.0));
    vals.truncate(6);
    let mut sum = today_secs;
    let mut count = 1usize;
    for (_, v) in vals {
        sum += v;
        count += 1;
    }
    (sum / count as u64, count)
}

#[derive(Serialize)]
struct SaveScreenTimeRef<'a> {
    current_date: &'a str,
    app_seconds: &'a HashMap<String, u64>,
    total_seconds: u64,
    hourly_seconds: &'a [u64],
    daily_totals: &'a HashMap<String, u64>,
}

pub struct ScreenTimeTracker {
    storage_path: PathBuf,
    current_date: String,
    last_ymd: (i32, u32, u32),
    app_seconds: HashMap<String, u64>,
    total_seconds: u64,
    hourly_seconds: [u64; 24],
    daily_totals: HashMap<String, u64>,
}

impl ScreenTimeTracker {
    fn appdata_storage_path() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join("CyNotch").join("screentime.json")
    }

    pub fn new() -> Self {
            let now = local_now();
            let ymd = (now.year, now.month, now.day);
            let today = format_ymd(now);
            let path = Self::appdata_storage_path();

        if let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(saved) = serde_json::from_str::<SavedScreenTimeV2>(&content)
        {
            let mut hours = [0u64; 24];
            for (i, v) in saved.hourly_seconds.iter().take(24).enumerate() {
                hours[i] = *v;
            }
            let mut tracker = Self {
                storage_path: path,
                current_date: if saved.current_date.is_empty() {
                    today.clone()
                } else {
                    saved.current_date
                },
                last_ymd: ymd,
                app_seconds: saved.app_seconds,
                total_seconds: saved.total_seconds,
                hourly_seconds: hours,
                daily_totals: saved.daily_totals,
            };
            tracker.rollover_if_needed(&today);
            return tracker;
        }

        // Start clean at 0 — accumulate purely from live active window tracking
        Self {
            storage_path: path,
            current_date: today,
            last_ymd: ymd,
            app_seconds: HashMap::new(),
            total_seconds: 0,
            hourly_seconds: [0u64; 24],
            daily_totals: HashMap::new(),
        }
    }

    fn rollover_if_needed(&mut self, today: &str) {
        if self.current_date != today {
            if !self.current_date.is_empty() && self.total_seconds > 0 {
                self.daily_totals
                    .insert(self.current_date.clone(), self.total_seconds);
            }
            self.current_date = today.to_string();
            self.app_seconds.clear();
            self.total_seconds = 0;
            self.hourly_seconds = [0u64; 24];
            self.save();
        }
    }

    pub fn save(&self) {
        if self.total_seconds == 0 && self.daily_totals.is_empty() && !self.storage_path.exists() {
            return;
        }
        if let Some(parent) = self.storage_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let saved = SaveScreenTimeRef {
            current_date: &self.current_date,
            app_seconds: &self.app_seconds,
            total_seconds: self.total_seconds,
            hourly_seconds: &self.hourly_seconds,
            daily_totals: &self.daily_totals,
        };
        if let Ok(json) = serde_json::to_string_pretty(&saved) {
            let _ = std::fs::write(&self.storage_path, json);
        }
    }

        pub fn reset_today(&mut self) {
            let now = local_now();
            self.last_ymd = (now.year, now.month, now.day);
            self.current_date = format_ymd(now);
            self.app_seconds.clear();
            self.total_seconds = 0;
            self.hourly_seconds = [0u64; 24];
            self.save();
        }

        fn should_periodic_save(total_seconds: u64) -> bool {
            total_seconds == 1 || total_seconds % 60 == 0
        }

        pub fn tick_quiet(&mut self, include_top_apps: bool) -> ScreenTimeStats {
            let now = local_now();
            let ymd = (now.year, now.month, now.day);
            if ymd != self.last_ymd {
                self.last_ymd = ymd;
                let today = format_ymd(now);
                self.rollover_if_needed(&today);
            }
            let hour = (now.hour as usize).min(23);

            if is_user_active_recently()
                && let Some(app) = get_active_app_name()
            {
                self.total_seconds += 1;
                self.hourly_seconds[hour] += 1;
                *self.app_seconds.entry(app).or_insert(0) += 1;

                if Self::should_periodic_save(self.total_seconds) {
                    self.save();
                }
            }

            if include_top_apps {
                self.stats(hour)
            } else {
                ScreenTimeStats {
                    today_secs: self.total_seconds,
                    top_app_name: "--".to_string(),
                    top_app_secs: 0,
                    top_apps: vec![],
                    seven_day_avg_secs: 0,
                    tracked_days: 0,
                    peak_hour: hour,
                    peak_hour_str: "--".to_string(),
                    hourly_secs: self.hourly_seconds,
                }
            }
        }

        pub fn tick(&mut self) -> ScreenTimeStats {
            self.tick_quiet(true)
        }

    pub fn stats(&self, current_hour: usize) -> ScreenTimeStats {
        let mut sorted: Vec<(&str, u64)> = self
            .app_seconds
            .iter()
            .map(|(k, &v)| (k.as_str(), v))
            .collect();
        sorted.sort_by_key(|a| std::cmp::Reverse(a.1));

        let (top_app_name, top_app_secs) = sorted
            .first()
            .map(|(k, v)| ((*k).to_string(), *v))
            .unwrap_or_else(|| ("--".to_string(), 0));
        let top_apps = sorted
            .into_iter()
            .take(3)
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        let (peak_hour, peak_hour_str) = compute_peak_hour(&self.hourly_seconds, current_hour);
        let (seven_day_avg_secs, tracked_days) =
            compute_seven_day_avg(&self.daily_totals, &self.current_date, self.total_seconds);

        ScreenTimeStats {
            today_secs: self.total_seconds,
            top_app_name,
            top_app_secs,
            top_apps,
            seven_day_avg_secs,
            tracked_days,
            peak_hour,
            peak_hour_str,
            hourly_secs: self.hourly_seconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_periodic_save() {
        assert!(ScreenTimeTracker::should_periodic_save(1));
        assert!(ScreenTimeTracker::should_periodic_save(60));
        assert!(ScreenTimeTracker::should_periodic_save(120));
        assert!(!ScreenTimeTracker::should_periodic_save(15));
        assert!(!ScreenTimeTracker::should_periodic_save(30));
        assert!(!ScreenTimeTracker::should_periodic_save(45));
    }
}

fn title_case_stem(stem: &str) -> String {
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn get_active_app_name() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }

        let mut buf: [u16; 1024] = [0; 1024];
        let mut size = buf.len() as u32;

        let res = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(handle);

        if res != 0 && size > 0 {
            let slice = &buf[..size as usize];
            let fname_start = slice
                .iter()
                .rposition(|&c| c == b'\\' as u16 || c == b'/' as u16)
                .map(|idx| idx + 1)
                .unwrap_or(0);
            let file_name = String::from_utf16_lossy(&slice[fname_start..]);

            let lower = file_name.to_lowercase();
            if matches!(lower.as_str(), "lockapp.exe" | "logonui.exe" | "idle") || lower.is_empty()
            {
                return None;
            }
            let clean_name = match lower.as_str() {
                "chrome.exe" => "Chrome",
                "msedge.exe" => "Edge",
                "firefox.exe" => "Firefox",
                "brave.exe" => "Brave",
                "zen.exe" => "Zen",
                "code.exe" => "VS Code",
                "windowsterminal.exe" => "Terminal",
                "powershell.exe" | "pwsh.exe" => "PowerShell",
                "spotify.exe" => "Spotify",
                "discord.exe" => "Discord",
                "explorer.exe" => "Explorer",
                "cynotch.exe" => "CyNotch",
                _ => {
                    let stem = Path::new(&file_name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&file_name);
                    return Some(title_case_stem(stem));
                }
            };
            return Some(clean_name.to_string());
        }

        None
    }
}
