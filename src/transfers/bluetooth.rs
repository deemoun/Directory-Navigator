//! Adapter between the shared queue and the workspace-owned Bluetooth worker.
use super::*;
use crate::bluetooth::transfer::{self, Direction};

pub(super) fn start(app: &mut App, index: usize, now: Instant) {
    let entry = &app.transfers.entries[index];
    let id = entry.id;
    let Request::Bluetooth(request) = &entry.request else {
        return;
    };
    if app.transfers.bluetooth.is_none() {
        // Incoming approval installs its existing listener immediately after
        // admission. Never create a second native listener here.
        if request.direction() == Direction::Receive {
            return;
        }
        match transfer::start_send(app, id, &request.clone()) {
            Ok(job) => app.transfers.bluetooth = Some(job),
            Err(error) => {
                fail(app, id, error, now);
                return;
            }
        }
    }
    let Some(job) = app.transfers.bluetooth.as_ref().filter(|j| j.id() == id) else {
        return;
    };
    job.activate();
    let entry = &mut app.transfers.entries[index];
    entry.started = Some(now);
    if entry.status != Status::Cancelling {
        entry.status = Status::Running;
    }
    app.transfers.active = Some(id);
}

pub(super) fn cancel(app: &mut App, id: TransferId) -> bool {
    let Some(job) = app.transfers.bluetooth.as_ref().filter(|j| j.id() == id) else {
        return false;
    };
    job.cancel();
    if let Some(entry) = app.transfers.entries.iter_mut().find(|e| e.id == id) {
        entry.status = Status::Cancelling;
    }
    true
}

pub(super) fn poll(app: &mut App, now: Instant) -> bool {
    let Some(job) = &app.transfers.bluetooth else {
        return false;
    };
    job.stop_offers_if_closed(app);
    let id = job.id();
    job.apply_conflict_policy();
    let completion = match job.poll() {
        Ok(result) => Some(result),
        Err(std::sync::mpsc::TryRecvError::Empty) => None,
        Err(_) => Some(Err(io::Error::other("Bluetooth transfer worker stopped"))),
    };
    let snapshot = job.snapshot();
    let progress = job.progress(&snapshot);
    if let Some(entry) = app.transfers.entries.iter_mut().find(|e| e.id == id) {
        entry.progress = progress;
        if app.transfers.active == Some(id) && entry.status != Status::Cancelling {
            entry.status = if snapshot.offer.is_some() || job.conflicts().pending().is_some() {
                Status::Waiting
            } else {
                Status::Running
            };
        }
    }
    let Some(result) = completion else {
        return true;
    };
    app.bluetooth.view.progress = snapshot.clone();
    app.bluetooth.view.progress.offer = None;
    app.transfers.bluetooth = None;
    if app.transfers.active == Some(id) {
        app.transfers.active = None;
    }
    let mut failed = false;
    if let Some(entry) = app.transfers.entries.iter_mut().find(|e| e.id == id) {
        let cancelling = entry.status == Status::Cancelling;
        entry.ended = Some(now);
        entry.status = match result {
            Ok(()) if snapshot.completed == 0 && snapshot.skipped > 0 => Status::Skipped,
            Ok(()) => Status::Completed,
            Err(error) => {
                let status = if transfer::unconfirmed(&error) {
                    Status::Unconfirmed
                } else if error.kind() == io::ErrorKind::Interrupted {
                    Status::Cancelled
                } else {
                    Status::Failed
                };
                entry.error_kind = Some(error.kind());
                entry.error = Some(error.to_string());
                status
            }
        };
        failed = matches!(entry.status, Status::Failed | Status::Unconfirmed);
        if failed || cancelling || entry.status == Status::Cancelled {
            app.transfers.paused = true;
        }
        if entry.bluetooth == Some(Direction::Receive) {
            for panel in &mut app.panels {
                if !panel.is_virtual() && snapshot.destinations.contains(&panel.path) {
                    panel.refresh(app.config.show_hidden);
                    panel.space.refresh();
                }
            }
        }
        app.bluetooth.view.notice = app.config.language.text(entry.status.caption()).into();
        entry.retire_payload();
    }
    if failed && (app.transfers.exit != ExitMode::None || app.quit_review.is_some()) {
        app.abort_quit();
    }
    app.transfers.trim();
    true
}
