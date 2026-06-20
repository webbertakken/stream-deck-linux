//! Turn raw button-state snapshots into discrete press/release events.

/// What happened to a single key between two state snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    Pressed,
    Released,
}

/// A change on one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: u8,
    pub kind: KeyEventKind,
}

/// Compute the press/release events between a previous and current snapshot.
///
/// Snapshots are `true` == pressed, indexed by hardware key. Lengths are
/// expected to match; any trailing keys in the longer slice are treated as
/// released so a short read never invents phantom presses.
pub fn diff_states(previous: &[bool], current: &[bool]) -> Vec<KeyEvent> {
    let len = previous.len().max(current.len());
    let mut events = Vec::new();
    for key in 0..len {
        let was = previous.get(key).copied().unwrap_or(false);
        let now = current.get(key).copied().unwrap_or(false);
        match (was, now) {
            (false, true) => events.push(KeyEvent {
                key: key as u8,
                kind: KeyEventKind::Pressed,
            }),
            (true, false) => events.push(KeyEvent {
                key: key as u8,
                kind: KeyEventKind::Released,
            }),
            _ => {}
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_single_press_and_release() {
        let before = [false, false, false];
        let pressed = [false, true, false];
        let released = [false, false, false];

        assert_eq!(
            diff_states(&before, &pressed),
            vec![KeyEvent {
                key: 1,
                kind: KeyEventKind::Pressed
            }]
        );
        assert_eq!(
            diff_states(&pressed, &released),
            vec![KeyEvent {
                key: 1,
                kind: KeyEventKind::Released
            }]
        );
    }

    #[test]
    fn no_change_yields_no_events() {
        let states = [true, false, true];
        assert!(diff_states(&states, &states).is_empty());
    }

    #[test]
    fn reports_multiple_simultaneous_changes() {
        let before = [true, false, false];
        let after = [false, true, false];
        let events = diff_states(&before, &after);
        assert_eq!(events.len(), 2);
        assert!(events.contains(&KeyEvent {
            key: 0,
            kind: KeyEventKind::Released
        }));
        assert!(events.contains(&KeyEvent {
            key: 1,
            kind: KeyEventKind::Pressed
        }));
    }

    #[test]
    fn shorter_current_snapshot_is_treated_as_released() {
        let before = [true, true];
        let after = [true];
        assert_eq!(
            diff_states(&before, &after),
            vec![KeyEvent {
                key: 1,
                kind: KeyEventKind::Released
            }]
        );
    }
}
