//! How much memory this computer has, for showing whether the optional AI
//! models fit.

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

#[cfg(target_os = "linux")]
mod imp {
    use super::Memory;

    pub fn memory() -> Option<Memory> {
        parse(&std::fs::read_to_string("/proc/meminfo").ok()?)
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
    use super::Memory;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

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
}
