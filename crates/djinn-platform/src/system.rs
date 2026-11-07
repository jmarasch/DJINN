//! How much memory this computer has (for showing whether the optional AI
//! models fit), how busy its processor is, and how much room a disk has
//! (for the admin's health dashboard).

use std::path::Path;

/// Bytes of memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    /// Free for programs right now (counting caches the system can drop).
    pub available: u64,
}

/// `None` where DJINN can't tell.
pub fn memory() -> Option<Memory> {
    imp::memory()
}

/// Processor time since the computer started, in the system's own ticks.
/// The share of `busy` in `total` between two readings is how busy the
/// processor was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuTimes {
    pub busy: u64,
    pub total: u64,
}

impl CpuTimes {
    /// How busy the processor was between `earlier` and this reading, 0 to 1.
    pub fn busy_since(&self, earlier: &CpuTimes) -> Option<f32> {
        let total = self.total.checked_sub(earlier.total)?;
        let busy = self.busy.checked_sub(earlier.busy)?;
        (total > 0).then(|| (busy as f64 / total as f64).clamp(0.0, 1.0) as f32)
    }
}

/// `None` where DJINN can't tell.
pub fn cpu_times() -> Option<CpuTimes> {
    imp::cpu_times()
}

/// Bytes on the disk holding a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disk {
    pub total: u64,
    /// Free for DJINN to use.
    pub available: u64,
}

/// `None` where DJINN can't tell.
pub fn disk(path: &Path) -> Option<Disk> {
    imp::disk(path)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{CpuTimes, Disk, Memory};
    use std::path::Path;

    pub fn memory() -> Option<Memory> {
        parse(&std::fs::read_to_string("/proc/meminfo").ok()?)
    }

    pub fn cpu_times() -> Option<CpuTimes> {
        parse_stat(&std::fs::read_to_string("/proc/stat").ok()?)
    }

    /// The first line of /proc/stat: "cpu user nice system idle iowait irq
    /// softirq steal ...". Idle and iowait are the time not busy.
    pub(super) fn parse_stat(stat: &str) -> Option<CpuTimes> {
        let line = stat.lines().find(|l| l.starts_with("cpu "))?;
        let values: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .take(8)
            .filter_map(|v| v.parse().ok())
            .collect();
        if values.len() < 4 {
            return None;
        }
        let total: u64 = values.iter().sum();
        let idle = values[3] + values.get(4).copied().unwrap_or(0);
        Some(CpuTimes {
            busy: total.saturating_sub(idle),
            total,
        })
    }

    pub fn disk(path: &Path) -> Option<Disk> {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: `name` is a NUL-terminated path and `stats` is room for
        // one statvfs, which the call fills when it returns 0.
        let stats = unsafe {
            if libc::statvfs(name.as_ptr(), stats.as_mut_ptr()) != 0 {
                return None;
            }
            stats.assume_init()
        };
        // The field types differ between platforms (u32 or u64).
        #[allow(clippy::unnecessary_cast)]
        let (block, blocks, available) = (
            (stats.f_frsize as u64).max(1),
            stats.f_blocks as u64,
            stats.f_bavail as u64,
        );
        Some(Disk {
            total: blocks * block,
            available: available * block,
        })
    }

    /// Reads `MemTotal` and `MemAvailable` (in kB) from /proc/meminfo.
    pub(super) fn parse(meminfo: &str) -> Option<Memory> {
        let field = |name: &str| -> Option<u64> {
            let line = meminfo.lines().find(|l| l.starts_with(name))?;
            let kb: u64 = line[name.len()..]
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse()
                .ok()?;
            Some(kb * 1024)
        };
        Some(Memory {
            total: field("MemTotal:")?,
            available: field("MemAvailable:")?,
        })
    }
}

#[cfg(windows)]
mod imp {
    use super::{CpuTimes, Disk, Memory};
    use std::path::Path;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    pub fn cpu_times() -> Option<CpuTimes> {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut idle, mut kernel, mut user) = (zero, zero, zero);
        // SAFETY: three valid FILETIMEs for the call to fill.
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 {
            return None;
        }
        let value = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        // Kernel time includes idle time.
        let total = value(kernel) + value(user);
        Some(CpuTimes {
            busy: total.saturating_sub(value(idle)),
            total,
        })
    }

    pub fn disk(path: &Path) -> Option<Disk> {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: `wide` is a NUL-terminated path; the call fills the three
        // counters when it returns nonzero.
        let ok =
            unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
        (ok != 0).then_some(Disk { total, available })
    }

    pub fn memory() -> Option<Memory> {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            dwMemoryLoad: 0,
            ullTotalPhys: 0,
            ullAvailPhys: 0,
            ullTotalPageFile: 0,
            ullAvailPageFile: 0,
            ullTotalVirtual: 0,
            ullAvailVirtual: 0,
            ullAvailExtendedVirtual: 0,
        };
        // SAFETY: `status` is a valid MEMORYSTATUSEX with dwLength set.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        (ok != 0).then_some(Memory {
            total: status.ullTotalPhys,
            available: status.ullAvailPhys,
        })
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    pub fn memory() -> Option<super::Memory> {
        None
    }

    pub fn cpu_times() -> Option<super::CpuTimes> {
        None
    }

    pub fn disk(_: &std::path::Path) -> Option<super::Disk> {
        None
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn reads_meminfo() {
        let m = super::imp::parse(
            "MemTotal:       32768000 kB\nMemFree:         1000 kB\nMemAvailable:   16384000 kB\n",
        )
        .unwrap();
        assert_eq!(m.total, 32_768_000 * 1024);
        assert_eq!(m.available, 16_384_000 * 1024);
        assert!(super::memory().is_some_and(|m| m.total > 0));
    }

    #[test]
    fn reads_processor_times_and_disk_room() {
        let t = super::imp::parse_stat("cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 2 3 4\n").unwrap();
        assert_eq!((t.busy, t.total), (150, 1000));
        let later = super::CpuTimes {
            busy: 250,
            total: 1200,
        };
        assert_eq!(later.busy_since(&t), Some(0.5));
        assert!(super::cpu_times().is_some());
        let disk = super::disk(std::path::Path::new("/")).unwrap();
        assert!(disk.total > 0 && disk.available <= disk.total);
    }
}
