use crate::painting::{Frame, Paragraph};
use crate::{
    app::App, desktop::WindowKind, filesystem::display_name, mouse::Target, windowing::Action,
};
use ratatui::layout::Rect;

pub(super) fn draw(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let Some(list) = &app.window_list else { return };
    let profile = app.config.palette;
    let language = app.config.language;
    let rect =
        super::dialog_style::window(frame, profile, area, 72, 12, language.text("Open windows"));
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::Window(Action::Cancel),
    );
    for (row, id) in list.ids.iter().enumerate() {
        let kind = app.desktop.windows().find(|w| w.id == *id).map(|w| w.kind);
        let width = rect.width - 6;
        let name = match kind {
            Some(WindowKind::Manager) => language.text("File manager").to_owned(),
            Some(WindowKind::Editor) => {
                let Some(editor) = app.desktop.editor() else {
                    continue;
                };
                let path = editor
                    .path
                    .as_ref()
                    .map(|p| display_name(&p.to_string_lossy()))
                    .unwrap_or_else(|| language.text("New file").into());
                let prefix = format!(
                    "{}{}: ",
                    if editor.dirty() { "* " } else { "" },
                    language.text("Editor")
                );
                let available =
                    width.saturating_sub(ratatui::text::Span::raw(&prefix).width() as u16);
                format!("{prefix}{}", crate::formatting::tail(&path, available))
            }
            Some(WindowKind::Viewer) => {
                let Some(viewer) = app.desktop.viewer() else {
                    continue;
                };
                let prefix = format!("{}: ", language.text("Viewer"));
                let available =
                    width.saturating_sub(ratatui::text::Span::raw(&prefix).width() as u16);
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
        let row_area = Rect::new(rect.x + 2, rect.y + 2 + row as u16, rect.width - 4, 1);
        let label = format!(
            "{} {}",
            if *id == app.desktop.active().id {
                ">"
            } else {
                " "
            },
            crate::formatting::tail(&name, row_area.width - 2)
        );
        frame.render_widget(
            Paragraph::new(label).style(profile.dos(if row == list.cursor { 0x30 } else { 0x70 })),
            row_area,
        );
        app.mouse
            .regions
            .push((row_area, Target::Window(Action::Row(*id))));
    }
    frame.render_widget(
        Paragraph::new(language.text("Switching keeps documents open")).style(profile.dos(0x70)),
        Rect::new(rect.x + 2, rect.bottom() - 4, rect.width - 4, 1),
    );
    for (label, command, focus, x) in [
        ("Switch", Action::Switch, 1, rect.x + 14),
        ("Cancel", Action::Cancel, 2, rect.right() - 30),
    ] {
        let button = Rect::new(x, rect.bottom() - 2, 16, 1);
        super::dialog_style::button(
            frame,
            profile,
            button,
            language.text(label),
            list.focus == focus,
        );
        app.mouse.regions.push((button, Target::Window(command)));
    }
}
