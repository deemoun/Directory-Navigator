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
    #[cfg(feature = "turbo-vision")]
    {
        native::select(current, choices, reverse)
    }
    #[cfg(not(feature = "turbo-vision"))]
    {
        let i = choices.iter().position(|v| *v == current).unwrap_or(0);
        choices[(i + if reverse { choices.len() - 1 } else { 1 }) % choices.len()]
    }
}
#[cfg(feature = "turbo-vision")]
mod native {
    use std::cell::RefCell;
    use turbo_vision::{
        core::{
            event::{Event, KB_SHIFT_TAB, KB_TAB},
            geometry::Rect,
        },
        terminal::Terminal,
        views::{group::Group, GroupLike, View, ViewCore},
    };
    struct Control {
        core: ViewCore,
        value: usize,
    }
    impl View for Control {
        fn core(&self) -> &ViewCore {
            &self.core
        }
        fn core_mut(&mut self) -> &mut ViewCore {
            &mut self.core
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
        fn get_palette(&self) -> Option<turbo_vision::core::palette::Palette> {
            None
        }
        fn can_focus(&self) -> bool {
            true
        }
        fn draw(&mut self, _: &mut Terminal) {}
        fn handle_event(&mut self, _: &mut Event) {}
    }
    struct Traversal {
        choices: Vec<usize>,
        group: Group,
    }
    impl Traversal {
        fn new(choices: &[usize]) -> Self {
            let mut group = Group::new(Rect::new(0, 0, 1, 1));
            for &value in choices {
                group.add(Control {
                    core: ViewCore::new(Rect::new(0, 0, 1, 1)),
                    value,
                });
            }
            group.set_initial_focus();
            Self {
                choices: choices.to_vec(),
                group,
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
            traversal
                .group
                .focused_child()
                .and_then(|v| v.as_any().downcast_ref::<Control>())
                .map_or(current, |v| v.value)
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
