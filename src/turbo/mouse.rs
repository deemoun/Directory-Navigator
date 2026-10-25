//! Native positional routing. Targets remain domain commands; TV Group performs
//! front-to-back hit dispatch for the current frame, including modal surfaces.
use crate::mouse::Target;
use crate::presentation::layout::Rect as HitRect;
use std::{cell::RefCell, rc::Rc};
use turbo_vision::{
    core::{
        event::{Event, EventType},
        geometry::{Point, Rect},
        palette::Palette,
    },
    terminal::Terminal,
    views::{group::Group, GroupLike, View, ViewCore},
};
struct Hit {
    core: ViewCore,
    target: Target,
    mailbox: Rc<RefCell<Option<Target>>>,
}
impl View for Hit {
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
    fn get_palette(&self) -> Option<Palette> {
        None
    }
    fn can_focus(&self) -> bool {
        false
    }
    fn draw(&mut self, _: &mut Terminal) {}
    fn handle_event(&mut self, event: &mut Event) {
        if event.what == EventType::MouseDown {
            *self.mailbox.borrow_mut() = Some(self.target);
            event.clear();
        }
    }
}
pub(super) struct Router {
    group: Group,
    regions: Vec<(HitRect, Target)>,
    mailbox: Rc<RefCell<Option<Target>>>,
}
impl Default for Router {
    fn default() -> Self {
        Self {
            group: Group::new(Rect::new(0, 0, 0, 0)),
            regions: Vec::new(),
            mailbox: Rc::new(RefCell::new(None)),
        }
    }
}
impl Router {
    pub(super) fn target(
        &mut self,
        regions: &[(HitRect, Target)],
        x: u16,
        y: u16,
    ) -> Option<Target> {
        if x > i16::MAX as u16 || y > i16::MAX as u16 {
            return None;
        }
        if self.regions != regions {
            self.regions = regions.to_vec();
            let max = i16::MAX as u16;
            let width = regions
                .iter()
                .map(|(r, _)| r.right().min(max))
                .max()
                .unwrap_or(0);
            let height = regions
                .iter()
                .map(|(r, _)| r.bottom().min(max))
                .max()
                .unwrap_or(0);
            self.group = Group::new(Rect::new(0, 0, width as i16, height as i16));
            for (rect, target) in regions {
                self.group.add(Hit {
                    core: ViewCore::new(Rect::new(
                        rect.x.min(max) as i16,
                        rect.y.min(max) as i16,
                        rect.right().min(max) as i16,
                        rect.bottom().min(max) as i16,
                    )),
                    target: *target,
                    mailbox: self.mailbox.clone(),
                });
            }
        }
        *self.mailbox.borrow_mut() = None;
        // This is a positional query. The original mouse payload (wheel, shift,
        // drag and click timing) is delivered unchanged to domain controllers.
        self.group.handle_event(&mut Event::mouse(
            EventType::MouseDown,
            Point::new(x as i16, y as i16),
            1,
            false,
        ));
        self.mailbox.borrow_mut().take()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_hit_order_modal_surface_and_replaced_regions_are_exact() {
        let mut r = Router::default();
        let regions = vec![
            (HitRect::new(0, 0, 80, 25), Target::Panel(0)),
            (HitRect::new(4, 3, 20, 1), Target::Entry(0, 3)),
            (HitRect::new(2, 2, 30, 10), Target::MenuSurface),
            (HitRect::new(3, 3, 28, 1), Target::MenuItem(0, 1)),
        ];
        assert_eq!(r.target(&regions, 5, 3), Some(Target::MenuItem(0, 1)));
        assert_eq!(r.target(&regions, 5, 4), Some(Target::MenuSurface));
        assert_eq!(r.target(&regions, 79, 24), Some(Target::Panel(0)));
        assert_eq!(r.target(&regions, 80, 25), None);
        assert_eq!(r.target(&regions[..2], 5, 3), Some(Target::Entry(0, 3)));
        assert_eq!(r.target(&[], 5, 3), None);
        assert_eq!(r.target(&regions, u16::MAX, 0), None);
    }
}
