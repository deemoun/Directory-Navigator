//! Shared list mechanics; callers explicitly preserve their existing extent and
//! empty-space policies, row identity, key mapping and click interval.
use std::time::{Duration, Instant};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListViewport {
    pub cursor: usize,
    pub offset: usize,
}
impl ListViewport {
    pub fn keep_visible(
        mut self,
        rows: usize,
        last_item: Option<usize>,
        last_offset: Option<usize>,
    ) -> Self {
        if let Some(last) = last_item {
            self.cursor = self.cursor.min(last);
        }
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + rows {
            self.offset = self.cursor + 1 - rows;
        }
        if let Some(last) = last_offset {
            self.offset = self.offset.min(last);
        }
        self
    }
    pub fn fit(
        cursor: &mut usize,
        offset: &mut usize,
        rows: usize,
        last_item: Option<usize>,
        last_offset: Option<usize>,
    ) {
        let result = Self {
            cursor: *cursor,
            offset: *offset,
        }
        .keep_visible(rows, last_item, last_offset);
        *cursor = result.cursor;
        *offset = result.offset;
    }
}
pub fn wheel_delta(kind: crossterm::event::MouseEventKind) -> isize {
    match kind {
        crossterm::event::MouseEventKind::ScrollUp => -3,
        crossterm::event::MouseEventKind::ScrollDown => 3,
        _ => 0,
    }
}
#[derive(Clone, Copy)]
pub struct ClickTracker {
    pub interval: Duration,
    pub inclusive: bool,
}
impl ClickTracker {
    pub const STRICT: Self = Self {
        interval: Duration::from_millis(400),
        inclusive: false,
    };
    pub const DISCOVERY: Self = Self {
        interval: Duration::from_millis(400),
        inclusive: true,
    };
    pub const FILE_HISTORY: Self = Self {
        interval: Duration::from_millis(500),
        inclusive: true,
    };
    pub fn matches<T: PartialEq>(
        self,
        previous: Option<&(T, Instant)>,
        current: &T,
        now: Instant,
    ) -> bool {
        previous.is_some_and(|(last, time)| {
            last == current
                && if self.inclusive {
                    now.duration_since(*time) <= self.interval
                } else {
                    now.duration_since(*time) < self.interval
                }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn viewport_matches_legacy_clamping_for_every_small_list_and_page() {
        for count in 0usize..20 {
            for rows in 1usize..10 {
                for cursor in 0usize..25 {
                    for offset in 0usize..25 {
                        for limit in [
                            None,
                            Some(count.saturating_sub(1)),
                            Some(count.saturating_sub(rows)),
                        ] {
                            let expected_cursor = cursor.min(count.saturating_sub(1));
                            let mut expected_offset = offset;
                            if expected_cursor < expected_offset {
                                expected_offset = expected_cursor;
                            }
                            if expected_cursor >= expected_offset + rows {
                                expected_offset = expected_cursor + 1 - rows;
                            }
                            if let Some(last) = limit {
                                expected_offset = expected_offset.min(last);
                            }
                            assert_eq!(
                                ListViewport { cursor, offset }.keep_visible(
                                    rows,
                                    Some(count.saturating_sub(1)),
                                    limit
                                ),
                                ListViewport {
                                    cursor: expected_cursor,
                                    offset: expected_offset
                                }
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn archive_parent_row_and_copy_history_unclamped_cursor_remain_explicit() {
        assert_eq!(
            ListViewport {
                cursor: 10,
                offset: 0
            }
            .keep_visible(3, Some(5), Some(5)),
            ListViewport {
                cursor: 5,
                offset: 3
            }
        );
        assert_eq!(
            ListViewport {
                cursor: 10,
                offset: 0
            }
            .keep_visible(3, None, Some(2)),
            ListViewport {
                cursor: 10,
                offset: 2
            }
        );
        assert_eq!(
            ListViewport {
                cursor: 2,
                offset: 5
            }
            .keep_visible(0, None, None)
            .offset,
            2
        );
        assert_eq!(
            ListViewport {
                cursor: 2,
                offset: 0
            }
            .keep_visible(0, None, None)
            .offset,
            3
        );
    }
    #[test]
    fn click_identity_and_inclusive_boundaries_are_preserved() {
        let now = Instant::now();
        let last = ("row", now);
        assert!(ClickTracker::STRICT.matches(
            Some(&last),
            &"row",
            now + Duration::from_millis(399)
        ));
        assert!(!ClickTracker::STRICT.matches(
            Some(&last),
            &"row",
            now + Duration::from_millis(400)
        ));
        assert!(ClickTracker::DISCOVERY.matches(
            Some(&last),
            &"row",
            now + Duration::from_millis(400)
        ));
        assert!(ClickTracker::FILE_HISTORY.matches(
            Some(&last),
            &"row",
            now + Duration::from_millis(500)
        ));
        assert!(!ClickTracker::FILE_HISTORY.matches(Some(&last), &"other", now));
    }
}
