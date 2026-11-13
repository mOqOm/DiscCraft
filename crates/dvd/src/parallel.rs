//! Running independent jobs (transcodes) on several threads.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

/// What a worker tells the calling thread.
enum Message<R> {
    /// A job's progress: its label and how far it is (0 to 1).
    Progress(String, f32),
    /// Job `index` finished.
    Done(usize, R),
}

/// Runs `work` on every item, on as many threads as the CPU has cores (at most one per item), and returns the
/// results in the items' order. `progress` is called on the calling thread with what the jobs report.
pub(crate) fn run<T: Sync, R: Send>(
    items: &[T],
    progress: &mut dyn FnMut(&str, f32),
    work: impl Fn(&T, &mut dyn FnMut(&str, f32)) -> R + Sync,
) -> Vec<R> {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(items.len());
    if workers <= 1 {
        return items.iter().map(|item| work(item, progress)).collect();
    }
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel::<Message<R>>();
    let mut results: Vec<Option<R>> = items.iter().map(|_| None).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let (next, work) = (&next, &work);
            let _ = scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let progress_sender = sender.clone();
                    let mut report = move |label: &str, fraction: f32| {
                        let _ = progress_sender.send(Message::Progress(label.to_owned(), fraction));
                    };
                    let result = work(item, &mut report);
                    let _ = sender.send(Message::Done(index, result));
                }
            });
        }
        drop(sender);
        for message in receiver {
            match message {
                Message::Progress(label, fraction) => progress(&label, fraction),
                Message::Done(index, result) => {
                    if let Some(slot) = results.get_mut(index) {
                        *slot = Some(result);
                    }
                }
            }
        }
    });
    // Every job sent its result (a worker that panicked takes the scope, and so the build, down with it).
    results.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn results_keep_the_items_order_and_progress_reaches_the_caller() {
        let items: Vec<u64> = (1..=20).collect();
        let mut heard = 0;
        let results = run(&items, &mut |_, _| heard += 1, |item, report| {
            report("job", 0.5);
            // Uneven work, so jobs finish out of order.
            std::thread::sleep(std::time::Duration::from_millis(21 - item));
            item * 10
        });
        assert_eq!(
            results,
            items.iter().map(|item| item * 10).collect::<Vec<_>>()
        );
        assert_eq!(heard, 20);
    }
}
