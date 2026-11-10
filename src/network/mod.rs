//! Network snapshots and typed remote locations. Native paths remain native.
pub mod information;
pub(crate) mod profiles;
pub mod remote;
pub(crate) mod task;
pub mod traffic;

pub const HELP:&str="Remote connections: SFTP over SSH, explicit FTPS, or unencrypted FTP.\nEnter host, port, username, initial absolute remote directory and password or SSH key.\nUnknown SSH host keys require explicit verification before authentication; changed pinned keys are refused. FTPS validates certificates and never falls back to plain FTP.\nSave profile stores connection metadata and trusted fingerprint, never passwords or key passphrases.\nProfiles opens a saved-profile list: Select, Edit, Delete or New. Save profile applies edits and renames. Deleting a profile does not close open connections.\nEach pane can browse a remote directory. Enter navigates, F3 previews, F4 edits a private working copy, F5 transfers selections.\nCtrl+PgUp disconnects; the shell working directory always remains local.\nRemote operations are cancellable and never run during drawing.";
pub(crate) mod editor;
pub(crate) mod transfer;
pub(crate) mod workspace;

pub(crate) mod controller;
pub(crate) use controller::{poll, prompt_copy, prompt_mutation, submit_copy, view, State};

pub const NATIVE_REQUIRED: &str =
    "Download remote items to disk first (F5) before using native-only commands.";

pub(crate) fn terminal(app: &mut crate::app::App, profile: remote::Profile) -> std::io::Result<()> {
    profile.validate()?;
    if profile.protocol != remote::Protocol::Sftp {
        return Err(std::io::Error::other(
            "SSH terminal requires an SFTP/SSH profile",
        ));
    }
    let mut args = vec![
        "-oStrictHostKeyChecking=ask".into(),
        "-p".into(),
        profile.port.to_string().into(),
        "-l".into(),
        profile.user.into(),
    ];
    if let Some(key) = profile.key {
        args.push("-i".into());
        args.push(key.into_os_string());
    }
    args.push("--".into());
    args.push(profile.host.into());
    app.modals.set_plugin_dialog(None);
    app.sync_modals();
    app.user_menu.pending = Some(crate::user_menu::runner::Plan {
        cwd: app.panels[app.active].path.clone(),
        commands: vec![crate::user_menu::runner::CommandSpec {
            program: "ssh".into(),
            args,
        }],
        env: vec![],
        list_files: vec![],
    });
    Ok(())
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod sftp_test;
