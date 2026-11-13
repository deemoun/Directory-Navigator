pub mod attributes;

use crate::{filesystem::display_name, formatting::local_timestamp};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc,
};

#[derive(Debug)]
pub struct FileInfo {
    pub lines: Vec<String>,
    pub attributes: Option<attributes::Attributes>,
    pub attribute_notice: String,
    applied: bool,
}

pub fn inspect(path: &Path) -> io::Result<FileInfo> {
    inspect_with(path, None)
}
fn inspect_with(path: &Path, retained: Option<attributes::Attributes>) -> io::Result<FileInfo> {
    let path = std::path::absolute(path)?;
    let metadata = fs::symlink_metadata(&path)?;
    let kind = if metadata.is_symlink() {
        "Symbolic link"
    } else if metadata.is_dir() {
        "Directory"
    } else if metadata.is_file() {
        "Regular file"
    } else {
        "Special file"
    };
    let size = if metadata.is_dir() {
        "— (directory contents not counted)".into()
    } else if metadata.is_symlink() {
        format!("{} bytes (link entry)", metadata.len())
    } else {
        format!("{} bytes", metadata.len())
    };
    let modified = match metadata.modified() {
        Ok(time) => local_timestamp(time)
            .map(|time| time.format("%Y-%m-%d %H:%M:%S %:z").to_string())
            .unwrap_or_else(|| "Unavailable (out of range)".into()),
        Err(error) => format!("Unavailable ({error})"),
    };
    let mut lines = vec![
        format!("Full path: {}", display_name(&path.to_string_lossy())),
        format!("Entry type: {kind}"),
        format!("Size: {size}"),
        format!("Modified (local): {modified}"),
    ];
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        let mut symbolic: Vec<char> = [
            0o400, 0o200, 0o100, 0o040, 0o020, 0o010, 0o004, 0o002, 0o001,
        ]
        .iter()
        .enumerate()
        .map(|(index, bit)| {
            if mode & bit == 0 {
                '-'
            } else {
                ['r', 'w', 'x'][index % 3]
            }
        })
        .collect();
        for (index, bit, lower, upper) in [
            (2, 0o4000, 's', 'S'),
            (5, 0o2000, 's', 'S'),
            (8, 0o1000, 't', 'T'),
        ] {
            if mode & bit != 0 {
                symbolic[index] = if symbolic[index] == 'x' { lower } else { upper };
            }
        }
        lines.push(format!(
            "Permissions: {} (0o{:04o})",
            symbolic.iter().collect::<String>(),
            mode & 0o7777
        ));
        if metadata.is_symlink() {
            lines.push("Permissions describe the link, not its target.".into());
        }
    }
    #[cfg(not(unix))]
    lines.push(format!(
        "Read-only attribute: {}",
        if metadata.permissions().readonly() {
            "Yes"
        } else {
            "No"
        }
    ));
    if metadata.is_symlink() {
        lines.push(match fs::read_link(&path) {
            Ok(target) => format!(
                "Link target (not followed): {}",
                display_name(&target.to_string_lossy())
            ),
            Err(error) => format!("Link target: Unavailable ({error})"),
        });
    }
    lines.push(String::new());
    // Keep a successfully changed handle: on macOS a newly removed read bit
    // can prevent reopening, but the existing handle can still restore it.
    let opened = retained
        .filter(|a| a.owns(&path))
        .map_or_else(|| attributes::Attributes::open(&path), Ok);
    let (attributes, attribute_notice) = match opened {
        Ok(value) => (Some(value), String::new()),
        Err(error) => (None, error.to_string()),
    };
    if !attribute_notice.is_empty() {
        lines.push(format!(
            "Attributes unavailable: {}",
            display_name(&attribute_notice)
        ));
    }
    Ok(FileInfo {
        lines,
        attributes,
        attribute_notice,
        applied: false,
    })
}

pub struct InfoDialog {
    pub(crate) requests: crate::requests::RequestScope,
    pub path: PathBuf,
    pub contents: Option<Result<FileInfo, String>>,
    pub offset: usize,
    pub horizontal: u16,
    pub focus: usize,
    pub editing: bool,
    pub notice: String,
    pending: Option<crate::task::Task<Result<FileInfo, String>>>,
}
impl InfoDialog {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let source = path.clone();
        let requests = crate::requests::RequestScope::default();
        let pending = requests.spawn("dn-file-info", move |_| {
            inspect(&source).map_err(|e| e.to_string())
        })?;
        Ok(Self {
            requests,
            path,
            contents: None,
            offset: 0,
            horizontal: 0,
            focus: 0,
            editing: false,
            notice: String::new(),
            pending: Some(pending),
        })
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn apply(&mut self) {
        if self.busy() {
            return;
        }
        let Some(Ok(info)) = &mut self.contents else {
            return;
        };
        let Some(mut attributes) = info.attributes.clone() else {
            return;
        };
        if !attributes.changed() {
            return;
        }
        let path = self.path.clone();
        match self.requests.spawn("dn-attributes", move |_| {
            let outcome = attributes.apply(&path);
            let applied = outcome.is_ok();
            match inspect_with(&path, applied.then_some(attributes)) {
                Ok(mut info) => {
                    info.applied = applied;
                    info.attribute_notice = match outcome {
                        Ok(()) => "Attributes applied.".into(),
                        Err(e) => e.to_string(),
                    };
                    Ok(info)
                }
                Err(e) => Err(if applied {
                    format!("Attributes applied, but metadata could not be refreshed: {e}")
                } else {
                    e.to_string()
                }),
            }
        }) {
            Ok(task) => {
                self.pending = Some(task);
                self.notice = "Applying attributes…".into();
            }
            Err(e) => self.notice = e.to_string(),
        }
    }
    pub fn poll(&mut self) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        let result = match pending.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("File information worker stopped.".into()),
        };
        if let Ok(info) = &result {
            self.notice = info.attribute_notice.clone();
        }
        self.contents = Some(result);
        self.pending = None;
        true
    }
    pub fn step(&mut self, amount: isize) {
        let last = match &self.contents {
            Some(Ok(info)) => info.lines.len().saturating_sub(1),
            _ => 0,
        };
        self.offset = self.offset.saturating_add_signed(amount).min(last);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Edit,
    Toggle(usize),
    Apply,
    Cancel,
}
pub fn action(app: &mut crate::app::App, action: Action) {
    let Some(info) = app.file_info_mut() else {
        return;
    };
    match action {
        Action::Edit => {
            info.editing = true;
            info.focus = 0;
        }
        Action::Toggle(index) if !info.busy() => {
            info.focus = index;
            if let Some(Ok(contents)) = &mut info.contents {
                if let Some(attributes) = &mut contents.attributes {
                    attributes.toggle(index);
                }
            }
        }
        Action::Apply => {
            info.focus = attributes::FIELDS.len();
            info.apply();
        }
        Action::Toggle(_) => {}
        Action::Cancel => {
            info.editing = false;
            info.focus = 0;
            if let Some(Ok(contents)) = &mut info.contents {
                if let Some(a) = &mut contents.attributes {
                    a.reset();
                }
            }
        }
    }
}

pub(crate) fn poll(app: &mut crate::app::App) -> bool {
    let Some(info) = app.file_info_mut() else {
        return false;
    };
    let changed = info.poll();
    if changed
        && info
            .contents
            .as_ref()
            .is_some_and(|r| r.as_ref().is_ok_and(|i| i.applied))
    {
        let parent = info.path.parent().map(Path::to_path_buf);
        for panel in &mut app.panels {
            if !panel.is_virtual() && Some(&panel.path) == parent.as_ref() {
                panel.refresh(app.config.show_hidden);
            }
        }
    }
    changed
}
