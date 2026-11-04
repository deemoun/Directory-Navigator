//! Owned creation/password dialogs using the common DN controls.
use super::{create::Creation, reader, Extraction, Password};
use crate::{
    app::App, focus::Focus, mouse::Target, operations::CopyOptions, panel::archive::MemberId,
    task::Task,
};
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use std::{io, path::PathBuf, sync::Arc};
use zeroize::Zeroize;
const HELP: &str = "Create a new ZIP from marked files and folders, or the current item. Paths and empty folders are preserved; originals are kept. Existing archives are never overwritten or updated. Links and special files are not packed.\nBlank password creates an ordinary ZIP. A password creates AES-256 encrypted file contents; filenames remain visible. Confirm the password before creating. Passwords are never saved to settings or history.\nCreation runs in Transfer Center and can be cancelled. The final archive appears only after success. DN currently accepts up to 10,000 entries and 256 MiB of input.\nEncrypted ZIP preview and extraction ask for a password. A successful password is remembered only while this archive pane is open; refresh forgets it. Cancel leaves files unchanged. Other ZIP applications must support AES encryption.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Field(usize),
    Confirm,
    Cancel,
    Help,
}
enum Resume {
    Preview(MemberId, crate::viewer::ViewMode),
    Extract {
        selection: Extraction,
        destination: PathBuf,
        options: CopyOptions,
        root: Option<crate::temp_drive::Root>,
    },
}
enum Operation {
    Create {
        sources: Vec<PathBuf>,
        base: PathBuf,
    },
    Unlock {
        panel: usize,
        generation: u64,
        catalog: Arc<reader::Catalog>,
        ids: Vec<usize>,
        resume: Resume,
    },
}
pub struct Dialog {
    pub(crate) requests: crate::requests::RequestScope,
    pub(crate) focus: usize,
    fields: [String; 3],
    cursors: [usize; 3],
    operation: Operation,
    pending: Option<Task<io::Result<()>>>,
    candidate: Option<Password>,
    error: Option<String>,
}
impl Drop for Dialog {
    fn drop(&mut self) {
        self.fields[1].zeroize();
        self.fields[2].zeroize();
    }
}
impl Dialog {
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
    fn new(operation: Operation) -> Self {
        Self {
            requests: Default::default(),
            focus: 1,
            fields: Default::default(),
            cursors: [0; 3],
            operation,
            pending: None,
            candidate: None,
            error: None,
        }
    }
    fn creating(&self) -> bool {
        matches!(self.operation, Operation::Create { .. })
    }
    fn edit(&mut self, text: Option<&str>, key: K) {
        let i = self.focus;
        if self.pending.is_some() || i >= 3 || (!self.creating() && i != 1) {
            return;
        }
        // Keep password work bounded and never silently truncate pasted secrets.
        if let Some(text) = text {
            if text.chars().any(char::is_control)
                || self.fields[i].len() + text.len() > if i == 0 { 16_384 } else { 1024 }
            {
                self.error = Some("Input is too long or contains control characters".into());
                return;
            }
            crate::dialog::paste_input(&mut self.fields[i], &mut self.cursors[i], text);
        } else if !matches!(key, K::Char(c) if self.fields[i].len() + c.len_utf8() > if i == 0 {16_384} else {1024})
        {
            crate::dialog::edit_input(&mut self.fields[i], &mut self.cursors[i], key);
        }
        self.error = None;
    }
}
pub fn create(app: &mut App) {
    crate::menu::close(app);
    if crate::windowing::blocked(app) || !super::require_native(app) || app.blocking_operation() {
        return;
    }
    let sources = app.panels[app.active].targets();
    if sources.is_empty() {
        app.error("Select files or folders to create an archive");
        return;
    }
    let base = app.panels[if app.panels[app.active ^ 1].is_archive() {
        app.active
    } else {
        app.active ^ 1
    }]
    .path
    .clone();
    let name = if sources.len() == 1 {
        sources[0].file_name()
    } else {
        app.panels[app.active].path.file_name()
    }
    .and_then(|s| s.to_str())
    .unwrap_or("archive");
    let path = base.join(format!("{name}.zip"));
    let mut d = Dialog::new(Operation::Create { sources, base });
    d.fields[0] = path.to_string_lossy().into_owned();
    d.cursors[0] = d.fields[0].len();
    d.focus = 0;
    crate::windowing::activate(app, crate::desktop::WindowId::MANAGER);
    app.modals.set_archive_dialog(Some(d));
}
pub(crate) fn password_for_preview(app: &mut App, id: MemberId) {
    let panel = app.active;
    let Some(a) = app.panels[panel].archive() else {
        return;
    };
    if a.generation != id.generation {
        return;
    }
    let catalog = a.catalog.as_ref().unwrap().clone();
    let mode = a.preview_mode;
    app.modals
        .set_archive_dialog(Some(Dialog::new(Operation::Unlock {
            panel,
            generation: id.generation,
            catalog,
            ids: vec![id.index],
            resume: Resume::Preview(id, mode),
        })));
}
pub(crate) fn password_for_extraction(
    app: &mut App,
    selection: Extraction,
    destination: PathBuf,
    options: CopyOptions,
    root: Option<crate::temp_drive::Root>,
) {
    let ids = match selection.selected() {
        Ok(nodes) => nodes.into_iter().map(|(id, _)| id).collect(),
        Err(e) => {
            app.error(e.to_string());
            return;
        }
    };
    app.modals
        .set_archive_dialog(Some(Dialog::new(Operation::Unlock {
            panel: app.active,
            generation: selection.catalog.generation,
            catalog: selection.catalog.clone(),
            ids,
            resume: Resume::Extract {
                selection,
                destination,
                options,
                root,
            },
        })));
}
pub fn action(app: &mut App, action: Action) {
    match action {
        Action::Cancel => app.modals.set_archive_dialog(None),
        Action::Help => app.push_dialog(crate::dialog::Dialog {
            title: " Archive creation help ",
            message: HELP.into(),
        }),
        Action::Field(i) => {
            if let Some(d) = app
                .modals
                .archive_dialog_mut()
                .filter(|d| d.pending.is_none())
            {
                d.focus = i;
            }
        }
        Action::Confirm => confirm(app),
    }
}
fn confirm(app: &mut App) {
    let Some(d) = app
        .modals
        .archive_dialog_mut()
        .filter(|d| d.pending.is_none())
    else {
        return;
    };
    if let Operation::Create { sources, base } = &d.operation {
        if d.fields[0].is_empty() {
            d.error = Some("Enter a .zip archive path".into());
            return;
        }
        if d.fields[1] != d.fields[2] {
            d.error = Some("Passwords do not match".into());
            d.focus = 2;
            return;
        }
        let path = PathBuf::from(&d.fields[0]);
        let path = if path.is_absolute() {
            path
        } else {
            base.join(path)
        };
        let password = (!d.fields[1].is_empty()).then(|| Password::new(&d.fields[1]));
        match Creation::capture(sources.clone(), path, password) {
            Ok(creation) => {
                let mut draft = app.modals.take_archive_dialog().unwrap();
                if let Err(e) = crate::transfers::submit_creation(app, creation) {
                    draft.error = Some(e);
                    app.modals.set_archive_dialog(Some(draft));
                }
            }
            Err(e) => d.error = Some(e.to_string()),
        }
        return;
    }
    if d.fields[1].is_empty() {
        d.error = Some("Enter the archive password".into());
        return;
    }
    let password = Password::new(&d.fields[1]);
    let Operation::Unlock { catalog, ids, .. } = &d.operation else {
        return;
    };
    let catalog = catalog.clone();
    let ids = ids.clone();
    let candidate = password.clone();
    match d.requests.spawn("archive-password", move |cancel| {
        reader::check_password(&catalog, &ids, &password, &cancel)
    }) {
        Ok(task) => {
            d.pending = Some(task);
            d.candidate = Some(candidate);
            d.error = None;
        }
        Err(e) => d.error = Some(e.to_string()),
    }
}
pub(crate) fn poll(app: &mut App) -> bool {
    if crate::focus::current(app) != Focus::ArchiveDialog {
        return false;
    }
    let Some(d) = app.modals.archive_dialog_mut() else {
        return false;
    };
    let Some(reply) = crate::task::take_reply(&mut d.pending) else {
        return false;
    };
    match reply {
        Ok(Ok(())) => {}
        other => {
            d.candidate = None;
            d.fields[1].zeroize();
            d.cursors[1] = 0;
            d.focus = 1;
            d.error = Some(match other {
                Ok(Err(e)) => e.to_string(),
                _ => "Password worker stopped".into(),
            });
            return true;
        }
    }
    let mut d = app.modals.take_archive_dialog().unwrap();
    let password = d.candidate.take().unwrap();
    // Payload owns secrets and has Drop; replace the operation before moving it.
    let operation = std::mem::replace(
        &mut d.operation,
        Operation::Create {
            sources: Vec::new(),
            base: PathBuf::new(),
        },
    );
    if let Operation::Unlock {
        panel,
        generation,
        resume,
        ..
    } = operation
    {
        if app.active != panel
            || !app.panels[panel]
                .archive()
                .is_some_and(|a| a.generation == generation)
        {
            return true;
        }
        app.panels[panel].archive_mut().unwrap().password = Some(password.clone());
        match resume {
            Resume::Preview(id, mode) => {
                if app.panels[panel]
                    .entries
                    .get(app.panels[panel].cursor)
                    .and_then(|e| app.panels[panel].member(e))
                    == Some(id)
                {
                    super::view_mode(app, mode);
                }
            }
            Resume::Extract {
                mut selection,
                destination,
                options,
                root,
            } => {
                selection.password = Some(password);
                match crate::transfers::submit_extraction(
                    app,
                    selection,
                    destination.clone(),
                    options,
                    root,
                ) {
                    Ok(_) => app.copy_dialog.record(destination),
                    Err(e) => app.error(e),
                }
            }
        }
    }
    true
}
pub fn key(app: &mut App, key: KeyEvent) {
    if key.code == K::Esc {
        action(app, Action::Cancel);
        return;
    }
    if key.code == K::F(1) {
        action(app, Action::Help);
        return;
    }
    let proposal = spec(app, crate::turbo::dialog_spec::input_area(app)).and_then(|s| s.key(key));
    if let Some(p) = proposal {
        crate::dialog_controls::apply(app, p, crate::dialog_controls::tool_action);
        return;
    }
    if !key.modifiers.intersects(M::ALT | M::CONTROL) {
        if let Some(d) = app.modals.archive_dialog_mut() {
            d.edit(None, key.code);
        }
    }
}
pub fn paste(app: &mut App, text: &str) {
    if let Some(d) = app.modals.archive_dialog_mut() {
        d.edit(Some(text), K::Null);
    }
}
pub fn mouse(app: &mut App, e: MouseEvent, target: Option<Target>) {
    if e.kind == MouseEventKind::Down(MouseButton::Left) {
        if let Some(Target::Archive(a)) = target {
            action(app, a);
        }
    }
}
pub(crate) fn spec(
    app: &App,
    area: crate::presentation::layout::Rect,
) -> Option<crate::turbo::dialog_spec::DialogSpec<'_>> {
    use crate::{
        painting::Paragraph,
        presentation::{layout::Rect, text::Line},
        turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec},
    };
    let d = app.modals.archive_dialog()?;
    let lang = app.config.language;
    let creating = d.creating();
    let r = centered(area, 72, if creating { 18 } else { 12 });
    let dos = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    let mut s = DialogSpec::new(
        r,
        scope(app, Focus::ArchiveDialog)?,
        d.focus,
        lang.text(if creating {
            " Create archive "
        } else {
            " Archive password "
        }),
        Target::Archive(Action::Confirm),
        Target::Archive(Action::Cancel),
    );
    if !creating {
        if let Operation::Unlock { catalog, .. } = &d.operation {
            s.caption(
                Paragraph::new(crate::formatting::tail(
                    &crate::filesystem::display_name(&catalog.path.to_string_lossy()),
                    r.width - 6,
                ))
                .style(dos(0x70)),
                Rect::new(r.x + 3, r.y + 1, r.width - 6, 1),
            );
        }
    }
    for (i, label, y) in if creating {
        vec![
            (0, "Archive path (.zip)", 2),
            (1, "Password (optional)", 5),
            (2, "Confirm password", 8),
        ]
    } else {
        vec![(1, "Password", 3)]
    } {
        s.caption(
            Paragraph::new(lang.text(label)).style(dos(0x70)),
            Rect::new(r.x + 3, r.y + y, r.width - 6, 1),
        );
        let value = if i == 0 {
            d.fields[i].clone()
        } else {
            "*".repeat(d.fields[i].chars().count())
        };
        let cursor = if i == 0 {
            Line::from(&d.fields[i][..d.cursors[i]]).width()
        } else {
            d.fields[i][..d.cursors[i]].chars().count()
        };
        let scroll = cursor.saturating_sub((r.width - 7) as usize);
        let mut control = Control::new(
            i,
            Rect::new(r.x + 3, r.y + y + 1, r.width - 6, 1),
            ControlKind::ScrolledInput { value, scroll },
            Target::Archive(Action::Field(i)),
        );
        control.enabled = d.pending.is_none();
        s.control(control);
        if d.focus == i && d.pending.is_none() {
            s.cursor = Some((r.x + 3 + (cursor - scroll) as u16, r.y + y + 1));
        }
    }
    let status = if d.pending.is_some() {
        lang.text("Checking password…").into()
    } else if let Some(error) = &d.error {
        lang.message(error)
    } else {
        lang.text(if creating {
            "ZIP · Originals kept · Blank password: no encryption"
        } else {
            "Remembered only while this archive pane is open"
        })
        .into()
    };
    s.caption(
        Paragraph::new(crate::formatting::tail(&status, r.width - 6)).style(dos(0x70)),
        Rect::new(r.x + 3, r.bottom() - 5, r.width - 6, 1),
    );
    for (id, label, a, x) in [
        (
            3,
            if creating { "Create" } else { "OK" },
            Action::Confirm,
            5,
        ),
        (4, "Cancel", Action::Cancel, 28),
        (5, "Help", Action::Help, 51),
    ] {
        let mut c = Control::new(
            id,
            Rect::new(r.x + x, r.bottom() - 3, 14, 1),
            ControlKind::Button(lang.text(label).into()),
            Target::Archive(a),
        );
        c.enabled = id != 3 || d.pending.is_none();
        s.control(c);
    }
    Some(s)
}
