//! The taskbar button of this program's window (M6.505): how far a long job (a build, a disc writing) is while the
//! window is minimized, and a flash when the job ends while the window is not in front.
//!
//! Windows: `ITaskbarList3::SetProgressState` and `SetProgressValue` (shobjidl.h, Windows 7 and later) and
//! `FlashWindowEx` with `FLASHW_TRAY | FLASHW_TIMERNOFG` (user32), as the Windows API documentation describes them.
//! The window is found as [`crate::keep_on_top`] finds it: this program's visible top-level window with the title.
//! Other systems: nothing is shown.

/// What the taskbar button shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TaskbarProgress {
    /// Nothing: the plain button.
    None,
    /// A job whose progress is not known: a bar that moves.
    Indeterminate,
    /// How far a job is, from 0 to 1.
    Normal(f32),
    /// A job that failed: a full red bar.
    Error,
}

/// The steps of the button's bar (`SetProgressValue`'s total).
const STEPS: u64 = 1000;

/// `fraction` (clamped to 0 to 1) in [`STEPS`].
fn steps(fraction: f32) -> u64 {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    // `as` saturates, and the value is within 0..=STEPS.
    (f64::from(fraction) * STEPS as f64).round() as u64
}

/// Shows `progress` on the taskbar button of this program's window titled `title`: whether the button took it.
pub fn set_progress(title: &str, progress: TaskbarProgress) -> bool {
    crate::displays::windows_titled(title)
        .first()
        .is_some_and(|window| imp::set_progress(*window, progress))
}

/// Flashes the taskbar button of this program's window titled `title`, which then stays lit until the window comes
/// to the front (nothing when it is in front already): whether a window was found.
pub fn flash(title: &str) -> bool {
    crate::displays::windows_titled(title)
        .first()
        .is_some_and(|window| imp::flash(*window))
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use super::{STEPS, TaskbarProgress, steps};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        ITaskbarList3, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TaskbarList,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx,
    };

    /// How many times the button flashes before it stays lit.
    const FLASHES: u32 = 3;

    /// The window handle `window` as the system's type.
    fn hwnd(window: isize) -> HWND {
        HWND(window as *mut core::ffi::c_void)
    }

    pub(super) fn set_progress(window: isize, progress: TaskbarProgress) -> bool {
        // SAFETY: COM for this call on the calling thread (the window's: a single-threaded apartment, which the
        // window's drag and drop has started already, so this only counts one more use of it).
        let started = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let shown = show(hwnd(window), progress).is_ok();
        if started {
            // SAFETY: balances the successful CoInitializeEx above, on the same thread, after the taskbar object
            // made in `show` has been released.
            unsafe { CoUninitialize() };
        }
        shown
    }

    /// Shows `progress` on `window`'s button through a taskbar object made for the call.
    fn show(window: HWND, progress: TaskbarProgress) -> windows::core::Result<()> {
        // SAFETY: plain COM calls on an object made here and released at the end of the block; `window` is a
        // top-level window of this program that the system just listed (one closed since makes the calls fail).
        unsafe {
            let list: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
            list.HrInit()?;
            match progress {
                TaskbarProgress::None => list.SetProgressState(window, TBPF_NOPROGRESS),
                TaskbarProgress::Indeterminate => list.SetProgressState(window, TBPF_INDETERMINATE),
                TaskbarProgress::Normal(fraction) => {
                    list.SetProgressState(window, TBPF_NORMAL)?;
                    list.SetProgressValue(window, steps(fraction), STEPS)
                }
                TaskbarProgress::Error => {
                    list.SetProgressState(window, TBPF_ERROR)?;
                    list.SetProgressValue(window, STEPS, STEPS)
                }
            }
        }
    }

    pub(super) fn flash(window: isize) -> bool {
        let info = FLASHWINFO {
            cbSize: u32::try_from(std::mem::size_of::<FLASHWINFO>()).unwrap_or(0),
            hwnd: hwnd(window),
            dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
            uCount: FLASHES,
            dwTimeout: 0,
        };
        // SAFETY: `info` is a whole `FLASHWINFO` with its size, alive for the call, which only reads it; the
        // window is one of this program's that the system just listed. The result is whether the window was lit
        // before, not whether the call worked.
        let _ = unsafe { FlashWindowEx(&raw const info) };
        true
    }
}

#[cfg(not(windows))]
mod imp {
    use super::TaskbarProgress;

    pub(super) fn set_progress(_window: isize, _progress: TaskbarProgress) -> bool {
        false
    }

    pub(super) fn flash(_window: isize) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{STEPS, TaskbarProgress, flash, set_progress, steps};

    /// A fraction becomes the bar's steps; one out of range or not a number does not.
    #[test]
    fn a_fraction_becomes_the_bars_steps() {
        assert_eq!(steps(0.0), 0);
        assert_eq!(steps(0.5), STEPS / 2);
        assert_eq!(steps(1.0), STEPS);
        assert_eq!(steps(2.0), STEPS);
        assert_eq!(steps(-1.0), 0);
        assert_eq!(steps(f32::NAN), 0);
    }

    /// The test program has no window by this title: nothing is shown and nothing flashes.
    #[test]
    fn a_window_that_is_not_there_shows_nothing() {
        let title = "DiscCraft test: no such window";
        assert!(!set_progress(title, TaskbarProgress::Normal(0.5)));
        assert!(!flash(title));
        assert!(!set_progress("", TaskbarProgress::None));
    }
}
