//! Control bindings use model indices; Turbo Vision owns keyboard traversal.
//! Mouse/mnemonic changes are synchronized before traversal, so opening another
//! dialog or restoring a parent never reuses stale toolkit focus.
pub fn cycle(current: usize, count: usize, reverse: bool) -> usize {
    select(current, &(0..count).collect::<Vec<_>>(), reverse)
}
pub fn select(current: usize, choices: &[usize], reverse: bool) -> usize {
    if choices.is_empty() {
        return current;
    }
    native::select(current, choices, reverse)
}
mod native {
    use std::cell::RefCell;
    use turbo_vision::{
        core::{
            event::{Event, KB_SHIFT_TAB, KB_TAB},
            geometry::Rect,
        },
        views::{button::Button, group::Group, GroupLike, View},
    };
    struct Traversal {
        choices: Vec<usize>,
        group: Group,
        ids: Vec<turbo_vision::views::ViewId>,
    }
    impl Traversal {
        fn new(choices: &[usize]) -> Self {
            let mut group = Group::new(Rect::new(0, 0, 1, 1));
            let mut ids = Vec::new();
            for _ in choices {
                let mut button = Button::new(Rect::new(0, 0, 4, 2), "", 0xf100, false);
                button.set_selectable(true);
                ids.push(group.add(button));
            }
            group.set_initial_focus();
            Self {
                choices: choices.to_vec(),
                group,
                ids,
            }
        }
    }
    thread_local! { static TRAVERSAL: RefCell<Traversal> = RefCell::new(Traversal::new(&[])); }
    pub(super) fn select(current: usize, choices: &[usize], reverse: bool) -> usize {
        TRAVERSAL.with(|cached| {
            let mut traversal = cached.borrow_mut();
            if traversal.choices != choices {
                *traversal = Traversal::new(choices);
            }
            let index = choices.iter().position(|v| *v == current).unwrap_or(0);
            traversal.group.set_focus_to(index);
            traversal
                .group
                .handle_event(&mut Event::keyboard(if reverse {
                    KB_SHIFT_TAB
                } else {
                    KB_TAB
                }));
            let selected = traversal.group.focused_view_id();
            traversal
                .ids
                .iter()
                .position(|id| Some(*id) == selected)
                .and_then(|i| choices.get(i).copied())
                .unwrap_or(current)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traversal_preserves_disabled_choices_wrap_and_external_focus() {
        assert_eq!(select(3, &[0, 3, 8], false), 8);
        assert_eq!(select(8, &[0, 3, 8], false), 0);
        assert_eq!(select(0, &[0, 3, 8], true), 8);
        assert_eq!(select(3, &[0, 3, 8], true), 0);
        assert_eq!(select(40, &[], false), 40);
        assert_eq!(select(5, &[5], false), 5);
        // New dialog then restored parent's mouse/mnemonic focus.
        assert_eq!(cycle(1, 2, false), 0);
        assert_eq!(select(8, &[0, 3, 8], true), 3);
        assert_eq!(cycle(0, 0, true), 0);
    }
}
