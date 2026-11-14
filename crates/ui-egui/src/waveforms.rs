//! Audio waveforms for the timeline (ROADMAP M9.10): each asset's peaks ([`disccraft_engine::waveform`]) worked out
//! once, on a thread of their own, and kept for the session; the timeline draws them as the Craft apps do
//! ([`crate::craft_timeline::draw_waveform`]).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// One asset's peaks and the largest of them.
#[derive(Clone, Debug)]
pub(crate) struct Peaks {
    pub(crate) peaks: Arc<Vec<(f32, f32)>>,
    pub(crate) max: f32,
}

/// The peaks worked out so far, and the assets being worked out.
#[derive(Default)]
pub(crate) struct Waveforms {
    done: Arc<Mutex<HashMap<u64, Peaks>>>,
    pending: Arc<Mutex<HashSet<u64>>>,
}

impl Waveforms {
    /// The peaks of asset `asset` (the file `path`) when they are ready; otherwise starts working them out (once)
    /// and asks for a repaint when they are.
    pub(crate) fn get(&self, asset: u64, path: PathBuf, ctx: &egui::Context) -> Option<Peaks> {
        if let Some(peaks) = self.done.lock().ok()?.get(&asset) {
            return Some(peaks.clone());
        }
        {
            let mut pending = self.pending.lock().ok()?;
            if !pending.insert(asset) {
                return None;
            }
        }
        let (done, pending, ctx) = (self.done.clone(), self.pending.clone(), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("disccraft-waveform".to_owned())
            .spawn(move || {
                // A panic in a decoder must not take the app down: the asset just has no waveform.
                let peaks = std::panic::catch_unwind(|| {
                    disccraft_engine::waveform::audio_peaks(&path).unwrap_or_default()
                })
                .unwrap_or_default();
                let max = peaks
                    .iter()
                    .fold(0f32, |m, (lo, hi)| m.max(lo.abs()).max(hi.abs()));
                if let Ok(mut done) = done.lock() {
                    let _ = done.insert(
                        asset,
                        Peaks {
                            peaks: Arc::new(peaks),
                            max,
                        },
                    );
                }
                if let Ok(mut pending) = pending.lock() {
                    let _ = pending.remove(&asset);
                }
                ctx.request_repaint();
            });
        if spawned.is_err()
            && let Ok(mut pending) = self.pending.lock()
        {
            let _ = pending.remove(&asset);
        }
        None
    }

    /// Forgets every waveform (a project closed, an asset replaced).
    pub(crate) fn clear(&self) {
        if let Ok(mut done) = self.done.lock() {
            done.clear();
        }
    }
}
