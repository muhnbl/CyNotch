use std::ffi::c_void;
use std::mem::size_of;
use std::sync::OnceLock;

#[repr(C)]
#[derive(Debug, Default)]
pub struct MemoryStatusEx {
    pub dw_length: u32,
    pub dw_memory_load: u32,
    pub ull_total_phys: u64,
    pub ull_avail_phys: u64,
    pub ull_total_page_file: u64,
    pub ull_avail_page_file: u64,
    pub ull_total_virtual: u64,
    pub ull_avail_virtual: u64,
    pub ull_avail_extended_virtual: u64,
}

#[repr(C)]
#[derive(Default)]
struct SystemFileCacheInfo {
    current_size: usize,
    peak_size: usize,
    page_fault_count: u32,
    minimum_working_set: usize,
    maximum_working_set: usize,
    current_size_including_transition: usize,
    peak_size_including_transition: usize,
    transition_re_purpose_count: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Default)]
struct MemoryCombineInformationEx {
    handle: usize,
    pages_combined: usize,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Luid {
    low_part: u32,
    high_part: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LuidAndAttributes {
    luid: Luid,
    attributes: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TokenPrivilegesOne {
    privilege_count: u32,
    privileges: [LuidAndAttributes; 1],
}

const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
const PROCESS_SET_QUOTA: u32 = 0x0100;
const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
const TOKEN_QUERY: u32 = 0x0008;
const SE_PRIVILEGE_ENABLED: u32 = 0x0000_0002;
const TOKEN_ELEVATION_CLASS: u32 = 20;

const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_SHARE_READ_WRITE: u32 = 0x0000_0001 | 0x0000_0002;
const OPEN_EXISTING: u32 = 3;
const DRIVE_FIXED: u32 = 3;
const INVALID_HANDLE_VAL: isize = -1;

const SYS_FILE_CACHE_INFO: u32 = 21;
const SYS_MEMORY_LIST_INFO: u32 = 80;
const SYS_COMBINE_PHYSICAL_MEMORY_INFO: u32 = 130;
const SYS_REGISTRY_RECONCILIATION_INFO: u32 = 155;

const CMD_EMPTY_WORKING_SETS: u32 = 2;
const CMD_FLUSH_MODIFIED_LIST: u32 = 3;
const CMD_PURGE_STANDBY_LIST: u32 = 4;
const CMD_PURGE_LOW_PRIORITY_STANDBY_LIST: u32 = 5;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
    fn K32EnumProcesses(lpid_process: *mut u32, cb: u32, lpcb_needed: *mut u32) -> i32;
    fn OpenProcess(
        dw_desired_access: u32,
        b_inherit_handle: i32,
        dw_process_id: u32,
    ) -> *mut c_void;
    fn K32EmptyWorkingSet(h_process: *mut c_void) -> i32;
    fn CloseHandle(h_object: *mut c_void) -> i32;
    fn SetProcessWorkingSetSize(
        h_process: *mut c_void,
        dw_minimum_working_set_size: usize,
        dw_maximum_working_set_size: usize,
    ) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
    fn GetLogicalDriveStringsW(n_buffer_length: u32, lp_buffer: *mut u16) -> u32;
    fn GetDriveTypeW(lp_root_path_name: *const u16) -> u32;
    fn CreateFileW(
        lp_file_name: *const u16,
        dw_desired_access: u32,
        dw_share_mode: u32,
        lp_security_attributes: *const c_void,
        dw_creation_disposition: u32,
        dw_flags_and_attributes: u32,
        h_template_file: *mut c_void,
    ) -> *mut c_void;
    fn FlushFileBuffers(h_file: *mut c_void) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(
        process_handle: *mut c_void,
        desired_access: u32,
        token_handle: *mut *mut c_void,
    ) -> i32;
    fn LookupPrivilegeValueW(
        lp_system_name: *const u16,
        lp_name: *const u16,
        lp_luid: *mut Luid,
    ) -> i32;
    fn AdjustTokenPrivileges(
        token_handle: *mut c_void,
        disable_all_privileges: i32,
        new_state: *const TokenPrivilegesOne,
        buffer_length: u32,
        previous_state: *mut c_void,
        return_length: *mut u32,
    ) -> i32;
    fn GetTokenInformation(
        token_handle: *mut c_void,
        token_information_class: u32,
        token_information: *mut c_void,
        token_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(
        system_information_class: u32,
        system_information: *mut c_void,
        system_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
    fn NtSetSystemInformation(
        system_information_class: u32,
        system_information: *const c_void,
        system_information_length: u32,
    ) -> i32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanAreas {
    pub working_sets: bool,
    pub system_file_cache: bool,
    pub modified_page_list: bool,
    pub standby_list: bool,
    pub standby_list_low: bool,
    pub modified_file_cache: bool,
    pub registry_cache: bool,
    pub combine_memory_lists: bool,
}

impl Default for CleanAreas {
    fn default() -> Self {
        // Matches Henry++ Mem Reduct ReductMask2 = 0x0F default
        Self {
            working_sets: true,
            system_file_cache: true,
            modified_page_list: true,
            standby_list: true,
            standby_list_low: false,
            modified_file_cache: false,
            registry_cache: false,
            combine_memory_lists: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryPoolStats {
    pub phys_used_gb: f32,
    pub phys_total_gb: f32,
    pub phys_percent: f32,
    pub page_used_gb: f32,
    pub page_total_gb: f32,
    pub page_percent: f32,
    pub sys_cache_mb: f32,
    pub sys_cache_peak_mb: f32,
    pub sys_cache_percent: f32,
    pub is_elevated: bool,
}

#[derive(Debug, Clone)]
pub struct CleanResult {
    pub freed_mb: f64,
    pub old_load_pct: u32,
    pub new_load_pct: u32,
    pub trimmed_processes: usize,
    pub areas_attempted: usize,
    pub flushed_volumes: usize,
    pub kernel_caches_purged: bool,
    pub is_elevated: bool,
}

static ELEVATION_CACHE: OnceLock<bool> = OnceLock::new();

pub fn is_process_elevated() -> bool {
    *ELEVATION_CACHE.get_or_init(|| unsafe {
        let mut token: *mut c_void = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 || token.is_null() {
            return false;
        }
        let mut elevated: u32 = 0;
        let mut ret_len: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TOKEN_ELEVATION_CLASS,
            &mut elevated as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elevated != 0
    })
}

pub fn enable_memory_privileges() {
    unsafe {
        let mut token: *mut c_void = std::ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        ) == 0
            || token.is_null()
        {
            return;
        }

        let privs: [&[u16]; 2] = [
            // SeProfileSingleProcessPrivilege
            &[
                b'S' as u16,
                b'e' as u16,
                b'P' as u16,
                b'r' as u16,
                b'o' as u16,
                b'f' as u16,
                b'i' as u16,
                b'l' as u16,
                b'e' as u16,
                b'S' as u16,
                b'i' as u16,
                b'n' as u16,
                b'g' as u16,
                b'l' as u16,
                b'e' as u16,
                b'P' as u16,
                b'r' as u16,
                b'o' as u16,
                b'c' as u16,
                b'e' as u16,
                b's' as u16,
                b's' as u16,
                b'P' as u16,
                b'r' as u16,
                b'i' as u16,
                b'v' as u16,
                b'i' as u16,
                b'l' as u16,
                b'e' as u16,
                b'g' as u16,
                b'e' as u16,
                0,
            ],
            // SeIncreaseQuotaPrivilege
            &[
                b'S' as u16,
                b'e' as u16,
                b'I' as u16,
                b'n' as u16,
                b'c' as u16,
                b'r' as u16,
                b'e' as u16,
                b'a' as u16,
                b's' as u16,
                b'e' as u16,
                b'Q' as u16,
                b'u' as u16,
                b'o' as u16,
                b't' as u16,
                b'a' as u16,
                b'P' as u16,
                b'r' as u16,
                b'i' as u16,
                b'v' as u16,
                b'i' as u16,
                b'l' as u16,
                b'e' as u16,
                b'g' as u16,
                b'e' as u16,
                0,
            ],
        ];

        for name in privs {
            let mut luid = Luid::default();
            if LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) != 0 {
                let tp = TokenPrivilegesOne {
                    privilege_count: 1,
                    privileges: [LuidAndAttributes {
                        luid,
                        attributes: SE_PRIVILEGE_ENABLED,
                    }],
                };
                let _ = AdjustTokenPrivileges(
                    token,
                    0,
                    &tp,
                    size_of::<TokenPrivilegesOne>() as u32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
        CloseHandle(token);
    }
}

pub fn get_memory_info() -> Option<MemoryStatusEx> {
    unsafe {
        let mut status = MemoryStatusEx {
            dw_length: size_of::<MemoryStatusEx>() as u32,
            ..Default::default()
        };
        if GlobalMemoryStatusEx(&mut status) != 0 {
            Some(status)
        } else {
            None
        }
    }
}

pub fn query_memory_pools() -> MemoryPoolStats {
    let is_elevated = is_process_elevated();
    let Some(mem) = get_memory_info() else {
        return MemoryPoolStats {
            is_elevated,
            ..Default::default()
        };
    };

    let gb = 1024.0 * 1024.0 * 1024.0_f64;
    let mb = 1024.0 * 1024.0_f64;

    let phys_total = mem.ull_total_phys as f64 / gb;
    let phys_avail = mem.ull_avail_phys as f64 / gb;
    let phys_used = (phys_total - phys_avail).max(0.0);
    let phys_pct = if phys_total > 0.0 {
        ((phys_used / phys_total) * 100.0) as f32
    } else {
        mem.dw_memory_load as f32
    };

    let page_total = mem.ull_total_page_file as f64 / gb;
    let page_avail = mem.ull_avail_page_file as f64 / gb;
    let page_used = (page_total - page_avail).max(0.0);
    let page_pct = if page_total > 0.0 {
        ((page_used / page_total) * 100.0).clamp(0.0, 100.0) as f32
    } else {
        0.0
    };

    let (sys_cache_mb, sys_cache_peak_mb, sys_cache_percent) = unsafe {
        let mut sfci = SystemFileCacheInfo::default();
        let mut ret_len: u32 = 0;
        let status = NtQuerySystemInformation(
            SYS_FILE_CACHE_INFO,
            &mut sfci as *mut SystemFileCacheInfo as *mut c_void,
            size_of::<SystemFileCacheInfo>() as u32,
            &mut ret_len,
        );
        if status >= 0 && (sfci.current_size > 0 || sfci.peak_size > 0) {
            let cur_mb = (sfci.current_size as f64 / mb) as f32;
            let peak_mb = (sfci.peak_size as f64 / mb).max(sfci.current_size as f64 / mb) as f32;
            let pct = if peak_mb > 0.0 {
                ((cur_mb / peak_mb) * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            (cur_mb, peak_mb, pct)
        } else {
            (0.0, 0.0, 0.0)
        }
    };

    MemoryPoolStats {
        phys_used_gb: phys_used as f32,
        phys_total_gb: phys_total as f32,
        phys_percent: phys_pct,
        page_used_gb: page_used as f32,
        page_total_gb: page_total as f32,
        page_percent: page_pct,
        sys_cache_mb,
        sys_cache_peak_mb,
        sys_cache_percent,
        is_elevated,
    }
}

pub fn execute_memory_clean_areas(areas: &CleanAreas) -> Option<CleanResult> {
    enable_memory_privileges();
    let is_elevated = is_process_elevated();
    let before = get_memory_info()?;

    let mut trimmed_count = 0usize;
    let mut areas_attempted = 0usize;
    let mut flushed_volumes = 0usize;
    let mut kernel_caches_purged = false;

    unsafe {
        // 1. Working Sets (per-process EmptyWorkingSet + kernel MemoryEmptyWorkingSets)
        if areas.working_sets {
            areas_attempted += 1;
            let self_pid = GetCurrentProcessId();
            SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);

            let mut pids: [u32; 4096] = [0; 4096];
            let mut bytes_needed: u32 = 0;
            let ok = K32EnumProcesses(
                pids.as_mut_ptr(),
                (pids.len() * size_of::<u32>()) as u32,
                &mut bytes_needed,
            );
            if ok != 0 {
                let count = (bytes_needed as usize) / size_of::<u32>();
                for &pid in &pids[..count] {
                    if pid == 0 || pid == self_pid {
                        continue;
                    }
                    let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_SET_QUOTA, 0, pid);
                    if !handle.is_null() {
                        if K32EmptyWorkingSet(handle) != 0 {
                            trimmed_count += 1;
                        }
                        CloseHandle(handle);
                    }
                }
            }

            if is_elevated
                && NtSetSystemInformation(
                    SYS_MEMORY_LIST_INFO,
                    &CMD_EMPTY_WORKING_SETS as *const u32 as *const c_void,
                    size_of::<u32>() as u32,
                ) >= 0
            {
                kernel_caches_purged = true;
            }
        }

        // 2. System File Cache (System Working Set)
        if areas.system_file_cache {
            areas_attempted += 1;
            SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
            let sfci = SystemFileCacheInfo {
                minimum_working_set: usize::MAX,
                maximum_working_set: usize::MAX,
                ..Default::default()
            };
            if NtSetSystemInformation(
                SYS_FILE_CACHE_INFO,
                &sfci as *const SystemFileCacheInfo as *const c_void,
                size_of::<SystemFileCacheInfo>() as u32,
            ) >= 0
            {
                kernel_caches_purged = true;
            }
        }

        // 3. Modified Page List
        if areas.modified_page_list {
            areas_attempted += 1;
            if NtSetSystemInformation(
                SYS_MEMORY_LIST_INFO,
                &CMD_FLUSH_MODIFIED_LIST as *const u32 as *const c_void,
                size_of::<u32>() as u32,
            ) >= 0
            {
                kernel_caches_purged = true;
            }
        }

        // 4. Standby List
        if areas.standby_list {
            areas_attempted += 1;
            if NtSetSystemInformation(
                SYS_MEMORY_LIST_INFO,
                &CMD_PURGE_STANDBY_LIST as *const u32 as *const c_void,
                size_of::<u32>() as u32,
            ) >= 0
            {
                kernel_caches_purged = true;
            }
        }

        // 5. Standby List (Low Priority)
        if areas.standby_list_low {
            areas_attempted += 1;
            if NtSetSystemInformation(
                SYS_MEMORY_LIST_INFO,
                &CMD_PURGE_LOW_PRIORITY_STANDBY_LIST as *const u32 as *const c_void,
                size_of::<u32>() as u32,
            ) >= 0
            {
                kernel_caches_purged = true;
            }
        }

        // 6. Modified File Cache (flush local fixed volume buffers)
        if areas.modified_file_cache {
            areas_attempted += 1;
            flushed_volumes = flush_fixed_volume_caches();
            if flushed_volumes > 0 {
                kernel_caches_purged = true;
            }
        }

        // 7. Registry Cache (Win 8.1+)
        if areas.registry_cache {
            areas_attempted += 1;
            if NtSetSystemInformation(SYS_REGISTRY_RECONCILIATION_INFO, std::ptr::null(), 0) >= 0 {
                kernel_caches_purged = true;
            }
        }

        // 8. Combine Memory Lists (Win 10+)
        if areas.combine_memory_lists {
            areas_attempted += 1;
            let mci = MemoryCombineInformationEx::default();
            if NtSetSystemInformation(
                SYS_COMBINE_PHYSICAL_MEMORY_INFO,
                &mci as *const MemoryCombineInformationEx as *const c_void,
                size_of::<MemoryCombineInformationEx>() as u32,
            ) >= 0
            {
                kernel_caches_purged = true;
            }
        }
    }

    let after = get_memory_info()?;
    let freed_bytes = after.ull_avail_phys.saturating_sub(before.ull_avail_phys);
    let freed_mb = freed_bytes as f64 / (1024.0 * 1024.0);

    Some(CleanResult {
        freed_mb,
        old_load_pct: before.dw_memory_load,
        new_load_pct: after.dw_memory_load,
        trimmed_processes: trimmed_count,
        areas_attempted,
        flushed_volumes,
        kernel_caches_purged,
        is_elevated,
    })
}

pub fn execute_memory_clean(
    clean_working_sets: bool,
    clean_system_ws: bool,
    clean_standby_list: bool,
) -> Option<CleanResult> {
    let areas = CleanAreas {
        working_sets: clean_working_sets,
        system_file_cache: clean_system_ws,
        modified_page_list: false,
        standby_list: clean_standby_list,
        standby_list_low: false,
        modified_file_cache: false,
        registry_cache: false,
        combine_memory_lists: false,
    };
    execute_memory_clean_areas(&areas)
}

unsafe fn flush_fixed_volume_caches() -> usize {
    let mut buf = [0u16; 512];
    let len = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
    if len == 0 || len >= buf.len() {
        return 0;
    }

    let mut flushed = 0usize;
    let mut start = 0usize;
    while start < len {
        let end = buf[start..len]
            .iter()
            .position(|&c| c == 0)
            .map(|rel| start + rel)
            .unwrap_or(len);
        if end == start {
            break;
        }

        let root_slice = &buf[start..=end];
        if end >= start + 2 && unsafe { GetDriveTypeW(root_slice.as_ptr()) } == DRIVE_FIXED {
            let letter = buf[start];
            let vol_path: [u16; 7] = [
                b'\\' as u16,
                b'\\' as u16,
                b'.' as u16,
                b'\\' as u16,
                letter,
                b':' as u16,
                0,
            ];
            let h = unsafe {
                CreateFileW(
                    vol_path.as_ptr(),
                    GENERIC_WRITE,
                    FILE_SHARE_READ_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if !h.is_null() && (h as isize) != INVALID_HANDLE_VAL {
                if unsafe { FlushFileBuffers(h) } != 0 {
                    flushed += 1;
                }
                unsafe {
                    CloseHandle(h);
                }
            }
        }
        start = end + 1;
    }
    flushed
}
