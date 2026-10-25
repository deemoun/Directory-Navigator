use crate::{app::App, task::Task, viewer::Viewer};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::TryRecvError,
    time::{Duration, Instant, SystemTime},
};
#[derive(PartialEq, Eq)]
struct Signature {
    path: PathBuf,
    size: Option<u64>,
    modified: Option<SystemTime>,
    directory: bool,
    link: bool,
}
enum Probe {
    File,
    Information(Vec<String>),
}
pub struct QuickView {
    pub(crate) requests: crate::requests::RequestScope,
    pub source: usize,
    pub viewer: Option<Viewer>,
    pub lines: Vec<String>,
    pub area: crate::presentation::layout::Rect,
    signature: Option<Signature>,
    due: Option<Instant>,
    probe: Option<Task<Result<Probe, String>>>,
}
impl QuickView {
    pub fn new(source: usize) -> Self {
        Self {
            requests: crate::requests::RequestScope::new(crate::requests::Owner::Panel(source)),
            source,
            viewer: None,
            lines: vec!["No entry".into()],
            area: Default::default(),
            signature: None,
            due: None,
            probe: None,
        }
    }
    pub fn pending(&self) -> bool {
        self.due.is_some()
            || self.probe.is_some()
            || self.viewer.as_ref().is_some_and(|v| v.contents.is_none())
    }
    pub fn path(&self) -> Option<&std::path::Path> {
        self.signature.as_ref().map(|s| s.path.as_path())
    }
    pub fn layout(&mut self, area: crate::presentation::layout::Rect) {
        self.area = area;
        if let Some(v) = &mut self.viewer {
            v.layout(area.width.saturating_sub(2));
        }
    }
    pub fn step(&mut self, amount: isize) {
        if let Some(v) = &mut self.viewer {
            v.step(amount);
        }
    }
    pub fn poll(&mut self, panel: &crate::panel::Panel, now: Instant) -> bool {
        let signature = panel.entries.get(panel.cursor).map(|e| Signature {
            path: e.path.clone(),
            size: e.size,
            modified: e.modified,
            directory: e.directory,
            link: e.symlink,
        });
        let mut changed = false;
        if self.signature != signature {
            self.signature = signature;
            self.viewer = None;
            self.probe = None;
            self.lines = vec!["Waiting for preview…".into()];
            self.due = Some(now + Duration::from_millis(150));
            changed = true;
        }
        if self.due.is_some_and(|due| now >= due) {
            self.due = None;
            if let Some(signature) = &self.signature {
                let path = signature.path.clone();
                match self.requests.spawn(
                    "dn-preview-info",
                    move |cancel| -> Result<Probe, String> {
                        cancel.check().map_err(|e| e.to_string())?;
                        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                        if metadata.is_file() {
                            return Ok(Probe::File);
                        }
                        let mut lines =
                            vec![crate::filesystem::display_name(&path.to_string_lossy())];
                        if metadata.is_symlink() {
                            lines.push("Symbolic link — target not previewed".into());
                            lines.push(format!(
                                "Target: {}",
                                fs::read_link(&path)
                                    .map(|p| crate::filesystem::display_name(&p.to_string_lossy()))
                                    .unwrap_or_else(|e| format!("Unavailable: {e}"))
                            ));
                        } else if metadata.is_dir() {
                            lines.push("Directory".into());
                            lines.push(
                                "Use Calculate directory sizes for a recursive snapshot.".into(),
                            );
                        } else {
                            lines.push("Special file — content not previewed".into());
                        }
                        Ok(Probe::Information(lines))
                    },
                ) {
                    Ok(task) => self.probe = Some(task),
                    Err(e) => self.lines = vec![format!("Preview unavailable: {e}")],
                }
            } else {
                self.lines = vec!["No entry".into()];
            }
            changed = true;
        }
        let result = self.probe.as_ref().and_then(|p| match p.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("Preview worker stopped".into())),
        });
        if let Some(result) = result {
            self.probe = None;
            changed = true;
            match result {
                Ok(Probe::Information(lines)) => self.lines = lines,
                Ok(Probe::File) => {
                    if let Some(signature) = &self.signature {
                        match Viewer::open_preview(signature.path.clone()) {
                            Ok(viewer) => {
                                viewer
                                    .requests
                                    .bind(crate::requests::Owner::Panel(self.source));
                                self.viewer = Some(viewer);
                            }
                            Err(e) => self.lines = vec![format!("Preview unavailable: {e}")],
                        }
                    }
                }
                Err(e) => self.lines = vec![format!("Preview unavailable: {e}")],
            }
        }
        if let Some(result) = self.viewer.as_mut().and_then(|v| v.poll()) {
            changed = true;
            if let Err(error) = result {
                self.viewer = None;
                self.lines = vec![format!("Preview unavailable: {error}")];
            }
        }
        changed
    }
}
pub fn toggle(app: &mut App) {
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    app.quick_view = if app.quick_view.is_some() {
        None
    } else {
        Some(QuickView::new(app.active))
    };
}
pub fn poll(app: &mut App) -> bool {
    if !matches!(
        crate::focus::current(app),
        crate::focus::Focus::Manager | crate::focus::Focus::QuickSearch
    ) {
        return false;
    }
    if app
        .quick_view
        .as_ref()
        .is_some_and(|q| q.source != app.active)
    {
        app.quick_view = None;
        return true;
    }
    app.quick_view
        .as_mut()
        .is_some_and(|q| q.poll(&app.panels[q.source], Instant::now()))
}
