//! The displays connected to the computer (Encore's Preferences ▸ Audio/Video Output lists them as video
//! devices: "Adobe Monitor 1: 2560 x 1440").
//!
//! Windows: `EnumDisplayMonitors` and `GetMonitorInfoW` (user32; the `MONITORINFO` structure of the Windows API
//! documentation). Other systems: none are listed (the video output is then a window the user places).

/// One display: where it is on the desktop and how large, in the system's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Display {
    /// Left edge on the desktop.
    pub x: i32,
    /// Top edge on the desktop.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The system's primary display.
    pub primary: bool,
}

/// The most displays listed.
const MAX_DISPLAYS: usize = 16;

/// The displays, in the system's order; empty when the system does not tell.
#[must_use]
pub fn displays() -> Vec<Display> {
    let mut found = imp::list();
    found.truncate(MAX_DISPLAYS);
    found
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use super::{Display, MAX_DISPLAYS};

    /// `RECT`.
    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    /// `MONITORINFO`.
    #[repr(C)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }

    /// `MONITORINFOF_PRIMARY`.
    const PRIMARY: u32 = 1;

    type Callback = unsafe extern "system" fn(isize, isize, *mut Rect, isize) -> i32;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumDisplayMonitors(
            hdc: isize,
            clip: *const Rect,
            callback: Option<Callback>,
            data: isize,
        ) -> i32;
        fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
    }

    /// Called by `EnumDisplayMonitors` once a display, with the list's address in `data`.
    unsafe extern "system" fn collect(
        monitor: isize,
        _hdc: isize,
        _rect: *mut Rect,
        data: isize,
    ) -> i32 {
        // SAFETY: `data` is the address of the `Vec<Display>` that `list` passes to `EnumDisplayMonitors`; the
        // list outlives that call, the callback runs on the calling thread during it, and nothing else touches
        // the list meanwhile.
        let list = unsafe { &mut *(data as *mut Vec<Display>) };
        if list.len() >= MAX_DISPLAYS {
            // Enough: stop the enumeration.
            return 0;
        }
        let empty = || Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut info = MonitorInfo {
            size: 40,
            monitor: empty(),
            work: empty(),
            flags: 0,
        };
        // SAFETY: `info` is a valid, writable `MONITORINFO` whose `cbSize` is its size (40 bytes); the function
        // writes only inside it. `monitor` is the handle the system just gave this callback.
        let ok = unsafe { GetMonitorInfoW(monitor, &raw mut info) };
        if ok != 0 {
            let width = info.monitor.right.saturating_sub(info.monitor.left);
            let height = info.monitor.bottom.saturating_sub(info.monitor.top);
            if let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height))
                && width > 0
                && height > 0
            {
                list.push(Display {
                    x: info.monitor.left,
                    y: info.monitor.top,
                    width,
                    height,
                    primary: info.flags & PRIMARY != 0,
                });
            }
        }
        1
    }

    pub(super) fn list() -> Vec<Display> {
        let mut found: Vec<Display> = Vec::with_capacity(MAX_DISPLAYS);
        let data = &raw mut found as isize;
        // SAFETY: a null device context and clip rectangle ask for every display; `collect` has the signature
        // `MONITORENUMPROC` requires and is given `data`, the address of `found`, which stays valid and is not
        // otherwise used until the function returns. The list was allocated for `MAX_DISPLAYS` entries and
        // `collect` adds no more, so pushing does not reallocate (and so cannot fail) inside the callback.
        let _ = unsafe { EnumDisplayMonitors(0, std::ptr::null(), Some(collect), data) };
        found
    }
}

#[cfg(not(windows))]
mod imp {
    pub(super) fn list() -> Vec<super::Display> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_displays_are_listed_with_their_sizes() {
        let displays = super::displays();
        // A machine without a desktop session (a build server) lists none; one with a session has a primary.
        if displays.is_empty() {
            return;
        }
        assert!(
            displays.iter().filter(|display| display.primary).count() == 1,
            "{displays:?}"
        );
        assert!(
            displays
                .iter()
                .all(|display| (1..=32_768).contains(&display.width)
                    && (1..=32_768).contains(&display.height)),
            "{displays:?}"
        );
    }
}
