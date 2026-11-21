//! Bounded, opt-in command history. Only new submissions are merged into the store.
use crate::{
    app::App,
    requests::RequestScope,
    task::{Cancellation, Task},
};
use std::{
    collections::VecDeque,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
const LIMIT: usize = 1024 * 1024;
#[derive(Clone)]
enum Operation {
    Load,
    Append(Vec<String>),
    Clear,
}
struct Reply {
    loaded: Option<Vec<String>>,
    cleared: bool,
    warning: Option<String>,
}
#[derive(Default)]
pub struct State {
    scope: RequestScope,
    enabled: bool,
    path: Option<PathBuf>,
    pending: Option<Task<Result<Reply, String>>>,
    queue: VecDeque<Operation>,
    active_clear: bool,
    pub notice: String,
    clearing: bool,
    after_clear: Vec<String>,
}
impl State {
    pub fn pending(&self) -> bool {
        self.pending.is_some() || !self.queue.is_empty()
    }
    pub fn clearing(&self) -> bool {
        self.clearing
    }
    pub(crate) fn cancel(&mut self) {
        self.pending = None;
        self.queue.clear();
        self.scope = RequestScope::default();
    }
}
fn valid(command: &str) -> bool {
    !command.trim().is_empty()
        && command.len() <= super::MAX_BYTES
        && !command.chars().any(char::is_control)
}
fn decode(bytes: &[u8]) -> io::Result<Vec<String>> {
    let root: toml::Table = std::str::from_utf8(bytes)
        .map_err(io::Error::other)?
        .parse()
        .map_err(|_: toml::de::Error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid command history TOML; clear history to reset the stored file",
            )
        })?;
    if root.get("version").and_then(toml::Value::as_integer) != Some(1) {
        return Err(io::Error::other("Unsupported command history format"));
    }
    let entries = root
        .get("commands")
        .and_then(toml::Value::as_array)
        .filter(|v| v.len() <= 100)
        .ok_or_else(|| io::Error::other("Invalid command history"))?;
    let mut result = Vec::new();
    for entry in entries.iter().rev() {
        let command = entry
            .as_str()
            .filter(|s| valid(s))
            .ok_or_else(|| io::Error::other("Invalid command history"))?;
        crate::history::remember(&mut result, command.into(), 100);
    }
    Ok(result)
}
fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let mut file = match crate::filesystem::open_regular(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let before = file.metadata()?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(io::Error::other("Command history exceeds 1 MiB"));
    }
    let after = file.metadata()?;
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || !crate::platform_fs::owns_file(path, &file)
    {
        return Err(io::Error::other("Command history changed while reading"));
    }
    Ok(Some(bytes))
}
fn encode(commands: &mut Vec<String>) -> Vec<u8> {
    loop {
        let mut root = toml::Table::new();
        root.insert("version".into(), 1.into());
        root.insert(
            "commands".into(),
            toml::Value::Array(commands.iter().cloned().map(toml::Value::String).collect()),
        );
        let bytes = toml::to_string(&root)
            .expect("history contains only TOML primitives")
            .into_bytes();
        if bytes.len() <= LIMIT {
            return bytes;
        }
        commands.pop();
    }
}
fn run(path: &Path, operation: Operation, cancel: &Cancellation) -> io::Result<Reply> {
    cancel.check()?;
    if matches!(operation, Operation::Load) {
        return Ok(Reply {
            loaded: Some(
                read(path)?
                    .map(|bytes| decode(&bytes))
                    .transpose()?
                    .unwrap_or_default(),
            ),
            cleared: false,
            warning: None,
        });
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing command history directory"))?;
    fs::create_dir_all(parent)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let _lock = loop {
        cancel.check()?;
        match crate::config::store::SaveLock::try_acquire(path) {
            Ok(lock) => break lock,
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(error) => return Err(error),
        }
    };
    let guard = crate::archive::extract::Destination::capture_with_parent_alias(path)?;
    let original = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.permissions().readonly() {
                return Err(io::Error::other(
                    "Command history must be a writable regular file",
                ));
            }
            Some((
                crate::platform_fs::entry_identity(path)?,
                metadata.len(),
                metadata.modified().ok(),
            ))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let cleared = matches!(operation, Operation::Clear);
    let previous = if cleared { None } else { read(path)? };
    let mut commands = if cleared {
        Vec::new()
    } else {
        previous
            .as_deref()
            .map(decode)
            .transpose()?
            .unwrap_or_default()
    };
    if let Operation::Append(entries) = operation {
        for command in entries {
            if !valid(&command) {
                return Err(io::Error::other("Invalid command history"));
            }
            crate::history::remember(&mut commands, command, 100);
        }
    }
    let bytes = encode(&mut commands);
    let mut stage = crate::publication::StagedFile::create(path)?;
    stage.file.write_all(&bytes)?;
    stage.file.sync_all()?;
    cancel.check()?;
    guard.validate()?;
    if !cleared && read(path)? != previous {
        return Err(io::Error::other("Command history changed before saving"));
    }
    if let Some((id, len, modified)) = original {
        let now = fs::symlink_metadata(path)?;
        if crate::platform_fs::entry_identity(path)? != id
            || now.len() != len
            || now.modified().ok() != modified
        {
            return Err(io::Error::other("Command history changed before saving"));
        }
    }
    if original.is_none() {
        stage.publish_unsynced(path)?;
    } else {
        fs::rename(stage.path(), path)?;
    }
    let warning = crate::platform_fs::sync_parent(path)
        .err()
        .map(|e| format!("History saved; directory synchronization failed: {e}"));
    Ok(Reply {
        loaded: None,
        cleared,
        warning,
    })
}
pub(crate) fn configure(app: &mut App) {
    let enabled = app.config.save_command_history;
    let path = app
        .config_store
        .path()
        .ok()
        .map(|p| p.with_file_name("command-history.toml"));
    let state = &mut app.command_line.persistence;
    if state.enabled == enabled && state.path == path {
        return;
    }
    if !state.clearing {
        state.pending = None;
        state.scope = RequestScope::default();
        state.queue.clear();
    }
    if !enabled {
        state.queue.retain(|op| matches!(op, Operation::Clear));
    }
    state.enabled = enabled;
    state.path = path;
    if enabled {
        state.queue.push_back(Operation::Load);
    }
}
pub(crate) fn submitted(app: &mut App, command: String) {
    configure(app);
    let state = &mut app.command_line.persistence;
    if state.clearing {
        crate::history::remember(&mut state.after_clear, command.clone(), 100);
    }
    if state.enabled {
        match state.queue.back_mut() {
            Some(Operation::Append(commands)) => {
                commands.retain(|entry| entry != &command);
                commands.push(command);
                if commands.len() > 100 {
                    commands.remove(0);
                }
            }
            _ => state.queue.push_back(Operation::Append(vec![command])),
        }
    }
}
pub(crate) fn clear(app: &mut App) {
    configure(app);
    let state = &mut app.command_line.persistence;
    if state.clearing {
        return;
    }
    state.clearing = true;
    state.after_clear.clear();
    state.queue.push_back(Operation::Clear);
}
pub(crate) fn poll(app: &mut App) -> bool {
    configure(app);
    let mut changed = false;
    let language = app.config.language;
    let line = &mut app.command_line;
    let state = &mut line.persistence;
    if let Some(reply) = crate::task::take_reply(&mut state.pending) {
        changed = true;
        match reply.unwrap_or_else(|_| Err("Command history worker stopped".into())) {
            Ok(reply) => {
                if let Some(mut loaded) = reply.loaded {
                    for command in line.history.iter().rev() {
                        crate::history::remember(&mut loaded, command.clone(), 100);
                    }
                    line.history = loaded;
                }
                if reply.cleared {
                    line.history = std::mem::take(&mut state.after_clear);
                    line.recalled = None;
                    state.clearing = false;
                }
                state.notice = reply.warning.unwrap_or_default();
            }
            Err(error) => {
                state.notice = format!("{}: {error}", language.text("Command history"));
                if state.active_clear {
                    state.clearing = false;
                    state.after_clear.clear();
                }
            }
        }
    }
    if state.pending.is_none() {
        if let Some(operation) = state.queue.pop_front() {
            changed = true;
            state.active_clear = matches!(operation, Operation::Clear);
            if let Some(path) = state.path.clone() {
                match state.scope.spawn("command-history", move |cancel| {
                    run(&path, operation, &cancel).map_err(|e| {
                        crate::formatting::tail(
                            &crate::filesystem::display_name(&e.to_string()),
                            2048,
                        )
                    })
                }) {
                    Ok(task) => state.pending = Some(task),
                    Err(error) => {
                        state.notice = error.to_string();
                        state.clearing = false;
                    }
                }
            } else {
                state.notice = language
                    .text("Command history storage is unavailable")
                    .into();
                if matches!(operation, Operation::Clear) {
                    line.history.clear();
                    line.recalled = None;
                    state.clearing = false;
                }
            }
        }
    }
    if changed && !state.notice.is_empty() {
        app.notice = state.notice.clone();
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-command-store-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> PathBuf {
            self.0.join("command-history.toml")
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn append(path: &Path, commands: &[&str]) -> io::Result<Reply> {
        run(
            path,
            Operation::Append(commands.iter().map(|s| (*s).into()).collect()),
            &Cancellation::default(),
        )
    }
    #[test]
    fn store_roundtrip_merges_deltas_and_clear_cannot_republish_cached_entries() {
        let t = Temp::new();
        let p = t.path();
        assert!(run(&p, Operation::Load, &Cancellation::default())
            .unwrap()
            .loaded
            .unwrap()
            .is_empty());
        append(&p, &["echo Ёж 世界", "echo two"]).unwrap();
        let cached = decode(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(cached, ["echo two", "echo Ёж 世界"]);
        append(&p, &["echo third", "echo two"]).unwrap();
        assert_eq!(
            decode(&fs::read(&p).unwrap()).unwrap(),
            ["echo two", "echo third", "echo Ёж 世界"]
        );
        run(&p, Operation::Clear, &Cancellation::default()).unwrap();
        append(&p, &["new only"]).unwrap();
        assert_eq!(decode(&fs::read(&p).unwrap()).unwrap(), ["new only"]);
        assert_eq!(cached.len(), 2);
    }
    #[test]
    fn concurrent_appenders_merge_under_the_store_lock() {
        let t = Temp::new();
        let p = t.path();
        std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..8)
                .map(|i| {
                    let path = &p;
                    scope.spawn(move || append(path, &[&format!("command {i}")]).unwrap())
                })
                .collect();
            for job in jobs {
                job.join().unwrap();
            }
        });
        let mut commands = decode(&fs::read(p).unwrap()).unwrap();
        commands.sort();
        assert_eq!(
            commands,
            (0..8).map(|i| format!("command {i}")).collect::<Vec<_>>()
        );
    }
    #[test]
    fn limits_corruption_cancellation_and_owned_cleanup() {
        let t = Temp::new();
        let p = t.path();
        for i in 0..110 {
            append(&p, &[&format!("{i}{}", "界".repeat(5000))]).unwrap();
        }
        let bytes = fs::read(&p).unwrap();
        assert!(bytes.len() <= LIMIT);
        assert!(decode(&bytes).unwrap().len() <= 100);
        let bad = b"not toml\0";
        fs::write(&p, bad).unwrap();
        assert!(append(&p, &["keep session"]).is_err());
        assert_eq!(fs::read(&p).unwrap(), bad);
        run(&p, Operation::Clear, &Cancellation::default()).unwrap();
        assert!(decode(&fs::read(&p).unwrap()).unwrap().is_empty());
        let before = fs::read(&p).unwrap();
        let cancel = Cancellation::default();
        cancel.cancel();
        assert_eq!(
            run(&p, Operation::Append(vec!["cancelled".into()]), &cancel)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::Interrupted
        );
        assert_eq!(fs::read(&p).unwrap(), before);
        assert!(append(&p, &[&"x".repeat(super::super::MAX_BYTES + 1)]).is_err());
        assert!(append(&p, &["invalid\ncommand"]).is_err());
        assert_eq!(fs::read_dir(&t.0).unwrap().count(), 1);
    }
    #[test]
    fn queued_clear_append_order_and_background_load_preserve_prompt_and_focus() {
        let t = Temp::new();
        let p = t.path();
        append(&p, &["old on disk"]).unwrap();
        let mut app = App::new(t.0.clone()).unwrap();
        app.config_store = crate::config::Store::new(Some(t.0.join("config.conf")));
        app.config.save_command_history = true;
        app.command_line.text = "unexecuted draft".into();
        app.command_line.cursor = 3;
        let owner = app.desktop.active().id;
        poll(&mut app);
        submitted(&mut app, "before clear".into());
        clear(&mut app);
        submitted(&mut app, "after clear".into());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.command_line.persistence.pending() {
            poll(&mut app);
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(decode(&fs::read(&p).unwrap()).unwrap(), ["after clear"]);
        assert_eq!(app.command_line.history(), ["after clear"]);
        assert_eq!(app.command_line.text, "unexecuted draft");
        assert_eq!(app.command_line.cursor, 3);
        assert_eq!(app.desktop.active().id, owner);
        assert!(app.command_line.pending.is_none());
        assert!(app.dialogs().is_empty());
    }
    #[test]
    fn read_only_failure_retains_disk_and_session_history() {
        let t = Temp::new();
        let p = t.path();
        append(&p, &["saved"]).unwrap();
        let before = fs::read(&p).unwrap();
        let permissions = fs::metadata(&p).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&p, readonly).unwrap();
        assert!(append(&p, &["not written"]).is_err());
        assert_eq!(fs::read(&p).unwrap(), before);
        fs::set_permissions(&p, permissions).unwrap();
        assert_eq!(fs::read_dir(&t.0).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn links_are_never_read_or_replaced() {
        let t = Temp::new();
        let p = t.path();
        let outside = t.0.join("other");
        fs::write(&outside, "untouched").unwrap();
        std::os::unix::fs::symlink(&outside, &p).unwrap();
        assert!(read(&p).is_err());
        assert!(append(&p, &["test"]).is_err());
        assert!(run(&p, Operation::Clear, &Cancellation::default()).is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "untouched");
    }
}
