//! How much memory the computer has (Encore's Preferences ▸ Memory shows "Installed RAM").
//!
//! Windows: `GlobalMemoryStatusEx` (kernel32; the `MEMORYSTATUSEX` structure of the Windows API documentation).
//! Linux: `MemTotal` of `/proc/meminfo`. macOS: `sysctl -n hw.memsize`, asked of the system's own tool so that no
//! foreign function is called there.

/// The physical memory installed, in bytes; `None` when the system does not tell.
#[must_use]
pub fn installed_memory_bytes() -> Option<u64> {
    imp::total().filter(|bytes| *bytes > 0)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    /// `MEMORYSTATUSEX`: the sizes are in bytes.
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    pub(super) fn total() -> Option<u64> {
        let mut status = MemoryStatusEx {
            length: u32::try_from(std::mem::size_of::<MemoryStatusEx>()).ok()?,
            memory_load: 0,
            total_phys: 0,
            avail_phys: 0,
            total_page_file: 0,
            avail_page_file: 0,
            total_virtual: 0,
            avail_virtual: 0,
            avail_extended_virtual: 0,
        };
        // SAFETY: `status` is a valid, writable `MEMORYSTATUSEX` whose `dwLength` is its size, as the function
        // requires; the function writes only inside it and keeps no pointer to it.
        let ok = unsafe { GlobalMemoryStatusEx(&raw mut status) };
        (ok != 0).then_some(status.total_phys)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    pub(super) fn total() -> Option<u64> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = text.lines().find(|line| line.starts_with("MemTotal:"))?;
        let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        kib.checked_mul(1024)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    pub(super) fn total() -> Option<u64> {
        let output = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod imp {
    pub(super) fn total() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_computer_tells_its_memory() {
        // Every machine the tests run on has between 256 MiB and 64 TiB.
        let bytes = super::installed_memory_bytes();
        assert!(
            bytes.is_some_and(|bytes| (1 << 28..1 << 46).contains(&bytes)),
            "{bytes:?}"
        );
    }
}
