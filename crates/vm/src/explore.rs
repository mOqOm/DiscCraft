//! Breadth-first exploration of reachable navigation states.

use std::collections::BTreeSet;
use std::collections::VecDeque;

use crate::VmError;
use crate::exec::Key;
use crate::exec::Location;
use crate::exec::Vm;
use crate::model::DiscModel;

const MAX_STATES: usize = 100_000;
const MENU_KEYS: [Key; 5] = [Key::Up, Key::Down, Key::Left, Key::Right, Key::Enter];

/// Exploration report.
#[derive(Debug, Default)]
pub struct Report {
    /// VMGM PGCNs reached as menus.
    pub menus_seen: BTreeSet<u16>,
    /// Title/chapter pairs reached.
    pub chapters_reached: BTreeSet<(u8, u16)>,
    /// Paths that ended in a non-loop error.
    pub dead_ends: Vec<String>,
    /// Paths that ended in a loop.
    pub loops: Vec<String>,
}

/// Explore all reachable states from first play.
pub fn explore(model: &DiscModel) -> Report {
    let mut report = Report::default();

    let vm = match Vm::start(model) {
        Ok(vm) => vm,
        Err(error) => {
            record_error(&mut report, "", error);
            return report;
        }
    };

    let mut visited: BTreeSet<State> = BTreeSet::new();
    let mut queue = VecDeque::new();

    let initial = state_of(&vm);
    visited.insert(initial);
    queue.push_back((vm, String::new()));

    while let Some((vm, path)) = queue.pop_front() {
        if visited.len() >= MAX_STATES {
            break;
        }

        let location = *vm.location();
        record_location(&mut report, &location);

        match location {
            Location::Menu { .. } => {
                for key in MENU_KEYS {
                    let new_path = append_path(&path, key_label(key));
                    let mut next = vm.clone();

                    match next.press(key) {
                        Ok(_) => {
                            let state = state_of(&next);
                            if visited.insert(state) {
                                queue.push_back((next, new_path));
                            }
                        }
                        Err(error) => record_error(&mut report, &new_path, error),
                    }
                }
            }
            Location::Title { .. } => {
                let new_path = append_path(&path, "Finish");
                let mut next = vm.clone();

                match next.finish_title() {
                    Ok(_) => {
                        let state = state_of(&next);
                        if visited.insert(state) {
                            queue.push_back((next, new_path));
                        }
                    }
                    Err(error) => record_error(&mut report, &new_path, error),
                }
            }
            Location::Stopped => {}
        }
    }

    report
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct State {
    location: Location,
    gprm: [u16; 16],
}

fn state_of(vm: &Vm<'_>) -> State {
    State {
        location: *vm.location(),
        gprm: vm.gprm_array(),
    }
}

fn record_location(report: &mut Report, location: &Location) {
    match location {
        Location::Menu { pgcn, .. } => {
            report.menus_seen.insert(*pgcn);
        }
        Location::Title { title, chapter } => {
            report.chapters_reached.insert((*title, *chapter));
        }
        Location::Stopped => {}
    }
}

fn record_error(report: &mut Report, path: &str, error: VmError) {
    let label = if path.is_empty() {
        "start".to_string()
    } else {
        path.to_string()
    };

    match error {
        VmError::Loop => report.loops.push(label),
        other => report.dead_ends.push(format!("{label}: {other}")),
    }
}

fn append_path(path: &str, label: &str) -> String {
    if path.is_empty() {
        label.to_string()
    } else {
        format!("{path}>{label}")
    }
}

fn key_label(key: Key) -> &'static str {
    match key {
        Key::Up => "Up",
        Key::Down => "Down",
        Key::Left => "Left",
        Key::Right => "Right",
        Key::Enter => "Enter",
    }
}
