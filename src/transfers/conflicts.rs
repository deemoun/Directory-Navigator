//! Workspace-owned prompts are identities into the active worker, never closures.
use super::*;
use crate::{
    focus::Focus,
    mouse::Target,
    operations::conflict::{Conflict, Decision},
    presentation::layout::Rect,
    turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec},
};

pub(crate) fn conflict_dialog(app: &App) -> bool {
    app.transfers
        .conflict_prompt
        .is_some_and(|(id, _)| app.modal_top().is_some_and(|m| m.id == id))
}
fn current(app: &App) -> Option<(TransferId, Conflict)> {
    let job = app.job.as_ref()?;
    let id = job.transfer?;
    if app.transfers.active != Some(id) || job.cancel.load(Ordering::Relaxed) {
        return None;
    }
    Some((id, job.conflicts.pending()?))
}
pub(super) fn poll(app: &mut App) {
    let pending = current(app);
    if let Some((modal, id)) = app.transfers.conflict_prompt {
        if pending.as_ref().is_none_or(|(active, _)| *active != id) {
            app.modals.remove_message(modal);
            app.transfers.conflict_prompt = None;
        }
    }
    if let Some((id, _)) = pending {
        if let Some(entry) = app.transfers.entries.iter_mut().find(|e| e.id == id) {
            entry.status = Status::Waiting;
        }
        if app.modal_top().is_none()
            && matches!(
                crate::focus::current(app),
                Focus::Manager | Focus::Transfers
            )
        {
            show(app);
        }
    }
}
pub(super) fn show(app: &mut App) {
    if app.transfers.conflict_prompt.is_some() {
        return;
    }
    let Some((id, _)) = current(app) else {
        return;
    };
    app.transfers.conflict_focus = 1;
    app.push_owned_dialog(crate::modals::ModalOwner::Workspace, crate::dialog::Dialog {
        title: "Replace existing file?",
        // Layout reserves enough room for both native paths, metadata and two button rows.
        message: "                                                                    \n\n\n\n\n\n\n\n\n\n\n\n\n\n".into(),
    });
    app.transfers.conflict_prompt = app.modal_top().map(|m| (m.id, id));
}
pub(super) fn answer(app: &mut App, id: TransferId, decision: Decision) {
    if !conflict_dialog(app)
        || app
            .transfers
            .conflict_prompt
            .is_none_or(|(_, owner)| owner != id)
        || current(app).is_none_or(|(owner, _)| owner != id)
    {
        return;
    }
    if let Some(job) = &app.job {
        job.conflicts.answer(decision);
    }
    if let Some((modal, _)) = app.transfers.conflict_prompt.take() {
        app.modals.remove_message(modal);
    }
    if let Some(entry) = app.transfers.entries.iter_mut().find(|e| e.id == id) {
        entry.status = Status::Running;
    }
}
pub(crate) fn conflict_spec(app: &App, r: Rect) -> Option<DialogSpec<'_>> {
    let (id, conflict) = current(app)?;
    let lang = app.config.language;
    let mut s = DialogSpec::new(
        r,
        scope(app, Focus::Message)?,
        app.transfers.conflict_focus,
        lang.text("Replace existing file?"),
        Target::Transfer(Action::Conflict(id, Decision::Skip)),
        Target::Transfer(Action::Conflict(id, Decision::Cancel)),
    );
    let date = |t: Option<std::time::SystemTime>| {
        t.map(|t| {
            chrono::DateTime::<chrono::Local>::from(t)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
    };
    let w = r.width.saturating_sub(4);
    let text = format!(
        "{}:\n{}\n{} · {}\n\n{}:\n{}\n{} · {}",
        lang.text("Source"),
        crate::formatting::tail(
            &crate::filesystem::display_name(
                conflict
                    .source_label
                    .as_deref()
                    .unwrap_or(&conflict.source.to_string_lossy())
            ),
            w
        ),
        conflict.source_size,
        date(conflict.source_modified),
        lang.text("Destination"),
        crate::formatting::tail(
            &crate::filesystem::display_name(&conflict.destination.to_string_lossy()),
            w
        ),
        conflict.destination_size,
        date(conflict.destination_modified)
    );
    s.memo(&text, Rect::new(r.x + 2, r.y + 2, w, 8));
    let controls = [
        ("Replace", Decision::Replace),
        ("Skip", Decision::Skip),
        ("Replace all", Decision::ReplaceAll),
        ("Skip all", Decision::SkipAll),
        ("Cancel", Decision::Cancel),
    ];
    for (i, (label, decision)) in controls.into_iter().enumerate() {
        let (row, column) = if i < 3 { (0, i) } else { (1, i - 3) };
        let width = w / 3;
        s.control(Control::new(
            i,
            Rect::new(
                r.x + 2 + column as u16 * width,
                r.bottom().saturating_sub(5) + row * 2,
                width.saturating_sub(2),
                1,
            ),
            ControlKind::Button(lang.text(label).into()),
            Target::Transfer(Action::Conflict(id, decision)),
        ));
    }
    Some(s)
}
