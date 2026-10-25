use crate::{filesystem::display_name, formatting::local_timestamp};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc,
};

#[derive(Debug)]
pub struct FileInfo {
    pub lines: Vec<String>,
}

pub fn inspect(path: &Path) -> io::Result<FileInfo> {
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
    lines.push("Read-only information · no attributes are changed.".into());
    Ok(FileInfo { lines })
}

pub struct InfoDialog {
    pub(crate) requests: crate::requests::RequestScope,
    pub path: PathBuf,
    pub contents: Option<Result<FileInfo, String>>,
    pub offset: usize,
    pub horizontal: u16,
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
            pending: Some(pending),
        })
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
