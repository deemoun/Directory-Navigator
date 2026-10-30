use super::{action, Action};
use crate::{
    app::App,
    focus::Focus,
    mouse::Target,
    painting::Paragraph,
    presentation::layout::Rect,
    turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec},
};
use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
pub fn spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let d = app.modals.recovery()?;
    let r = centered(area, 72, 13);
    let language = app.config.language;
    let mut s = DialogSpec::new(
        r,
        scope(app, Focus::Recovery)?,
        d.focus,
        language.text(" Editor recovery "),
        Target::Recovery(if d.error.is_some() {
            Action::Later
        } else {
            Action::Recover
        }),
        Target::Recovery(Action::Later),
    );
    let source = d.record.path.as_ref().map_or_else(
        || language.text("Untitled document").into(),
        |p| crate::filesystem::display_name(&p.to_string_lossy()),
    );
    let text = if let Some(error) = &d.error {
        format!(
            "{}\n\n{}",
            language.text("Cannot read this checkpoint. You may discard it or leave it for later."),
            crate::filesystem::display_name(error)
        )
    } else {
        format!(
            "{}\n{}\n{} {}\n\n{}",
            language.text("Unsaved work from a previous session"),
            crate::formatting::tail(&source, 66),
            language.text("Checkpoint (UTC):"),
            chrono::DateTime::from_timestamp(d.record.time, 0)
                .map_or_else(|| "?".into(), |t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
            language.text("Recover opens an unsaved copy. The source file stays unchanged.")
        )
    };
    s.caption(
        Paragraph::new(text).style(
            app.config
                .palette
                .for_group(crate::palette::theme::Group::Dialogs, 0x70),
        ),
        Rect::new(r.x + 2, r.y + 2, r.width - 4, 7),
    );
    let busy = app.editor_recovery.as_ref().is_some_and(|m| m.busy());
    for (i, label, a) in [
        (0, "Recover", Action::Recover),
        (1, "Inspect", Action::Inspect),
        (2, "Discard", Action::Discard),
        (3, "Later", Action::Later),
    ] {
        let mut c = Control::new(
            i,
            Rect::new(r.x + 3 + i as u16 * 17, r.bottom() - 3, 15, 1),
            ControlKind::Button(language.text(label).into()),
            Target::Recovery(a),
        );
        c.enabled = !busy && !(d.error.is_some() && matches!(a, Action::Recover | Action::Inspect));
        s.control(c);
    }
    if busy {
        s.close_visible = false;
    }
    Some(s)
}
pub fn key(app: &mut App, key: KeyEvent) {
    crate::dialog_controls::key(app, key, Focus::Recovery);
}
pub fn mouse(app: &mut App, e: MouseEvent, target: Option<Target>) {
    if e.kind == MouseEventKind::Down(MouseButton::Left) {
        if let Some(Target::Recovery(a)) = target {
            action(app, a);
        }
    }
}
