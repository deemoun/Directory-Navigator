//! Shared structural and foreground checks. Commands and close guards stay in controllers.
use crate::{
    app::App,
    desktop::WindowId,
    layout::FrameContext,
    modals::{ModalInfo, ModalOwner},
    windowing::geometry::{Surface, SurfaceId, SurfaceKey},
};
pub(crate) struct Gate<'a>(&'a App);
impl<'a> Gate<'a> {
    pub fn new(app: &'a App) -> Self {
        Self(app)
    }
    pub fn live(&self, owner: ModalOwner) -> bool {
        match owner {
            ModalOwner::Workspace => true,
            ModalOwner::Document(id) => self.0.desktop.contains(id),
        }
    }
    fn interactive(&self) -> bool {
        !self.0.output_visible
            && !self.0.screensaver.active
            && !(self.0.exit_pending && self.0.quit_review.is_some())
            && !self.0.desktop.native().manipulating()
    }
    pub fn document(&self, id: WindowId, revision: Option<u64>) -> bool {
        self.interactive()
            && self.live(ModalOwner::Document(id))
            && self.0.desktop.active().id == id
            && revision.is_none_or(|r| r == self.0.layout_revision)
            && self.0.modal_top().is_none()
    }
    pub fn modal(&self, modal: ModalInfo) -> bool {
        self.interactive()
            && self.live(modal.owner)
            && self
                .0
                .modal_top()
                .is_some_and(|m| m.id == modal.id && m.owner == modal.owner)
    }
    pub fn modal_at(&self, modal: ModalInfo, revision: u64) -> bool {
        self.modal(modal) && revision == self.0.layout_revision
    }
    pub fn frame(&self, frame: Option<FrameContext>) -> bool {
        self.interactive()
            && frame.is_none_or(|f| {
                self.0.desktop.contains(f.owner)
                    && f.owner == self.0.desktop.active().id
                    && f.revision == self.0.layout_revision
                    && f.modal == self.0.modal_top().map(|m| m.id)
            })
    }
    pub fn surface(&self, id: SurfaceId) -> Option<Surface> {
        if !self.interactive() {
            return None;
        }
        let surface = self
            .0
            .desktop
            .native()
            .surface(id.key)
            .filter(|s| s.id == id)?;
        let owner = match id.key {
            SurfaceKey::Document(w) | SurfaceKey::Page(w, _) => ModalOwner::Document(w),
            SurfaceKey::Modal(m) | SurfaceKey::ModalPage(m, _) => {
                self.0.modal_infos().into_iter().find(|x| x.id == m)?.owner
            }
        };
        if !self.live(owner) {
            return None;
        }
        if self.0.modal_top().is_some()
            || self
                .0
                .desktop
                .native()
                .top_surface()
                .is_some_and(|s| matches!(s.id.key, SurfaceKey::Page(..)))
        {
            if self
                .0
                .desktop
                .native()
                .top_surface()
                .is_none_or(|top| top.id != id)
            {
                return None;
            }
            if self.0.modal_top().is_some()
                && matches!(id.key, SurfaceKey::Document(_) | SurfaceKey::Page(..))
            {
                return None;
            }
        }
        Some(surface)
    }
}
