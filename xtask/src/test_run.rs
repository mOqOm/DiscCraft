//! `cargo xtask test` (M6.354): builds every test target, then runs the test binaries several at a time instead of
//! one after another as `cargo test` does. The whole takes about as long as its slowest binary.
//!
//! - **Order:** the slowest binaries of the last run start first (`target/xtask-test-times.tsv`).
//! - **Shared fixtures:** some binaries make the same file under `target/fixtures/` when it is missing. Until a
//!   run has passed with the fixtures there (`target/fixtures/.parallel-ready`), the binaries run one at a time.
//!   A binary that fails among others is run again alone before it counts as failed.
//! - **Hangs:** a binary that runs longer than the timeout is stopped and fails (M6.349's hang held a run for
//!   50 minutes).
//! - **Output:** one line per binary as it finishes; the whole output of a failing one.
//! - **Settings:** `DISCCRAFT_TEST_JOBS` (binaries at once; default a quarter of the logical CPUs, 2 to 8) and
//!   `DISCCRAFT_TEST_TIMEOUT` (seconds; default 900).

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::Result;
use crate::cargo_json;

/// One test binary to run.
#[derive(Debug, Clone)]
struct TestBinary {
    executable: PathBuf,
    package_dir: PathBuf,
    /// `package/target (kind)`, as the summary names it.
    label: String,
}

/// How a test binary's run went.
#[derive(Debug)]
struct Outcome {
    binary: TestBinary,
    seconds: f64,
    passed: bool,
    timed_out: bool,
    log: PathBuf,
}

pub fn run(root: &Path) -> Result<()> {
    let started = Instant::now();
    let build = cargo_json::build(root, &["test", "--workspace", "--all-targets", "--no-run"])?;
    let mut binaries: Vec<TestBinary> = build
        .artifacts
        .iter()
        .filter(|artifact| artifact.test)
        .filter_map(|artifact| {
            let executable = artifact.executable.clone()?;
            let package = artifact
                .package_dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            Some(TestBinary {
                executable,
                package_dir: artifact.package_dir.clone(),
                label: format!("{package}/{} ({})", artifact.target, artifact.kind),
            })
        })
        .collect();
    let built = started.elapsed().as_secs_f64();

    let target = root.join("target");
    let times_path = target.join("xtask-test-times.tsv");
    let times = read_times(&times_path);
    // Slowest first; a binary not seen before may be slow too.
    binaries.sort_by(|a, b| {
        let time = |binary: &TestBinary| times.get(&binary.label).copied().unwrap_or(f64::MAX);
        time(b).total_cmp(&time(a))
    });

    let ffmpeg = ffmpeg_available();
    let marker = target.join("fixtures").join(".parallel-ready");
    let ready = fs::read_to_string(&marker).is_ok_and(|text| text.trim() == marker_text(ffmpeg));
    let jobs = if ready { jobs() } else { 1 };
    let timeout = timeout();
    let logs = target.join("xtask-test-logs");
    fs::create_dir_all(&logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    println!(
        "==> {} test binaries, {jobs} at a time{}",
        binaries.len(),
        if ready {
            ""
        } else {
            " (the fixtures are made first: no run has passed with them yet)"
        }
    );

    let ran = Instant::now();
    let mut outcomes = run_all(&binaries, jobs, timeout, &logs);
    // A failure among others is tried again alone: two binaries may have made the same fixture at once.
    let mut retried = Vec::new();
    if jobs > 1 {
        for outcome in &mut outcomes {
            if !outcome.passed && !outcome.timed_out {
                println!("again alone: {}", outcome.binary.label);
                // The first log is kept beside the second: a test that fails only among others is worth finding.
                let first = outcome.log.with_extension("first.log");
                let _ = fs::rename(&outcome.log, &first);
                let failed_first = failed_tests(&fs::read_to_string(&first).unwrap_or_default());
                let again = run_one(&outcome.binary, timeout, &logs);
                if again.passed {
                    retried.push(format!(
                        "{}: {} (log: {})",
                        outcome.binary.label,
                        failed_first.join(", "),
                        first.display()
                    ));
                }
                *outcome = again;
            }
        }
    }
    let wall = ran.elapsed().as_secs_f64();

    write_times(&times_path, &outcomes);
    let failed: Vec<&Outcome> = outcomes.iter().filter(|outcome| !outcome.passed).collect();
    for outcome in &failed {
        println!("\n---- {} ----", outcome.binary.label);
        if outcome.timed_out {
            println!("stopped after {} s", timeout.as_secs());
        }
        println!("{}", fs::read_to_string(&outcome.log).unwrap_or_default());
    }
    let (passed_tests, failed_tests) = outcomes
        .iter()
        .map(|outcome| count_tests(&outcome.log))
        .fold((0, 0), |(p, f), (a, b)| (p + a, f + b));
    let sum: f64 = outcomes.iter().map(|outcome| outcome.seconds).sum();
    println!(
        "==> tests: {passed_tests} passed, {failed_tests} failed in {} binaries; built in {built:.0} s, ran in \
         {wall:.0} s ({sum:.0} s one after another)",
        outcomes.len()
    );
    if !retried.is_empty() {
        println!(
            "warning: passed only when run again alone (a fixture made by two binaries at once, or a test that \
             depends on timing?):"
        );
        for retry in &retried {
            println!("  {retry}");
        }
    }
    if failed.is_empty() {
        if let Some(parent) = marker.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&marker, marker_text(ffmpeg));
        Ok(())
    } else {
        Err(format!("{} test binaries failed", failed.len()))
    }
}

/// Runs `binaries` `jobs` at a time (in the order given), printing each as it finishes.
fn run_all(binaries: &[TestBinary], jobs: usize, timeout: Duration, logs: &Path) -> Vec<Outcome> {
    let queue = Arc::new(Mutex::new(
        binaries.iter().cloned().collect::<VecDeque<_>>(),
    ));
    let (sender, receiver) = mpsc::channel::<Outcome>();
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let queue = Arc::clone(&queue);
            let sender = sender.clone();
            scope.spawn(move || {
                loop {
                    let next = queue.lock().ok().and_then(|mut queue| queue.pop_front());
                    let Some(binary) = next else { break };
                    if sender.send(run_one(&binary, timeout, logs)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let mut outcomes = Vec::new();
        for outcome in receiver {
            println!(
                "{} {:6.1} s  {}",
                if outcome.passed { "ok  " } else { "FAIL" },
                outcome.seconds,
                outcome.binary.label
            );
            outcomes.push(outcome);
        }
        outcomes
    })
}

/// Runs one test binary in its package's directory (as `cargo test` does), its output to a log file.
fn run_one(binary: &TestBinary, timeout: Duration, logs: &Path) -> Outcome {
    let name = binary
        .executable
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let log = logs.join(format!("{name}.log"));
    let started = Instant::now();
    let mut outcome = Outcome {
        binary: binary.clone(),
        seconds: 0.0,
        passed: false,
        timed_out: false,
        log: log.clone(),
    };
    let spawned = fs::File::create(&log).and_then(|file| {
        let errors = file.try_clone()?;
        crate::without_xtask_env(&mut Command::new(&binary.executable))
            .current_dir(&binary.package_dir)
            .env("CARGO_MANIFEST_DIR", &binary.package_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(file))
            .stderr(Stdio::from(errors))
            .spawn()
    });
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::write(
                &log,
                format!("could not start {}: {error}", binary.executable.display()),
            );
            return outcome;
        }
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                outcome.passed = status.success();
                break;
            }
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                outcome.timed_out = true;
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }
    outcome.seconds = started.elapsed().as_secs_f64();
    outcome
}

/// Passed and failed tests in a test binary's log (its `test result:` lines).
fn count_tests(log: &Path) -> (u64, u64) {
    let text = fs::read_to_string(log).unwrap_or_default();
    let mut counts = (0, 0);
    for line in text.lines().filter(|line| line.starts_with("test result:")) {
        let number = |word: &str| {
            line.split(';')
                .find(|part| part.trim_end().ends_with(word))
                .and_then(|part| part.split_whitespace().rev().nth(1))
                .and_then(|count| count.parse::<u64>().ok())
                .unwrap_or(0)
        };
        counts.0 += number("passed");
        counts.1 += number("failed");
    }
    counts
}

/// The names of the tests a test binary's log says failed (`test <name> ... FAILED`, or `<name> --- FAILED` when
/// the output is terse).
fn failed_tests(log: &str) -> Vec<String> {
    let mut names: Vec<String> = log
        .lines()
        .filter_map(|line| {
            line.strip_prefix("test ")
                .and_then(|rest| rest.strip_suffix(" ... FAILED"))
                .or_else(|| line.strip_suffix(" --- FAILED"))
                .map(|name| name.trim().to_string())
        })
        .collect();
    names.dedup();
    names
}

fn read_times(path: &Path) -> HashMap<String, f64> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (label, seconds) = line.split_once('\t')?;
            Some((label.to_string(), seconds.parse().ok()?))
        })
        .collect()
}

fn write_times(path: &Path, outcomes: &[Outcome]) {
    let text: String = outcomes
        .iter()
        .map(|outcome| format!("{}\t{:.1}\n", outcome.binary.label, outcome.seconds))
        .collect();
    let _ = fs::write(path, text);
}

/// Whether the tests that compare with ffmpeg run (and make their fixtures): ffmpeg found as the tests find it
/// (disccraft-test-support, M9.67).
fn ffmpeg_available() -> bool {
    disccraft_test_support::ffmpeg().is_some()
}

fn marker_text(ffmpeg: bool) -> &'static str {
    if ffmpeg { "ffmpeg" } else { "no ffmpeg" }
}

fn jobs() -> usize {
    std::env::var("DISCCRAFT_TEST_JOBS")
        .ok()
        .and_then(|jobs| jobs.parse::<usize>().ok())
        .filter(|jobs| *jobs > 0)
        .unwrap_or_else(|| {
            let cpus = std::thread::available_parallelism().map_or(4, |cpus| cpus.get());
            (cpus / 4).clamp(2, 8)
        })
}

fn timeout() -> Duration {
    Duration::from_secs(
        std::env::var("DISCCRAFT_TEST_TIMEOUT")
            .ok()
            .and_then(|seconds| seconds.parse().ok())
            .unwrap_or(900),
    )
}

#[cfg(test)]
mod tests {
    use super::failed_tests;

    #[test]
    fn failed_tests_are_named_from_the_log() {
        let log = "running 3 tests\n\
                   test a::works ... ok\n\
                   test a::flaky ... FAILED\n\
                   b::terse --- FAILED\n\
                   \n\
                   failures:\n\
                   \x20   a::flaky\n\
                   test result: FAILED. 1 passed; 2 failed\n";
        assert_eq!(failed_tests(log), ["a::flaky", "b::terse"]);
        assert!(failed_tests("test a::works ... ok\n").is_empty());
    }
}
