use crate::memreduct::{MemoryPoolStats, query_memory_pools};
use crate::models::HardwareTelemetry;
use std::ffi::c_void;
use std::time::Instant;
use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

#[repr(C)]
#[derive(Default)]
struct SystemPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    system_status_flag: u8,
    battery_life_time: u32,
    battery_full_life_time: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FileTime {
    dw_low_date_time: u32,
    dw_high_date_time: u32,
}

impl FileTime {
    #[inline]
    fn as_u64(self) -> u64 {
        ((self.dw_high_date_time as u64) << 32) | (self.dw_low_date_time as u64)
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemPowerStatus(lp_system_power_status: *mut SystemPowerStatus) -> i32;
    fn GetSystemTimes(
        lp_idle_time: *mut FileTime,
        lp_kernel_time: *mut FileTime,
        lp_user_time: *mut FileTime,
    ) -> i32;
}

pub fn poll_power_status() -> (bool, Option<u8>) {
    unsafe {
        let mut sps = SystemPowerStatus::default();
        if GetSystemPowerStatus(&mut sps) != 0 {
            let on_ac = sps.ac_line_status == 1;
            // 128 = no system battery (desktop PC), 255 = unknown
            let pct = if (sps.battery_flag & 128) != 0 || sps.battery_life_percent > 100 {
                None
            } else {
                Some(sps.battery_life_percent)
            };
            (on_ac, pct)
        } else {
            (true, None)
        }
    }
}

/// Fast initial snapshot of real Windows Physical RAM, Pagefile, System Cache & Power status
/// so UI frame 0 never displays 0% RAM or placeholder numbers.
pub fn poll_memory_and_power() -> (MemoryPoolStats, bool, Option<u8>) {
    let pools = query_memory_pools();
    let (on_ac, battery_pct) = poll_power_status();
    (pools, on_ac, battery_pct)
}

#[inline]
fn sample_cpu_times() -> (u64, u64, u64) {
    unsafe {
        let mut idle = FileTime::default();
        let mut kernel = FileTime::default();
        let mut user = FileTime::default();
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
            (idle.as_u64(), kernel.as_u64(), user.as_u64())
        } else {
            (0, 0, 0)
        }
    }
}

/// Queries live aggregate physical NIC throughput counters via Windows IpHelper (`GetIfTable2`),
/// filtering out loopback (24), tunnel (131), offline adapters, and NDIS filter pseudo-interfaces.
fn sample_network_octets() -> (u64, u64) {
    unsafe {
        let mut table_ptr: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table_ptr) != NO_ERROR || table_ptr.is_null() {
            return (0, 0);
        }
        let table = &*table_ptr;
        let count = table.NumEntries as usize;
        let rows = std::slice::from_raw_parts(table.Table.as_ptr(), count);

        let mut total_in: u64 = 0;
        let mut total_out: u64 = 0;
        for row in rows {
            if row.OperStatus != IfOperStatusUp {
                continue;
            }
            // Skip software loopback (24) and virtual tunnel interfaces (131)
            if row.Type == 24 || row.Type == 131 {
                continue;
            }
            // Bit 2 (0x04) in MIB_IF_ROW2.InterfaceAndOperStatusFlags is FilterInterface
            // (Skip NDIS lightweight filter pseudo-interfaces to prevent double-counting)
            if (row.InterfaceAndOperStatusFlags._bitfield & 0x04) != 0 {
                continue;
            }
            // Bit 0 (0x01) is HardwareInterface; prefer hardware adapters when available,
            // falling back to all non-loopback non-filter operational links (e.g. VPN TUN).
            if row.Type == 1 || row.Type == 53 {
                continue;
            }
            total_in = total_in.saturating_add(row.InOctets);
            total_out = total_out.saturating_add(row.OutOctets);
        }
        FreeMibTable(table_ptr as *const c_void);
        (total_in, total_out)
    }
}

/// Compact speed readout for narrow idle islands: `0K`, `142K`, `1.4M`.
pub fn format_speed_short(kbps: f32) -> String {
    let v = kbps.max(0.0);
    if v >= 1000.0 {
        let mbps = v / 1024.0;
        if mbps >= 10.0 {
            format!("{:.0}M", mbps)
        } else {
            format!("{:.1}M", mbps)
        }
    } else if v >= 1.0 {
        format!("{:.0}K", v)
    } else {
        "0K".to_string()
    }
}

/// Detailed speed readout for expanded bento cards: `0 KB/s`, `142 KB/s`, `1.4 MB/s`.
pub fn format_speed_long(kbps: f32) -> String {
    let v = kbps.max(0.0);
    if v >= 1000.0 {
        let mbps = v / 1024.0;
        if mbps >= 10.0 {
            format!("{:.0} MB/s", mbps)
        } else {
            format!("{:.1} MB/s", mbps)
        }
    } else {
        format!("{:.0} KB/s", v)
    }
}

pub struct TelemetryCollector {
    prev_idle: u64,
    prev_kernel: u64,
    prev_user: u64,
    prev_in_octets: u64,
    prev_out_octets: u64,
    last_net_at: Instant,
}

impl TelemetryCollector {
    pub fn new() -> Self {
        let (idle, kernel, user) = sample_cpu_times();
        let (in_oct, out_oct) = sample_network_octets();
        Self {
            prev_idle: idle,
            prev_kernel: kernel,
            prev_user: user,
            prev_in_octets: in_oct,
            prev_out_octets: out_oct,
            last_net_at: Instant::now(),
        }
    }

    pub fn poll(&mut self) -> HardwareTelemetry {
        let (idle, kernel, user) = sample_cpu_times();
        let idle_diff = idle.saturating_sub(self.prev_idle);
        let kernel_diff = kernel.saturating_sub(self.prev_kernel);
        let user_diff = user.saturating_sub(self.prev_user);
        self.prev_idle = idle;
        self.prev_kernel = kernel;
        self.prev_user = user;

        // Note: on Windows GetSystemTimes, kernel_time already includes idle_time.
        let total = kernel_diff.saturating_add(user_diff);
        let cpu = if total > 0 {
            let busy = total.saturating_sub(idle_diff);
            ((busy as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as f32
        } else {
            0.0
        };

        let now = Instant::now();
        let dt_s = now.duration_since(self.last_net_at).as_secs_f64();
        let (in_oct, out_oct) = sample_network_octets();
        let (down_kbps, up_kbps) = if dt_s >= 0.05 && self.prev_in_octets > 0 {
            let d_in = in_oct.saturating_sub(self.prev_in_octets);
            let d_out = out_oct.saturating_sub(self.prev_out_octets);
            (
                (d_in as f64 / 1024.0 / dt_s).clamp(0.0, 9_999_999.0) as f32,
                (d_out as f64 / 1024.0 / dt_s).clamp(0.0, 9_999_999.0) as f32,
            )
        } else {
            (0.0, 0.0)
        };
        self.prev_in_octets = in_oct;
        self.prev_out_octets = out_oct;
        self.last_net_at = now;

        let (pools, on_ac, battery_pct) = poll_memory_and_power();

        HardwareTelemetry {
            cpu_usage: cpu,
            memory_used_gb: pools.phys_used_gb,
            memory_total_gb: pools.phys_total_gb,
            memory_percent: pools.phys_percent,
            pagefile_used_gb: pools.page_used_gb,
            pagefile_total_gb: pools.page_total_gb,
            pagefile_percent: pools.page_percent,
            sys_cache_mb: pools.sys_cache_mb,
            sys_cache_peak_mb: pools.sys_cache_peak_mb,
            sys_cache_percent: pools.sys_cache_percent,
            net_down_kbps: down_kbps,
            net_up_kbps: up_kbps,
            has_admin_privileges: pools.is_elevated,
            on_ac,
            battery_pct,
        }
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}
