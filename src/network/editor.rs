use super::{
    remote::{Connection, Member},
    workspace::Workspace,
};
use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
};
pub struct Origin {
    pub path: PathBuf,
    pub label: String,
    pub base: PathBuf,
    workspace: Workspace,
    connection: Arc<Connection>,
    member: Member,
    original: Mutex<Vec<u8>>,
}
impl Origin {
    pub fn prepare(
        connection: Arc<Connection>,
        member: Member,
        base: PathBuf,
        cancel: &crate::task::Cancellation,
    ) -> io::Result<Arc<Self>> {
        if !member.regular || member.link {
            return Err(io::Error::other("Remote editing requires a regular file"));
        }
        let workspace = Workspace::create(std::env::temp_dir())?;
        let name = crate::archive::extract::component(member.path.name().as_bytes())?;
        let path = workspace.root.join(name);
        let mut bytes = Vec::new();
        connection.with(|backend| {
            backend.read(
                &member.path,
                &mut bytes,
                cancel,
                crate::limits::EDITOR_BYTES as u64,
            )
        })?;
        cancel.check()?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(Arc::new(Self {
            label: connection.label(&member.path),
            path,
            base,
            workspace,
            connection,
            member,
            original: Mutex::new(bytes),
        }))
    }
    pub fn save(&self, cancel: &crate::task::Cancellation) -> Result<Option<String>, String> {
        let run = || -> io::Result<Option<String>> {
            self.workspace.validate()?;
            let mut bytes = Vec::new();
            use std::io::Read;
            crate::filesystem::open_regular(&self.path)?
                .take(crate::limits::EDITOR_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > crate::limits::EDITOR_BYTES {
                return Err(io::Error::other(
                    "Remote editor output exceeds the editor limit",
                ));
            }
            let mut original = self
                .original
                .lock()
                .map_err(|_| io::Error::other("Remote editor state unavailable"))?;
            self.connection.with(|backend| {
                let member = backend.stat(&self.member.path, cancel)?.ok_or_else(|| io::Error::other("Remote file disappeared"))?;
                if !member.regular || member.link { return Err(io::Error::other("Remote editor destination is no longer a regular file")); }
                let mut current = Vec::new();
                backend.read(&self.member.path, &mut current, cancel, crate::limits::EDITOR_BYTES as u64)?;
                if current != *original {
                    return Err(io::Error::other("Remote file changed outside DN; unsaved edits retained. Reopen it or export with Save As."));
                }
                let stage = super::transfer::stage_directory(backend, &self.member.path.parent())?;
                let file = stage.child("content")?;
                let result = (|| {
                    let written = backend.write_new(&file, &mut bytes.as_slice(), cancel)?;
                    if written != bytes.len() as u64 { return Err(io::Error::other("Incomplete remote upload; existing file retained")); }
                    cancel.check()?;
                    let mut check = Vec::new();
                    backend.read(&self.member.path, &mut check, cancel, crate::limits::EDITOR_BYTES as u64)?;
                    if check != *original || backend.stat(&self.member.path, cancel)?.as_ref() != Some(&member) {
                        return Err(io::Error::other("Remote file changed before publication; existing file retained"));
                    }
                    backend.publish(&file, &self.member.path, true)?;
                    *original = bytes;
                    Ok(None)
                })();
                let _ = backend.remove(&file, false);
                let _ = backend.remove(&stage, true);
                result
            })
        };
        run().map_err(|e| e.to_string())
    }
}
