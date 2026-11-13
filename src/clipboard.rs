//! Text-only desktop clipboard. One worker retains Linux selection ownership.
//! App construction is side-effect free; the interactive executable opts in.
use crate::{app::App, desktop::WindowId};
use std::sync::mpsc::{self, Receiver, SyncSender};

const LIMIT: usize = crate::file_tools::clipboard::MAX_BYTES;
enum Request {
    Write(String, Option<(crate::plugins::Binding, u64)>),
    Read(u64),
}
enum ReadError {
    Unavailable,
    Invalid(String),
}
enum Reply {
    Written(Result<(), String>, Option<(crate::plugins::Binding, u64)>),
    Read(u64, Result<String, ReadError>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Destination {
    Plugin {
        binding: crate::plugins::Binding,
        token: crate::plugins::ClipboardToken,
    },
    Editor {
        id: WindowId,
        revision: u64,
        cursor: usize,
        anchor: Option<usize>,
    },
    Command {
        text: String,
        cursor: usize,
    },
}
#[derive(Default)]
pub struct State {
    enabled: bool,
    sender: Option<SyncSender<Request>>,
    receiver: Option<Receiver<Reply>>,
    sequence: u64,
    pending: Option<(u64, Destination, String)>,
    requested_at: Option<std::time::Instant>,
}
impl State {
    pub fn enable(&mut self) {
        self.enabled = std::env::var_os("DN_DISABLE_SYSTEM_CLIPBOARD").is_none();
        #[cfg(target_os = "linux")]
        {
            self.enabled &= !crate::terminal::virtual_console();
        }
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    fn start(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        if self.sender.is_some() {
            return true;
        }
        let (send, receive) = mpsc::sync_channel(4);
        let (reply, replies) = mpsc::sync_channel(4);
        if std::thread::Builder::new()
            .name("dn-clipboard".into())
            .spawn(move || {
                let mut clipboard = None;
                while let Ok(request) = receive.recv() {
                    // Retry opening after a previous unavailable desktop or locked clipboard.
                    let backend = match &mut clipboard {
                        Some(value) => Ok(value),
                        None => arboard::Clipboard::new()
                            .map(|value| clipboard.insert(value))
                            .map_err(|e| e.to_string()),
                    };
                    let result = match request {
                        Request::Write(text, target) => Reply::Written(
                            backend.and_then(|c| c.set_text(text).map_err(|e| e.to_string())),
                            target,
                        ),
                        Request::Read(id) => Reply::Read(
                            id,
                            backend
                                .map_err(|_| ReadError::Unavailable)
                                .and_then(|c| read_result(c.get_text())),
                        ),
                    };
                    if reply.send(result).is_err() {
                        break;
                    }
                }
            })
            .is_err()
        {
            return false;
        }
        self.sender = Some(send);
        self.receiver = Some(replies);
        true
    }
}
fn read_result(result: Result<String, arboard::Error>) -> Result<String, ReadError> {
    match result {
        Ok(text) => validate(text).map_err(ReadError::Invalid),
        // An accessible clipboard with no text is different from no clipboard.
        Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
        Err(arboard::Error::ConversionFailure) => Err(ReadError::Invalid(
            "Clipboard text could not be decoded; paste was not inserted.".into(),
        )),
        Err(_) => Err(ReadError::Unavailable),
    }
}
fn validate(text: String) -> Result<String, String> {
    if text.len() > LIMIT {
        Err("Clipboard text exceeds 2 MiB; paste was not inserted.".into())
    } else if text.contains('\0') {
        Err("Clipboard contains NUL; paste was not inserted.".into())
    } else {
        Ok(text)
    }
}
pub fn copy(app: &mut App, text: String) {
    copy_to(app, text, None);
}
pub(crate) fn copy_plugin(
    app: &mut App,
    binding: crate::plugins::Binding,
    serial: u64,
    text: String,
) {
    copy_to(app, text, Some((binding, serial)));
}
fn copy_notice(app: &mut App, target: Option<(crate::plugins::Binding, u64)>, message: &str) {
    let message = app.config.language.text(message).to_owned();
    app.notice = message.clone();
    if let Some((binding, serial)) = target {
        crate::plugins::host::clipboard_copied(app, binding, serial, message);
    }
}
fn copy_to(app: &mut App, text: String, target: Option<(crate::plugins::Binding, u64)>) {
    copy_notice(
        app,
        target,
        "System clipboard unavailable; text retained internally.",
    );
    // Internal clipboard remains useful even if the desktop is absent or busy.
    app.editor_clipboard = text.clone();
    if text.len() > LIMIT {
        app.notice = app
            .config
            .language
            .text("Text retained internally; desktop clipboard limit is 2 MiB.")
            .into();
        return;
    }
    if app.clipboard.start() {
        if app
            .clipboard
            .sender
            .as_ref()
            .is_some_and(|s| s.try_send(Request::Write(text, target)).is_ok())
        {
            app.notice = app
                .config
                .language
                .text("Copied internally; updating system clipboard…")
                .into();
        } else {
            app.notice = app
                .config
                .language
                .text("System clipboard busy; text retained internally.")
                .into();
        }
    }
    if let Some((binding, serial)) = target {
        let message = app.notice.clone();
        crate::plugins::host::clipboard_copied(app, binding, serial, message);
    }
}
fn destination(app: &App, editor: bool) -> Option<Destination> {
    if editor {
        if crate::focus::current(app) != crate::focus::Focus::Editor {
            return None;
        }
        let e = app.desktop.active_editor()?;
        if e.busy() || e.modals.editor_modal().is_some() || e.modals.editor_menu().is_some() {
            return None;
        }
        let b = e.buffer.as_ref()?;
        Some(Destination::Editor {
            id: app.desktop.active().id,
            revision: b.revision(),
            cursor: b.cursor,
            anchor: b.anchor,
        })
    } else {
        (crate::focus::current(app) == crate::focus::Focus::Manager).then(|| Destination::Command {
            text: app.command_line.text.clone(),
            cursor: app.command_line.cursor,
        })
    }
}
pub fn paste(app: &mut App, editor: bool) {
    let Some(target) = destination(app, editor) else {
        if editor {
            crate::editor::paste(app, &app.editor_clipboard.clone());
        }
        return;
    };
    request_paste(app, target);
}
fn request_paste(app: &mut App, target: Destination) {
    if app.clipboard.start() {
        app.clipboard.sequence = app.clipboard.sequence.wrapping_add(1);
        let id = app.clipboard.sequence;
        if app
            .clipboard
            .sender
            .as_ref()
            .is_some_and(|s| s.try_send(Request::Read(id)).is_ok())
        {
            app.clipboard.pending = Some((id, target, app.editor_clipboard.clone()));
            app.clipboard.requested_at = Some(std::time::Instant::now());
            return;
        }
    }
    insert(app, &target, &app.editor_clipboard.clone());
}
pub(crate) fn paste_plugin(
    app: &mut App,
    binding: crate::plugins::Binding,
    token: crate::plugins::ClipboardToken,
) {
    if crate::plugins::host::clipboard_token(app, binding) == Some(token) {
        request_paste(app, Destination::Plugin { binding, token });
    }
}
fn valid(app: &App, target: &Destination) -> bool {
    match target {
        Destination::Plugin { binding, token } => {
            crate::plugins::host::clipboard_token(app, *binding) == Some(*token)
        }
        _ => {
            destination(app, matches!(target, Destination::Editor { .. })).as_ref() == Some(target)
        }
    }
}
fn insert(app: &mut App, target: &Destination, text: &str) {
    if app.screensaver.active || !valid(app, target) {
        return;
    }
    if let Destination::Plugin { binding, .. } = target {
        crate::plugins::host::dispatch(app, *binding, crate::plugins::Event::Paste(text));
        return;
    }
    let editor = matches!(target, Destination::Editor { .. });
    if destination(app, editor).as_ref() != Some(target) || app.screensaver.active {
        return;
    }
    if editor {
        crate::editor::paste(app, text);
    } else {
        crate::command_line::paste(app, text);
    }
}
fn inform(app: &mut App, target: &Destination, message: &str) {
    if !valid(app, target) {
        return;
    }
    let message = app.config.language.text(message).to_owned();
    if let Destination::Editor { id, .. } = target {
        if let Some(editor) = app.desktop.editor_mut(*id) {
            editor.notice = message.clone();
        }
    }
    if let Destination::Plugin { binding, token } = target {
        crate::plugins::host::dispatch(
            app,
            *binding,
            crate::plugins::Event::ClipboardPasteNotice {
                token: *token,
                message: message.clone(),
            },
        );
    }
    app.notice = message;
}
pub fn poll(app: &mut App) -> bool {
    let mut changed = false;
    if app
        .clipboard
        .requested_at
        .is_some_and(|start| start.elapsed() >= std::time::Duration::from_secs(1))
    {
        app.clipboard.requested_at = None;
        if let Some((_, target, text)) = app.clipboard.pending.take() {
            inform(
                app,
                &target,
                "System clipboard unavailable; using internal clipboard.",
            );
            insert(app, &target, &text);
            changed = true;
        }
    }
    while let Some(reply) = app
        .clipboard
        .receiver
        .as_ref()
        .and_then(|r| r.try_recv().ok())
    {
        changed = true;
        match reply {
            Reply::Written(result, target) => {
                copy_notice(
                    app,
                    target,
                    if result.is_ok() {
                        "Copied to system and internal clipboards."
                    } else {
                        "System clipboard unavailable; text retained internally."
                    },
                );
            }
            Reply::Read(id, result) => {
                if app.clipboard.pending.as_ref().is_none_or(|p| p.0 != id) {
                    continue;
                }
                app.clipboard.requested_at = None;
                let (_, target, fallback) = app.clipboard.pending.take().unwrap();
                match result {
                    Ok(text) => insert(app, &target, &text),
                    Err(ReadError::Invalid(message)) => {
                        inform(app, &target, &message);
                    }
                    Err(ReadError::Unavailable) => {
                        inform(
                            app,
                            &target,
                            "System clipboard unavailable; using internal clipboard.",
                        );
                        insert(app, &target, &fallback);
                    }
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_or_non_text_desktop_clipboard_never_pastes_stale_internal_text() {
        assert!(
            matches!(read_result(Err(arboard::Error::ContentNotAvailable)), Ok(text) if text.is_empty())
        );
        assert!(matches!(
            read_result(Err(arboard::Error::ClipboardNotSupported)),
            Err(ReadError::Unavailable)
        ));
        assert!(matches!(
            read_result(Err(arboard::Error::ConversionFailure)),
            Err(ReadError::Invalid(_))
        ));
    }
    #[test]
    fn text_validation_preserves_unicode_and_newlines() {
        let text = "Ёж /世界\r\nhello\n".to_owned();
        assert_eq!(validate(text.clone()).unwrap(), text);
        assert!(validate("a\0b".into()).is_err());
        assert!(validate("x".repeat(LIMIT + 1)).is_err());
        assert_eq!(validate(String::new()).unwrap(), "");
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use std::{fs, io};
    #[test]
    fn injected_system_results_fallback_empty_rejection_and_stale_command_target() -> io::Result<()>
    {
        let root =
            std::env::temp_dir().join(format!("dn-clipboard-routing-{}", std::process::id()));
        fs::create_dir(&root)?;
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _temp = Temp(root.clone());
        let mut app = App::new(root)?;
        let (send, receive) = mpsc::sync_channel(4);
        app.clipboard.receiver = Some(receive);
        app.editor_clipboard = "internal".into();
        for (result, expected) in [
            (Ok("Ёж世界".into()), "Ёж世界"),
            (Err(ReadError::Unavailable), "internal"),
            (Ok(String::new()), ""),
            (Err(ReadError::Invalid("invalid".into())), ""),
        ] {
            app.command_line.text.clear();
            app.command_line.cursor = 0;
            let target = destination(&app, false).unwrap();
            app.clipboard.pending = Some((10, target, "internal".into()));
            send.send(Reply::Read(10, result)).unwrap();
            assert!(poll(&mut app));
            assert_eq!(app.command_line.text, expected);
            assert_eq!(app.editor_clipboard, "internal");
        }
        app.command_line.text.clear();
        app.command_line.cursor = 0;
        let target = destination(&app, false).unwrap();
        app.clipboard.pending = Some((11, target, "internal".into()));
        crate::command_line::paste(&mut app, "new input");
        send.send(Reply::Read(10, Ok("stale generation".into())))
            .unwrap();
        send.send(Reply::Read(11, Ok("stale cursor".into())))
            .unwrap();
        poll(&mut app);
        assert_eq!(app.command_line.text, "new input");
        assert!(app.clipboard.sender.is_none());
        Ok(())
    }
}

#[cfg(test)]
mod editor_tests {
    use super::*;
    use std::{
        fs, io,
        time::{Duration, Instant},
    };
    #[test]
    fn delayed_paste_preserves_document_identity_cursor_revision_and_modal_ownership(
    ) -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-clipboard-editor-{}", std::process::id()));
        fs::create_dir(&root)?;
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _temp = Temp(root.clone());
        let path = root.join("a.txt");
        let second = root.join("b.txt");
        fs::write(&path, b"abc")?;
        fs::write(&second, b"abc")?;
        let mut app = App::new(root)?;
        crate::editor::open_path(&mut app, path);
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.desktop.active_editor().is_none_or(|e| e.busy()) {
            app.poll();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let target = destination(&app, true).unwrap();
        insert(&mut app, &target, "Ё");
        assert!(app
            .desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text
            .contains('Ё'));
        // The old revision cannot receive a second insertion.
        insert(&mut app, &target, "STALE");
        assert!(!app
            .desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text
            .contains("STALE"));
        let target = destination(&app, true).unwrap();
        app.push_dialog(crate::dialog::Dialog {
            title: " Test ",
            message: "owned prompt".into(),
        });
        insert(&mut app, &target, "MODAL");
        app.pop_dialog();
        assert!(!app
            .desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text
            .contains("MODAL"));
        crate::editor::open_path(&mut app, second);
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.desktop.active_editor().is_none_or(|e| e.busy()) {
            app.poll();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        insert(&mut app, &target, "OTHER");
        assert_eq!(
            app.desktop
                .active_editor()
                .unwrap()
                .buffer
                .as_ref()
                .unwrap()
                .text,
            "abc"
        );
        assert!(app.clipboard.sender.is_none());
        Ok(())
    }
}

#[cfg(test)]
mod plugin_tests {
    use super::*;
    use crate::plugins::{host, ActionId, Event, NEARBY_TRANSFERS};
    #[test]
    fn plugin_paste_uses_internal_fallback_and_rejects_changed_or_reopened_composers(
    ) -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-clipboard-nearby-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _fixture = Fixture(root.clone());
        let mut app = App::new(root)?;
        host::open(&mut app, NEARBY_TRANSFERS);
        let binding = app.desktop.plugins.binding(NEARBY_TRANSFERS).unwrap();
        host::dispatch(&mut app, binding, Event::Focus(20));
        host::dispatch(&mut app, binding, Event::Paste("manual-card"));
        host::dispatch(&mut app, binding, Event::Action(ActionId(31)));
        let token = host::clipboard_token(&app, binding).unwrap();
        app.editor_clipboard = "https://example.test/Ёжик".into();
        paste_plugin(&mut app, binding, token);
        let after = host::clipboard_token(&app, binding).unwrap();
        assert_ne!(after, token);
        assert_eq!(after.cursor, app.editor_clipboard.len());
        insert(&mut app, &Destination::Plugin { binding, token }, "STALE");
        assert_eq!(host::clipboard_token(&app, binding), Some(after));
        host::dispatch(&mut app, binding, Event::Action(ActionId(33)));
        assert!(host::clipboard_token(&app, binding).is_none());
        host::dispatch(&mut app, binding, Event::Action(ActionId(31)));
        let reopened = host::clipboard_token(&app, binding).unwrap();
        assert_ne!(reopened.session, after.session);
        insert(
            &mut app,
            &Destination::Plugin {
                binding,
                token: after,
            },
            "STALE",
        );
        assert_eq!(host::clipboard_token(&app, binding), Some(reopened));
        assert!(app.clipboard.sender.is_none());
        Ok(())
    }
}
