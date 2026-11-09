//! Viewer-owned, masked PDF authentication. Credentials never enter history/config.
use super::*;
use crate::{
    app::App,
    mouse::Target,
    task::Task,
    viewer::{Action, Viewer},
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use zeroize::{Zeroize, Zeroizing};
pub const HELP: &str = "Enter the PDF password to view its text. The field is masked. An incorrect password can be retried. Cancel closes this PDF and returns to the previous document or file pane. Passwords are not saved or shared with ZIP archives. Unlocking uses the loaded PDF bytes, without rereading a changed file.";
pub struct Dialog {
    pub(crate) requests: crate::requests::RequestScope,
    pub(crate) focus: usize,
    field: Zeroizing<String>,
    cursor: usize,
    bytes: Arc<[u8]>,
    pending: Option<Task<io::Result<Contents>>>,
    error: Option<String>,
}
impl Dialog {
    pub(crate) fn new(bytes: Arc<[u8]>) -> Self {
        Self {
            requests: Default::default(),
            focus: 0,
            field: Zeroizing::new(String::new()),
            cursor: 0,
            bytes,
            pending: None,
            error: None,
        }
    }
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn edit(&mut self, text: Option<&str>, key: K) {
        if self.loading() || self.focus != 0 {
            return;
        }
        if let Some(text) = text {
            if text.chars().any(char::is_control)
                || self.field.len().saturating_add(text.len()) > 1024
            {
                self.error = Some("Input is too long or contains control characters".into());
                return;
            }
            crate::dialog::paste_input(&mut self.field, &mut self.cursor, text);
        } else if !matches!(key, K::Char(c) if self.field.len() + c.len_utf8() > 1024) {
            crate::dialog::edit_input(&mut self.field, &mut self.cursor, key);
        }
        self.error = None;
    }
    fn submit(&mut self) {
        if self.loading() {
            return;
        }
        let password = crate::archive::Password::new(&self.field);
        self.field.zeroize();
        self.cursor = 0;
        let bytes = self.bytes.clone();
        match self.requests.spawn("dn-pdf-password", move |cancel| {
            super::extract_shared(bytes, Some(&password), &cancel)
        }) {
            Ok(task) => {
                self.pending = Some(task);
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}
impl Viewer {
    pub(crate) fn poll_pdf_password(&mut self) -> bool {
        let reply = self
            .modals
            .viewer_pdf_password_mut()
            .and_then(|d| crate::task::take_reply(&mut d.pending));
        let Some(reply) = reply else {
            return false;
        };
        match reply {
            Ok(Ok(contents)) => {
                self.modals.set_viewer_pdf_password(None);
                self.contents = Some(contents);
                self.wrap = true;
                self.offset = 0;
                self.horizontal = 0;
                self.current = None;
                self.rows.clear();
                self.layout_width = 0;
                self.notice.clear();
                self.offer_encoding();
            }
            reply => {
                if let Some(d) = self.modals.viewer_pdf_password_mut() {
                    d.error = Some(match reply {
                        Ok(Err(e)) => e.to_string(),
                        _ => "PDF password worker stopped".into(),
                    });
                    d.focus = 0;
                }
            }
        }
        true
    }
}
pub(crate) fn action(app: &mut App, action: Action) -> bool {
    if !app
        .desktop
        .active_viewer()
        .is_some_and(|v| v.modals.viewer_pdf_password().is_some())
    {
        return false;
    }
    match action {
        Action::Cancel | Action::Close => {
            let id = app.desktop.target();
            app.retire_document(id);
        }
        Action::Submit => {
            app.desktop
                .active_viewer_mut()
                .unwrap()
                .modals
                .viewer_pdf_password_mut()
                .unwrap()
                .submit();
        }
        Action::Field => {
            if let Some(d) = app
                .desktop
                .active_viewer_mut()
                .and_then(|v| v.modals.viewer_pdf_password_mut())
                .filter(|d| !d.loading())
            {
                d.focus = 0;
            }
        }
        Action::Help => app.push_dialog(crate::dialog::Dialog {
            title: "PDF password help",
            message: app.config.language.text(HELP).into(),
        }),
        _ => {}
    }
    true
}
pub(crate) fn key(app: &mut App, key: KeyEvent) -> bool {
    if !app
        .desktop
        .active_viewer()
        .is_some_and(|v| v.modals.viewer_pdf_password().is_some())
    {
        return false;
    }
    if key.code == K::F(1) {
        action(app, Action::Help);
        return true;
    }
    let proposal = spec(app, crate::turbo::dialog_spec::input_area(app)).and_then(|s| s.key(key));
    if let Some(p) = proposal {
        crate::dialog_controls::apply(app, p, |app, target| {
            if let Target::Viewer(a) = target {
                action(app, a);
            }
        });
    } else if let Some(d) = app
        .desktop
        .active_viewer_mut()
        .and_then(|v| v.modals.viewer_pdf_password_mut())
    {
        if key.code == K::Char('a')
            && key.modifiers.contains(M::CONTROL)
            && d.focus == 0
            && !d.loading()
        {
            d.field.zeroize();
            d.cursor = 0;
        } else if !key.modifiers.intersects(M::ALT | M::CONTROL | M::SUPER) {
            d.edit(None, key.code);
        }
    }
    true
}
pub(crate) fn spec(
    app: &App,
    area: crate::presentation::layout::Rect,
) -> Option<crate::turbo::dialog_spec::DialogSpec<'_>> {
    use crate::{
        focus::Focus,
        painting::Paragraph,
        presentation::layout::Rect,
        turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec},
    };
    let viewer = app.desktop.active_viewer()?;
    let d = viewer.modals.viewer_pdf_password()?;
    let r = centered(area, 72, 12);
    let lang = app.config.language;
    let style = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    let mut s = DialogSpec::new(
        r,
        scope(app, Focus::ViewerSearch)?,
        d.focus,
        lang.text("PDF password"),
        Target::Viewer(Action::Submit),
        Target::Viewer(Action::Cancel),
    );
    s.caption(
        Paragraph::new(crate::formatting::tail(
            &crate::filesystem::display_name(&viewer.path.to_string_lossy()),
            r.width - 6,
        ))
        .style(style(0x70)),
        Rect::new(r.x + 3, r.y + 1, r.width - 6, 1),
    );
    s.caption(
        Paragraph::new(lang.text("Password")).style(style(0x70)),
        Rect::new(r.x + 3, r.y + 3, r.width - 6, 1),
    );
    let cursor = d.field[..d.cursor].chars().count();
    let scroll = cursor.saturating_sub(usize::from(r.width - 7));
    let mut input = Control::new(
        0,
        Rect::new(r.x + 3, r.y + 4, r.width - 6, 1),
        ControlKind::ScrolledInput {
            value: "*".repeat(d.field.chars().count()),
            scroll,
        },
        Target::Viewer(Action::Field),
    );
    input.enabled = !d.loading();
    s.control(input);
    if d.focus == 0 && !d.loading() {
        s.cursor = Some((r.x + 3 + (cursor - scroll) as u16, r.y + 4));
    }
    let status = if d.loading() {
        "Checking password…"
    } else {
        d.error
            .as_deref()
            .unwrap_or("Password is used only to unlock this PDF; never saved")
    };
    s.memo(
        &lang.message(status),
        Rect::new(r.x + 3, r.y + 6, r.width - 6, 2),
    );
    for (id, label, a, x) in [
        (1, "OK", Action::Submit, 5),
        (2, "Cancel", Action::Cancel, 28),
        (3, "Help", Action::Help, 51),
    ] {
        let mut c = Control::new(
            id,
            Rect::new(r.x + x, r.bottom() - 3, 14, 1),
            ControlKind::Button(lang.text(label).into()),
            Target::Viewer(a),
        );
        c.enabled = id != 1 || !d.loading();
        s.control(c);
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_password_modal_rejects_a_late_worker_reply() -> io::Result<()> {
        use std::{
            sync::mpsc,
            time::{Duration, Instant},
        };
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dn-pdf-stale-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&root)?;
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let mut app = App::new(root)?;
        let mut viewer = Viewer::from_contents(
            "locked snapshot".into(),
            crate::viewer::text_contents("locked"),
        );
        viewer
            .modals
            .set_viewer_pdf_password(Some(Dialog::new(Arc::from(b"encrypted".as_slice()))));
        app.show_viewer(viewer);
        let id = app.desktop.active().id;
        let d = app
            .desktop
            .active_viewer_mut()
            .unwrap()
            .modals
            .viewer_pdf_password_mut()
            .unwrap();
        let (started, start) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let (done, finished) = mpsc::sync_channel(1);
        d.pending = Some(d.requests.spawn("controlled-pdf-password", move |_| {
            started.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            done.send(()).unwrap();
            Ok(crate::viewer::text_contents("late plaintext"))
        })?);
        start.recv_timeout(Duration::from_secs(5)).unwrap();
        action(&mut app, Action::Cancel);
        assert!(!app.desktop.contains(id));
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        app.poll();
        assert!(!app.desktop.contains(id));
        assert!(app.modal_infos().is_empty());
        assert_eq!(app.desktop.active().id, crate::desktop::WindowId::MANAGER);
        Ok(())
    }
}
