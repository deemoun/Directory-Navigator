use super::*;
use crate::{
    dialog_theme as theme,
    focus::Focus,
    mouse::Target,
    painting::Paragraph,
    presentation::layout::Rect,
    turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec},
};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
pub(crate) fn spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let state = &app.transfers;
    let r = crate::tool_windows::bounds(app, area, 90, 20);
    let (x, w) = (r.x + 2, r.width.saturating_sub(4));
    let rows = usize::from(r.height.saturating_sub(12)).max(1);
    let cursor = state
        .entries
        .iter()
        .position(|e| Some(e.id) == state.selected)
        .unwrap_or(0);
    let offset = cursor.saturating_sub(rows.saturating_sub(1));
    let lang = app.config.language;
    let mut s = DialogSpec::new(
        r,
        scope(app, Focus::Transfers)?,
        state.focus,
        lang.text(" Transfer Center "),
        Target::Transfer(Action::Details),
        Target::Transfer(Action::Close),
    );
    let style = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    s.caption(
        Paragraph::new(format!(
            "{}: {} · {}: {}{}",
            lang.text("Active"),
            usize::from(state.active.is_some()),
            lang.text("Queued"),
            state
                .entries
                .iter()
                .filter(|e| matches!(e.status, Status::Preparing | Status::Queued)
                    && Some(e.id) != state.active)
                .count(),
            if state.paused {
                format!(" · {}", lang.text("Queue paused"))
            } else {
                String::new()
            }
        ))
        .style(style(theme::TEXT)),
        Rect::new(x, r.y + 1, w, 1),
    );
    let items = state
        .entries
        .iter()
        .map(|e| {
            format!(
                "#{} {} · {} · {} → {}",
                e.id,
                lang.text(if e.plan.kind == operations::Kind::Copy {
                    "Copy"
                } else {
                    "Move"
                }),
                lang.text(e.status.caption()),
                crate::formatting::tail(&e.source_summary(), w.saturating_sub(28) / 2),
                crate::formatting::tail(
                    &crate::filesystem::display_name(&e.plan.destination.to_string_lossy()),
                    w.saturating_sub(28) / 2
                )
            )
        })
        .collect();
    let mut list = Control::new(
        0,
        Rect::new(x, r.y + 3, w, rows as u16),
        ControlKind::List {
            items,
            cursor,
            offset,
        },
        Target::Transfer(Action::Details),
    );
    list.enter_action = true;
    list.hits = state
        .entries
        .iter()
        .skip(offset)
        .take(rows)
        .enumerate()
        .map(|(row, e)| {
            (
                Rect::new(x, r.y + 3 + row as u16, w, 1),
                Target::Transfer(Action::Select(e.id)),
            )
        })
        .collect();
    s.control(list);
    if let Some(e) = state.selected() {
        let now = state.now.expect("transfer clock initialized at submission");
        s.native(
            crate::turbo::widgets::transfer_progress(
                e.percent(),
                w,
                (e.elapsed(now).as_millis() / 120).min(u128::from(u64::MAX)) as u64,
            ),
            Rect::new(x, r.bottom().saturating_sub(9), w, 1),
        );
        let percent = e.percent().map_or_else(|| "…".into(), |p| format!("{p}%"));
        let rate = e
            .throughput(now)
            .map_or_else(|| "—".into(), |r| format!("{:.1} KiB/s", r / 1024.0));
        let eta = e
            .remaining(now)
            .map_or_else(|| "—".into(), |d| format!("{} s", d.as_secs()));
        let text = format!(
            "{} · {} · {} {}\n{}: {} · {}: {} · {}: {}\n{}: {} · {}: {} · {}: {}\n{}\n{}",
            lang.text(e.status.caption()),
            lang.text(e.progress.phase.caption()),
            percent,
            if e.progress.total_bytes.is_some() {
                lang.text("estimated")
            } else {
                ""
            },
            lang.text("Items"),
            e.progress.files,
            lang.text("Skipped"),
            e.progress.skipped,
            lang.text("Bytes"),
            e.progress.bytes,
            lang.text("Elapsed"),
            e.elapsed(now).as_secs(),
            lang.text("Speed"),
            rate,
            lang.text("Remaining"),
            eta,
            crate::formatting::tail(
                &crate::filesystem::display_name(&e.progress.path.to_string_lossy()),
                w
            ),
            e.error
                .as_ref()
                .map(|s| lang.message(s))
                .unwrap_or_else(|| lang
                    .text("Enter: details · F1: Help · Esc: close window")
                    .into())
        );
        s.caption(
            Paragraph::new(text).style(style(theme::TEXT)),
            Rect::new(x, r.bottom().saturating_sub(8), w, 5),
        );
    } else {
        s.caption(
            Paragraph::new(lang.text("No transfers in this session")).style(style(theme::TEXT)),
            Rect::new(x, r.y + 4, w, 1),
        );
    }
    let controls = [
        (
            1,
            "Cancel selected",
            Action::Cancel,
            state
                .selected()
                .is_some_and(|e| !e.status.terminal() && e.status != Status::Cancelling),
        ),
        (2, "Resume queue", Action::Resume, state.paused),
        (
            3,
            "Clear finished",
            Action::Clear,
            state.entries.iter().any(|e| e.status.terminal()),
        ),
        (4, "Close", Action::Close, true),
    ];
    let labels = controls.map(|(_, caption, _, _)| lang.text(caption));
    let bounds = theme::button_row(Rect::new(x, r.bottom() - 2, w, 1), &labels, 12, 1);
    for ((id, _, action, enabled), (caption, bounds)) in
        controls.into_iter().zip(labels.into_iter().zip(bounds))
    {
        let mut control = Control::new(
            id,
            bounds,
            ControlKind::Button(caption.into()),
            Target::Transfer(action),
        );
        control.enabled = enabled;
        s.control(control);
    }
    Some(s)
}
pub(crate) fn key(app: &mut App, key: KeyEvent) {
    if crate::dialog_controls::key(app, key, Focus::Transfers) {
        return;
    }
    if key.code == KeyCode::F(1) {
        action(app, Action::Help);
        return;
    }
    if app.transfers.focus == 0 {
        let cursor = app
            .transfers
            .entries
            .iter()
            .position(|e| Some(e.id) == app.transfers.selected)
            .unwrap_or(0);
        let rows = app.transfers.rows.max(1);
        if let Some(next) =
            crate::turbo::widgets::list_key(cursor, app.transfers.entries.len(), rows, key.code)
        {
            app.transfers.selected = app.transfers.entries.get(next).map(|e| e.id);
        }
    }
}
pub(crate) fn mouse(app: &mut App, event: MouseEvent, target: Option<Target>) {
    if event.kind == MouseEventKind::Down(MouseButton::Left) {
        if let Some(Target::Transfer(a)) = target {
            action(app, a);
        }
    } else if matches!(
        event.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        app.transfers.focus = 0;
        key(
            app,
            KeyEvent::new(
                if event.kind == MouseEventKind::ScrollUp {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                },
                event.modifiers,
            ),
        );
    }
}
pub(crate) fn quit_spec(app: &App, r: Rect) -> Option<DialogSpec<'_>> {
    let lang = app.config.language;
    let mut s = DialogSpec::new(
        r,
        scope(app, Focus::Message)?,
        app.transfers.quit_focus,
        lang.text(" Transfers pending "),
        Target::Transfer(Action::QuitReturn),
        Target::Transfer(Action::QuitReturn),
    );
    s.caption(
        Paragraph::new(
            lang.text("Transfers are still pending. Choose how to finish this session."),
        )
        .style(
            app.config
                .palette
                .for_group(crate::palette::theme::Group::Dialogs, theme::TEXT),
        ),
        Rect::new(r.x + 2, r.y + 2, r.width - 4, 2),
    );
    let actions = [
        ("Wait and quit", Action::QuitWait),
        ("Cancel transfers and quit", Action::QuitCancel),
        ("Return to DN", Action::QuitReturn),
    ];
    let labels = actions.map(|(caption, _)| lang.text(caption));
    let bounds = theme::button_row(
        Rect::new(r.x + 2, r.bottom() - 3, r.width - 4, 1),
        &labels,
        12,
        1,
    );
    for (index, ((_, a), (label, bounds))) in actions
        .into_iter()
        .zip(labels.into_iter().zip(bounds))
        .enumerate()
    {
        s.control(Control::new(
            index,
            bounds,
            ControlKind::Button(label.into()),
            Target::Transfer(a),
        ));
    }
    Some(s)
}
