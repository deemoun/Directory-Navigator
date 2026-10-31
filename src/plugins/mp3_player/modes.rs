//! Playback order is independent of the editable, visible queue.
use std::hash::BuildHasher;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Repeat {
    #[default]
    Off,
    One,
    All,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Mode {
    pub shuffle: bool,
    pub repeat: Repeat,
}
pub(super) struct Order {
    pub mode: Mode,
    source: Vec<u64>,
    sequence: Vec<u64>,
    random: u64,
}
impl Default for Order {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            source: Vec::new(),
            sequence: Vec::new(),
            random: std::collections::hash_map::RandomState::new()
                .hash_one(std::time::Instant::now())
                .max(1),
        }
    }
}
impl Order {
    fn index(&mut self, bound: usize) -> usize {
        let bound = bound as u64;
        let limit = u64::MAX - u64::MAX % bound;
        loop {
            self.random ^= self.random << 13;
            self.random ^= self.random >> 7;
            self.random ^= self.random << 17;
            if self.random < limit {
                return (self.random % bound) as usize;
            }
        }
    }
    pub fn sync(&mut self, ids: Vec<u64>, current: Option<u64>) {
        if self.source != ids {
            self.rebuild(ids, current);
        }
    }
    pub fn apply(&mut self, mode: Mode, ids: Vec<u64>, current: Option<u64>) {
        let changed = self.mode.shuffle != mode.shuffle;
        self.mode = mode;
        if changed || self.source != ids {
            self.rebuild(ids, current);
        }
    }
    pub fn rebuild(&mut self, ids: Vec<u64>, current: Option<u64>) {
        self.source = ids;
        self.sequence = self.source.clone();
        if self.mode.shuffle {
            if let Some(index) = self.sequence.iter().position(|id| Some(*id) == current) {
                self.sequence.swap(0, index);
            }
            // Keep the selected track first; shuffle the remaining entries once
            // per queue/mode change. Repeat All repeats this same cycle.
            for i in (2..self.sequence.len()).rev() {
                let j = 1 + self.index(i);
                self.sequence.swap(i, j);
            }
        }
    }
    pub fn neighbor(&self, current: Option<u64>, forward: bool, automatic: bool) -> Option<u64> {
        let index = self.sequence.iter().position(|id| Some(*id) == current)?;
        if automatic && self.mode.repeat == Repeat::One {
            return current;
        }
        if forward {
            self.sequence
                .get(index + 1)
                .copied()
                .or_else(|| (self.mode.repeat == Repeat::All).then(|| self.sequence[0]))
        } else {
            index
                .checked_sub(1)
                .and_then(|n| self.sequence.get(n))
                .copied()
                .or_else(|| {
                    (self.mode.repeat == Repeat::All).then(|| *self.sequence.last().unwrap())
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeat_one_affects_eof_only_and_all_wraps_both_directions() {
        let mut order = Order::default();
        order.sync(vec![1, 2, 3], Some(1));
        assert_eq!(order.neighbor(Some(1), false, false), None);
        assert_eq!(order.neighbor(Some(3), true, true), None);
        order.apply(
            Mode {
                shuffle: false,
                repeat: Repeat::One,
            },
            vec![1, 2, 3],
            Some(1),
        );
        assert_eq!(order.neighbor(Some(1), true, true), Some(1));
        assert_eq!(order.neighbor(Some(1), true, false), Some(2));
        order.apply(
            Mode {
                shuffle: false,
                repeat: Repeat::All,
            },
            vec![1, 2, 3],
            Some(1),
        );
        assert_eq!(order.neighbor(Some(3), true, true), Some(1));
        assert_eq!(order.neighbor(Some(1), false, false), Some(3));
    }
    #[test]
    fn shuffle_visits_each_identity_once_and_previous_retraces_without_reordering_queue() {
        let mut order = Order {
            random: 12345,
            ..Default::default()
        };
        let source = (0..20).collect::<Vec<_>>();
        order.apply(
            Mode {
                shuffle: true,
                repeat: Repeat::Off,
            },
            source.clone(),
            Some(7),
        );
        let mut heard = vec![7];
        while let Some(id) = order.neighbor(heard.last().copied(), true, true) {
            heard.push(id);
        }
        assert_eq!(heard.len(), 20);
        let mut sorted = heard.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, source);
        for pair in heard.windows(2) {
            assert_eq!(order.neighbor(Some(pair[1]), false, false), Some(pair[0]));
        }
        assert_eq!(order.source, source);
        let cycle = order.sequence.clone();
        order.apply(
            Mode {
                shuffle: true,
                repeat: Repeat::All,
            },
            source.clone(),
            Some(7),
        );
        assert_eq!(order.sequence, cycle);
        order.sync(vec![1, 7, 21], Some(7));
        assert_eq!(order.sequence[0], 7);
        let mut sorted = order.sequence.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![1, 7, 21]);
        order.apply(Mode::default(), source.clone(), Some(7));
        assert_eq!(order.sequence, source);
    }
    #[test]
    fn empty_single_and_retired_current_have_no_invalid_neighbors() {
        let mut order = Order::default();
        order.apply(
            Mode {
                shuffle: true,
                repeat: Repeat::All,
            },
            vec![],
            None,
        );
        assert_eq!(order.neighbor(None, true, true), None);
        order.sync(vec![42], Some(42));
        assert_eq!(order.neighbor(Some(42), true, true), Some(42));
        assert_eq!(order.neighbor(Some(8), true, true), None);
    }
}
