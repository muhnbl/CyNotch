use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
unsafe extern "system" {
    fn GetLocalTime(lpSystemTime: *mut SYSTEMTIME);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millis: u32,
}

pub fn local_now() -> LocalTime {
    unsafe {
        let mut st = SYSTEMTIME::default();
        GetLocalTime(&mut st);
        LocalTime {
            year: st.wYear as i32,
            month: st.wMonth as u32,
            day: st.wDay as u32,
            hour: st.wHour as u32,
            minute: st.wMinute as u32,
            second: st.wSecond as u32,
            millis: st.wMilliseconds as u32,
        }
    }
}

pub fn format_capture_stamp(lt: LocalTime) -> String {
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}_{:03}",
        lt.year, lt.month, lt.day, lt.hour, lt.minute, lt.second, lt.millis
    )
}

pub fn format_ymd(lt: LocalTime) -> String {
    format!("{:04}-{:02}-{:02}", lt.year, lt.month, lt.day)
}

pub fn format_hhmm(lt: LocalTime) -> String {
    format!("{:02}:{:02}", lt.hour, lt.minute)
}

pub fn format_rfc3339(lt: LocalTime) -> String {
    let offset = get_utc_offset_minutes();
    let sign = if offset >= 0 { '+' } else { '-' };
    let abs_offset = offset.abs();
    let offset_hours = abs_offset / 60;
    let offset_mins = abs_offset % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
        lt.year, lt.month, lt.day, lt.hour, lt.minute, lt.second, sign, offset_hours, offset_mins
    )
}

fn get_utc_offset_minutes() -> i32 {
    unsafe {
        let mut tzi: TIME_ZONE_INFORMATION = TIME_ZONE_INFORMATION::default();
        let result = GetTimeZoneInformation(&mut tzi);
        if result == 0xFFFFFFFF {
            return 0;
        }
        // Bias is in minutes, positive = west of UTC (UTC = local + bias)
        // RFC3339 offset is local = UTC + offset, so offset = -bias
        // Also need to account for daylight bias if in DST
        let bias = tzi.Bias as i32;
        let daylight_bias = tzi.DaylightBias as i32;
        // Standard bias applies when not in DST
        // The result tells us if we're in daylight time
        let in_daylight = matches!(result, 2); // TIME_ZONE_ID_DAYLIGHT
        let total_bias = if in_daylight { bias + daylight_bias } else { bias };
        -total_bias
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_capture_stamp() {
        let lt = LocalTime {
            year: 2026,
            month: 11,
            day: 19,
            hour: 14,
            minute: 30,
            second: 45,
            millis: 123,
        };
        assert_eq!(format_capture_stamp(lt), "20261119_143045_123");
    }

    #[test]
    fn test_format_ymd() {
        let lt = LocalTime {
            year: 2026,
            month: 11,
            day: 19,
            hour: 14,
            minute: 30,
            second: 45,
            millis: 123,
        };
        assert_eq!(format_ymd(lt), "2026-11-19");
    }

    #[test]
    fn test_format_hhmm() {
        let lt = LocalTime {
            year: 2026,
            month: 11,
            day: 19,
            hour: 14,
            minute: 30,
            second: 45,
            millis: 123,
        };
        assert_eq!(format_hhmm(lt), "14:30");
        
        let lt2 = LocalTime {
            year: 2026,
            month: 1,
            day: 1,
            hour: 0,
            minute: 5,
            second: 0,
            millis: 0,
        };
        assert_eq!(format_hhmm(lt2), "00:05");
    }

    #[test]
    fn test_format_rfc3339() {
        let lt = LocalTime {
            year: 2026,
            month: 11,
            day: 19,
            hour: 14,
            minute: 30,
            second: 45,
            millis: 123,
        };
        let result = format_rfc3339(lt);
        // Should match RFC3339 format: YYYY-MM-DDTHH:MM:SS±HH:MM
        assert!(result.len() >= 20);
        assert!(result.chars().nth(4) == Some('-'));
        assert!(result.chars().nth(7) == Some('-'));
        assert!(result.chars().nth(10) == Some('T'));
        assert!(result.chars().nth(13) == Some(':'));
        assert!(result.chars().nth(16) == Some(':'));
        // Check for timezone offset
        assert!(result.contains('+') || result.contains('-'));
    }

    #[test]
    fn test_local_now_structure() {
        let now = local_now();
        assert!(now.year >= 2020);
        assert!(now.month >= 1 && now.month <= 12);
        assert!(now.day >= 1 && now.day <= 31);
        assert!(now.hour <= 23);
        assert!(now.minute <= 59);
        assert!(now.second <= 59);
        assert!(now.millis <= 999);
    }
}