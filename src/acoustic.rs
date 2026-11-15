//! Host-owned experimental acoustic sessions. No devices are opened at startup.
use crate::{
    app::App,
    plugins::{Binding, Owner, ACOUSTIC_TRANSFERS},
};
use std::path::PathBuf;
#[cfg(all(
    feature = "acoustic",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
mod audio;
#[cfg(all(
    feature = "acoustic",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
mod protocol;
#[cfg(all(
    feature = "acoustic",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
mod session;
pub(crate) const fn available() -> bool {
    cfg!(all(
        feature = "acoustic",
        any(
            target_os = "macos",
            target_os = "linux",
            target_os = "windows"
        )
    ))
}
#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pub generation: u64,
    pub busy: bool,
    pub notice: String,
    pub cause: Option<String>,
    pub code: Option<String>,
    pub offer: Option<String>,
    pub received: Option<String>,
    pub source: String,
    pub devices: String,
    pub input: u8,
    pub output: u8,
    pub packets: (u32, u32),
    pub conflict: Option<crate::operations::conflict::Conflict>,
}
pub(crate) enum Action {
    Capture,
    SendText(String),
    SendFile,
    Receive,
    Stop,
    Confirm(bool),
    Approve { accept: bool, destination: PathBuf },
    Conflict(crate::operations::conflict::Decision),
    Dismiss,
    Tree,
}
#[derive(Default)]
pub(crate) struct State {
    pub view: Snapshot,
    #[cfg(all(
        feature = "acoustic",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    service: Option<session::Service>,
    #[cfg(all(
        feature = "acoustic",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    source: Option<session::Payload>,
    #[cfg(all(
        feature = "acoustic",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    music: Option<crate::plugins::mp3_player::AcousticPause>,
}
pub(crate) fn opened(app: &mut App) {
    #[cfg(all(
        feature = "acoustic",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    session::reset(app);
    let generation = app.acoustic.view.generation.wrapping_add(1);
    app.acoustic.view = Snapshot {
        generation,
        ..Default::default()
    };
}
pub(crate) fn active(app: &App) -> bool {
    app.acoustic.view.busy
}
fn alive(app: &App, b: Binding) -> bool {
    b.plugin == ACOUSTIC_TRANSFERS
        && app.config.plugin_enabled(b.plugin)
        && matches!(b.owner,Owner::Document(id) if app.desktop.contains(id))
        && app.desktop.plugins.binding(b.plugin) == Some(b)
}
pub(crate) fn action(app: &mut App, b: Binding, generation: u64, action: Action) {
    if !alive(app, b) || app.acoustic.view.generation != generation {
        return;
    }
    if !available() {
        app.acoustic.view.notice =
            "Acoustic support is unavailable in this build; rebuild with --features acoustic."
                .into();
        return;
    }
    #[cfg(all(
        feature = "acoustic",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    session::action(app, b, action);
    #[cfg(not(all(
        feature = "acoustic",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    )))]
    match action {
        Action::SendText(text) => drop(text),
        Action::Approve {
            accept,
            destination,
        } => {
            let _ = (accept, destination);
        }
        Action::Confirm(v) => {
            let _ = v;
        }
        Action::Conflict(v) => {
            let _ = v;
        }
        _ => {}
    }
}
pub(crate) fn poll(app: &mut App) -> bool {
    #[cfg(all(
        feature = "acoustic",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    {
        session::poll(app)
    }
    #[cfg(not(all(
        feature = "acoustic",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    )))]
    {
        let _ = app;
        false
    }
}
pub(crate) fn picked(app: &mut App, owner: crate::desktop::WindowId, path: PathBuf) {
    let b = Binding {
        plugin: ACOUSTIC_TRANSFERS,
        owner: Owner::Document(owner),
    };
    if alive(app, b) {
        crate::plugins::host::dispatch(app, b, crate::plugins::Event::AcousticDirectory(path));
    }
}
