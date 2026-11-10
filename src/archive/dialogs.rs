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
const HELP: &str = "Create ZIP archives from selected files and folders; originals are kept. New ZIPs may use an AES-256 password; filenames remain visible.\nTo update a ZIP, open it in the other pane and press F5 on disk files. Update explicitly approves replacing matching files; unrelated members are kept. Cancel is selected initially. F8 in a ZIP confirms removal of selected trees; the root archive is kept.\nF4 edits a ZIP text member in a private workspace. F2 rebuilds and publishes the ZIP only after success; failed saves retain the dirty editor. Save As exports a regular file.\nCreation and updates support 100,000 entries and 64 GiB of input. They stage beside the archive, retain originals on failure and support cancellation.\nPasswords are never saved to configuration or history. Encrypted previews and extraction authenticate before publication. ZIP passwords are remembered only for the current archive pane. RAR and 7z are read-only; encrypted headers can also ask for a password.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Field(usize),
    Confirm,
    Cancel,
    Help,
}
enum Resume {
    Edit(MemberId),
    Preview(MemberId, crate::viewer::ViewMode),
    Extract {
        selection: Extraction,
        destination: PathBuf,
        options: CopyOptions,
        root: Option<crate::temp_drive::Root>,
    },
}
enum Operation {
    Open {
        panel: usize,
        path: PathBuf,
        generation: u64,
    },
    Remove {
        catalog: Arc<reader::Catalog>,
        ids: std::collections::BTreeSet<usize>,
    },
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
    error_kind: Option<io::ErrorKind>,
    validation_id: Option<&'static str>,
    target: Option<(Arc<reader::Catalog>, usize)>,
}
impl Drop for Dialog {
    fn drop(&mut self) {
        self.fields[1].zeroize();
        self.fields[2].zeroize();
    }
}
impl Dialog {
    fn fail(&mut self, error: io::Error) {
        self.error_kind = Some(error.kind());
        self.validation_id = None;
        self.error = Some(error.to_string());
    }
    fn validation(&mut self, id: &'static str) {
        self.error = Some(id.into());
        self.validation_id = Some(id);
        self.error_kind = None;
    }
    fn report(&self, lang: crate::localization::Language) -> Option<crate::error_report::Report> {
        let cause = self.error.as_ref()?;
        let (summary,advice)=match &self.operation {
            Operation::Open{..}|Operation::Unlock{..}=>("Archive could not be unlocked.","Check the password and file access. The archive may use unsupported encryption."),
            _ if self.target.is_some() || matches!(self.operation,Operation::Remove{..})=>("Cannot prepare the ZIP update.","Check the selected members, archive access and free space. Refresh the archive after external changes."),
            _=>("Cannot prepare the ZIP archive.","Check the archive path, options and free space. New archive creation requires an unused destination name."),
        };
        let mut report = crate::error_report::Report::new(
            summary,
            self.validation_id
                .map_or_else(|| cause.clone(), |id| lang.text(id).into()),
            advice,
        )
        .outcome(crate::error_report::Outcome::Unchanged);
        match &self.operation {
            Operation::Open { path, .. } => report = report.path("Archive", path),
            Operation::Unlock { catalog, .. } | Operation::Remove { catalog, .. } => {
                report = report.path("Archive", &catalog.path)
            }
            _ => {}
        }
        // Password checking can return PermissionDenied for either authentication
        // or filesystem permissions; retain the context-specific advice.
        if !matches!(
            self.operation,
            Operation::Open { .. } | Operation::Unlock { .. }
        ) {
            if let Some(kind) = self.error_kind {
                report = report.io(kind);
            }
        }
        Some(report)
    }
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
            error_kind: None,
            validation_id: None,
            target: None,
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
                self.validation("Input is too long or contains control characters");
                return;
            }
            crate::dialog::paste_input(&mut self.fields[i], &mut self.cursors[i], text);
        } else if !matches!(key, K::Char(c) if self.fields[i].len() + c.len_utf8() > if i == 0 {16_384} else {1024})
        {
            crate::dialog::edit_input(&mut self.fields[i], &mut self.cursors[i], key);
        }
        self.error = None;
        self.error_kind = None;
        self.validation_id = None;
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
pub(crate) fn add_to_zip(app: &mut App, panel: usize) {
    let sources = app.panels[app.active].targets();
    let Some(a) = app.panels[panel].archive() else {
        return;
    };
    let Some(catalog) = a
        .catalog
        .clone()
        .filter(|c| c.format == reader::Format::Zip)
    else {
        app.error("Only ZIP archives can be updated");
        return;
    };
    let mut d = Dialog::new(Operation::Create {
        sources,
        base: app.panels[app.active].path.clone(),
    });
    d.fields[0] = catalog.path.to_string_lossy().into_owned();
    d.cursors[0] = d.fields[0].len();
    d.target = Some((catalog, a.directory));
    // Existing members may be replaced: require explicit Update, never default it.
    d.focus = 4;
    app.modals.set_archive_dialog(Some(d));
}
pub(crate) fn remove(app: &mut App) {
    let Some(selection) = app.panels[app.active].extraction() else {
        return;
    };
    if selection.catalog.format != reader::Format::Zip {
        app.error(super::EXTRACT_FIRST);
        return;
    }
    let ids = selection.members.iter().map(|id| id.index).collect();
    let mut d = Dialog::new(Operation::Remove {
        catalog: selection.catalog,
        ids,
    });
    d.focus = 4;
    app.modals.set_archive_dialog(Some(d));
}
pub(super) fn password_for_open(app: &mut App, panel: usize) {
    let Some(a) = app.panels[panel].archive() else {
        return;
    };
    let d = Dialog::new(Operation::Open {
        panel,
        path: a.path.clone(),
        generation: a.generation,
    });
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
pub(crate) fn password_for_edit(app: &mut App, id: MemberId) {
    password_for_preview(app, id);
    if let Some(dialog) = app.modals.archive_dialog_mut() {
        if let Operation::Unlock { resume, .. } = &mut dialog.operation {
            *resume = Resume::Edit(id);
        }
    }
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
        Action::Cancel => {
            if let Some(d) = app.modals.archive_dialog() {
                if let Operation::Open {
                    panel, generation, ..
                } = d.operation
                {
                    if app.panels[panel]
                        .archive()
                        .is_some_and(|a| a.generation == generation)
                    {
                        app.panels[panel].leave_archive(app.config.show_hidden);
                    }
                }
            }
            app.modals.set_archive_dialog(None);
        }
        Action::Help => {
            let mut message = app.config.language.text(HELP).to_owned();
            if let Some(report) = app
                .modals
                .archive_dialog()
                .and_then(|d| d.report(app.config.language))
            {
                message = format!("{}\n\n{}", report.render(app.config.language), message);
            }
            app.push_dialog(crate::dialog::Dialog {
                title: " Archive creation help ",
                message,
            });
        }
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
    d.error_kind = None;
    d.validation_id = None;
    if let Operation::Remove { catalog, ids } = &d.operation {
        let request = Creation::update(catalog.clone(), Vec::new(), 0, ids.clone(), None);
        match request {
            Ok(creation) => {
                let mut draft = app.modals.take_archive_dialog().unwrap();
                if let Err(error) = crate::transfers::submit_creation(app, creation) {
                    draft.error = Some(error);
                    app.modals.set_archive_dialog(Some(draft));
                }
            }
            Err(error) => d.fail(error),
        }
        return;
    }
    if let Operation::Create { sources, base } = &d.operation {
        if d.fields[0].is_empty() {
            d.validation("Enter a .zip archive path");
            return;
        }
        if d.fields[1] != d.fields[2] {
            d.validation("Passwords do not match");
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
        let request = if let Some((catalog, directory)) = &d.target {
            if path != catalog.path {
                d.validation("Keep the selected ZIP path for an update");
                return;
            }
            Creation::update(
                catalog.clone(),
                sources.clone(),
                *directory,
                Default::default(),
                password,
            )
        } else {
            Creation::capture(sources.clone(), path, password)
        };
        match request {
            Ok(creation) => {
                let mut draft = app.modals.take_archive_dialog().unwrap();
                if let Err(e) = crate::transfers::submit_creation(app, creation) {
                    draft.error = Some(e);
                    app.modals.set_archive_dialog(Some(draft));
                }
            }
            Err(e) => d.fail(e),
        }
        return;
    }
    if d.fields[1].is_empty() {
        d.validation("Enter the archive password");
        return;
    }
    let password = Password::new(&d.fields[1]);
    let candidate = password.clone();
    let task = match &d.operation {
        Operation::Open { path, .. } => {
            let path = path.clone();
            d.requests.spawn("archive-password", move |cancel| {
                reader::list_with_password(path, &password, &cancel).map(|_| ())
            })
        }
        Operation::Unlock { catalog, ids, .. } => {
            let catalog = catalog.clone();
            let ids = ids.clone();
            d.requests.spawn("archive-password", move |cancel| {
                reader::check_password(&catalog, &ids, &password, &cancel)
            })
        }
        _ => return,
    };
    match task {
        Ok(task) => {
            d.pending = Some(task);
            d.candidate = Some(candidate);
            d.error = None;
        }
        Err(e) => d.fail(e),
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
            match other {
                Ok(Err(e)) => d.fail(e),
                _ => d.fail(io::Error::other("Password worker stopped")),
            };
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
    if let Operation::Open {
        panel,
        path,
        generation,
    } = &operation
    {
        let panel = *panel;
        if app.active == panel
            && app.panels[panel]
                .archive()
                .is_some_and(|a| a.generation == *generation && a.path == *path)
        {
            let path = path.clone();
            let value = password.clone();
            match app.panels[panel]
                .scope
                .spawn("archive-index", move |cancel| {
                    reader::list_with_password(path, &value, &cancel)
                }) {
                Ok(task) => {
                    let a = app.panels[panel].archive_mut().unwrap();
                    a.index = Some(task);
                    a.password = Some(password);
                }
                Err(error) => app.error(error.to_string()),
            }
        }
        return true;
    }
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
            Resume::Edit(id) => {
                if app.panels[panel]
                    .entries
                    .get(app.panels[panel].cursor)
                    .and_then(|e| app.panels[panel].member(e))
                    == Some(id)
                {
                    super::edit::open(app);
                }
            }
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
    let removing = matches!(d.operation, Operation::Remove { .. });
    let updating = d.target.is_some();
    let r = centered(area, 72, if creating { 18 } else { 12 });
    let dos = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    let mut s = DialogSpec::new(
        r,
        scope(app, Focus::ArchiveDialog)?,
        d.focus,
        lang.text(if removing {
            " Remove ZIP members "
        } else if updating {
            " Update ZIP "
        } else if creating {
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
    if let Operation::Remove { catalog, ids } = &d.operation {
        s.caption(
            Paragraph::new(format!(
                "{}\n{}\n{}",
                lang.text(
                    "Permanently remove selected members and their descendants from this ZIP?"
                ),
                crate::formatting::tail(&catalog.path.to_string_lossy(), r.width - 6),
                ids.len()
            ))
            .style(dos(0x70)),
            Rect::new(r.x + 3, r.y + 2, r.width - 6, 4),
        );
    }
    for (i, label, y) in if removing {
        Vec::new()
    } else if creating {
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
    } else if let Some(report) = d.report(lang) {
        report.inline(lang, r.width - 6)
    } else {
        lang.text(if removing {
            "The original ZIP is kept until the update succeeds"
        } else if updating {
            "Matching files will be replaced; other members are kept"
        } else if creating {
            "ZIP · Originals kept · Blank password: no encryption"
        } else {
            "Remembered only while this archive pane is open"
        })
        .into()
    };
    s.caption(
        Paragraph::new(if d.error.is_some() && !removing {
            status
        } else {
            crate::formatting::tail(&status, r.width - 6)
        })
        .style(dos(0x70)),
        Rect::new(
            r.x + 3,
            r.bottom() - if d.error.is_some() && !removing { 7 } else { 5 },
            r.width - 6,
            if d.error.is_some() && !removing { 3 } else { 1 },
        ),
    );
    for (id, label, a, x) in [
        (
            3,
            if removing {
                "Remove"
            } else if updating {
                "Update"
            } else if creating {
                "Create"
            } else {
                "OK"
            },
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
