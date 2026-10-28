use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(s) = windows_spec(app, area) {
        s.paint(frame, app);
    }
}
use crate::painting::{Frame, Paragraph};
use crate::presentation::layout::Rect;
use crate::{
    app::App, desktop::WindowKind, filesystem::display_name, mouse::Target, windowing::Action,
};

pub(crate) fn windows_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let list = app.modals.window_list()?;
    let profile = app.config.palette;
    let language = app.config.language;
    let rect = crate::turbo::dialog_spec::centered(
        area,
        72,
        (list.ids.len() + 6)
            .max(12)
            .min(usize::from(area.height.saturating_sub(2))) as u16,
    );
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::WindowList)?,
        list.focus,
        language.text("Open windows"),
        Target::Window(Action::Switch),
        Target::Window(Action::Cancel),
    );
    let mut labels = Vec::with_capacity(list.ids.len());
    for (row, id) in list.ids.iter().enumerate() {
        let kind = app.desktop.windows().find(|w| w.id == *id).map(|w| w.kind);
        let width = rect.width - 6;
        let name = match kind {
            Some(WindowKind::Manager) => language.text("File manager").to_owned(),
            Some(WindowKind::Editor) => {
                let Some(editor) = app.desktop.editor(*id) else {
                    continue;
                };
                let path = editor
                    .path
                    .as_ref()
                    .map(|p| display_name(&p.to_string_lossy()))
                    .unwrap_or_else(|| format!("{} {}", language.text("New file"), id));
                let prefix = format!(
                    "{}{}: ",
                    if editor.dirty() { "* " } else { "" },
                    language.text("Editor")
                );
                let available = width
                    .saturating_sub(crate::presentation::text::Span::raw(&prefix).width() as u16);
                format!("{prefix}{}", crate::formatting::tail(&path, available))
            }
            Some(WindowKind::Viewer) => {
                let Some(viewer) = app.desktop.viewer(*id) else {
                    continue;
                };
                let prefix = format!("{}: ", language.text("Viewer"));
                let available = width
                    .saturating_sub(crate::presentation::text::Span::raw(&prefix).width() as u16);
                format!(
                    "{prefix}{}",
                    crate::formatting::tail(
                        &display_name(&viewer.path.to_string_lossy()),
                        available
                    )
                )
            }
            _ => continue,
        };
        let row_area = Rect::new(
            rect.x + 2,
            rect.y + 2 + row.saturating_sub(list.offset) as u16,
            rect.width - 4,
            1,
        );
        let label = format!(
            "{} {}",
            if *id == app.desktop.active().id {
                ">"
            } else {
                " "
            },
            crate::formatting::tail(&name, row_area.width - 2)
        );
        labels.push(label);
        if row < list.offset || row >= list.offset + list.rows {
            continue;
        }
        spec.hit(row_area, Target::Window(Action::Row(*id)));
    }
    let mut control = Control::new(
        0,
        Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, list.rows as u16),
        ControlKind::List {
            items: labels,
            cursor: list.cursor,
            offset: list.offset,
        },
        Target::Window(Action::Switch),
    );
    control.hits.clear();
    spec.control(control);
    spec.caption(
        Paragraph::new(language.text("Switching keeps documents open")).style(profile.dos(0x70)),
        Rect::new(rect.x + 2, rect.bottom() - 4, rect.width - 4, 1),
    );
    for (label, command, focus, x) in [
        ("Switch", Action::Switch, 1, rect.x + 14),
        ("Cancel", Action::Cancel, 2, rect.right() - 30),
    ] {
        let button = Rect::new(x, rect.bottom() - 2, 16, 1);
        spec.control(Control::new(
            focus,
            button,
            ControlKind::Button(language.text(label).into()),
            Target::Window(command),
        ));
    }
    Some(spec)
}
