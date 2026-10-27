use crate::{
    app::App, mouse::Target, painting::Frame, presentation::layout::Rect, welcome::Action,
};

pub(super) fn draw(frame: &mut impl Frame, app: &App, _area: Rect) {
    let Some(welcome) = app.welcome.modals.welcome() else {
        return;
    };
    frame.regions().clear();
    if let Some(scene) = welcome.page.scene() {
        let mut scene = scene.clone();
        let rect = welcome.page.bounds();
        scene.regions = welcome.links.clone();
        scene.regions.push((
            Rect::new(rect.x + 2, rect.y, 3, 1),
            Target::Welcome(Action::Start),
        ));
        frame.compose(scene);
        frame.native_shadow(rect);
        super::overlays::help_status(frame, app, true);
    }
}
