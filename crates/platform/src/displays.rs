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

/// The most windows of this program [`keep_on_top`] raises at once.
const MAX_RAISED: usize = 8;

/// Puts this program's visible windows titled `title` above every other window, the taskbar included, without
/// making them the active window; the number of windows raised.
///
/// For the video output on a display of its own (M6.426): Windows moves a display's taskbar behind a window that
/// covers the display only while that window is the active one, so with DiscCraft's own window active on another
/// display the taskbar showed over the picture. A window kept above the others ("topmost") stays above the
/// taskbar until the system raises the taskbar again (when it is clicked, or when a full-screen window stops
/// being the active one), so the caller raises the window again from time to time.
///
/// Windows: `EnumWindows`, `GetWindowThreadProcessId`, `IsWindowVisible`, `GetWindowTextW` and `SetWindowPos`
/// with `HWND_TOPMOST` (user32, the Windows API documentation). Other systems: nothing is raised.
#[must_use]
pub fn keep_on_top(title: &str) -> usize {
    imp::keep_on_top(&windows_titled(title))
}

/// This program's visible top-level windows titled `title` (at most [`MAX_RAISED`]); none for an empty title, and
/// none on other systems. [`keep_on_top`] raises them; [`crate::taskbar`] shows a job's progress on the first one's
/// taskbar button (M6.505).
pub(crate) fn windows_titled(title: &str) -> Vec<isize> {
    if title.is_empty() {
        return Vec::new();
    }
    imp::windows_titled(title)
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

    type WindowCallback = unsafe extern "system" fn(isize, isize) -> i32;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumDisplayMonitors(
            hdc: isize,
            clip: *const Rect,
            callback: Option<Callback>,
            data: isize,
        ) -> i32;
        fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
        fn EnumWindows(callback: Option<WindowCallback>, data: isize) -> i32;
        fn GetWindowThreadProcessId(window: isize, process: *mut u32) -> u32;
        fn IsWindowVisible(window: isize) -> i32;
        fn GetWindowTextW(window: isize, text: *mut u16, capacity: i32) -> i32;
        fn SetWindowPos(
            window: isize,
            after: isize,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> i32;
    }

    /// `HWND_TOPMOST`.
    const TOPMOST: isize = -1;
    /// `SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOOWNERZORDER`.
    const ONLY_ORDER: u32 = 0x0001 | 0x0002 | 0x0010 | 0x0200;
    /// The longest title compared, in UTF-16 units (longer titles do not match).
    const TITLE_UNITS: usize = 256;

    /// What `matching` looks for and what it found.
    struct Search {
        process: u32,
        title: Vec<u16>,
        found: Vec<isize>,
    }

    /// Called by `EnumWindows` once a top-level window, with the search's address in `data`.
    unsafe extern "system" fn matching(window: isize, data: isize) -> i32 {
        // SAFETY: `data` is the address of the `Search` that `keep_on_top` passes to `EnumWindows`; it outlives
        // that call, the callback runs on the calling thread during it, and nothing else touches it meanwhile.
        let search = unsafe { &mut *(data as *mut Search) };
        if search.found.len() >= super::MAX_RAISED {
            // Enough: stop the enumeration.
            return 0;
        }
        let mut process = 0u32;
        // SAFETY: `window` is the handle the system just gave this callback; `process` is a valid, writable u32.
        let _ = unsafe { GetWindowThreadProcessId(window, &raw mut process) };
        if process != search.process {
            return 1;
        }
        // SAFETY: as above, a handle from the system; the function only reads the window's state.
        if unsafe { IsWindowVisible(window) } == 0 {
            return 1;
        }
        let mut text = [0u16; TITLE_UNITS];
        // SAFETY: `text` is a writable buffer of `TITLE_UNITS` units and the capacity passed is that length; the
        // function writes at most that many units, the terminating zero included.
        let length = unsafe { GetWindowTextW(window, text.as_mut_ptr(), TITLE_UNITS as i32) };
        let length = usize::try_from(length).unwrap_or(0).min(TITLE_UNITS);
        if text.get(..length) == Some(search.title.as_slice()) {
            search.found.push(window);
        }
        1
    }

    pub(super) fn windows_titled(title: &str) -> Vec<isize> {
        let title: Vec<u16> = title.encode_utf16().collect();
        if title.len() >= TITLE_UNITS {
            return Vec::new();
        }
        let mut search = Search {
            process: std::process::id(),
            title,
            found: Vec::with_capacity(super::MAX_RAISED),
        };
        let data = &raw mut search as isize;
        // SAFETY: `matching` has the signature `WNDENUMPROC` requires and is given `data`, the address of
        // `search`, which stays valid and is not otherwise used until the function returns. The list was
        // allocated for `MAX_RAISED` entries and `matching` adds no more, so pushing does not reallocate inside
        // the callback.
        let _ = unsafe { EnumWindows(Some(matching), data) };
        search.found
    }

    pub(super) fn keep_on_top(windows: &[isize]) -> usize {
        let mut raised = 0;
        for &window in windows {
            // SAFETY: `window` is a top-level window of this process that `EnumWindows` just listed; the flags
            // change only its place in the order (not its size, position or activation). A window closed since
            // makes the call fail, which is ignored.
            if unsafe { SetWindowPos(window, TOPMOST, 0, 0, 0, 0, ONLY_ORDER) } != 0 {
                raised += 1;
            }
        }
        raised
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

    pub(super) fn windows_titled(_title: &str) -> Vec<isize> {
        Vec::new()
    }

    pub(super) fn keep_on_top(_windows: &[isize]) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_this_programs_windows_with_the_title_are_raised() {
        // The test program has no window by these titles (and an empty title never matches).
        assert_eq!(super::keep_on_top(""), 0);
        assert_eq!(super::keep_on_top("DiscCraft test: no such window"), 0);
        assert_eq!(super::keep_on_top(&"x".repeat(1000)), 0);
    }

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
