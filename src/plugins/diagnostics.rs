//! Explicit, read-only plugin diagnostics using existing bounded tool probes.
use super::*;
use crate::utilities::information::Fields;
#[derive(Default)]
pub(crate) struct Input {
    entries: Vec<(&'static str, bool)>,
    dos: crate::dos::Preferences,
    video: crate::video::Preferences,
    bluetooth: String,
}
impl Input {
    pub fn capture(app: &crate::app::App) -> Self {
        Self {
            entries: app
                .desktop
                .plugins
                .definitions()
                .map(|d| (d.title, app.config.plugin_enabled(d.id)))
                .collect(),
            dos: app.config.dos.clone(),
            video: app.config.video.clone(),
            bluetooth: app.bluetooth.view.notice.clone(),
        }
    }
}
pub(crate) fn collect(input: Input, cancel: &crate::task::Cancellation) -> Fields {
    let mut fields = vec![(
        "Checks".into(),
        "Version checks only; no game or transfer started".into(),
    )];
    fields.extend(input.entries.into_iter().map(|(name, enabled)| {
        (
            name.into(),
            if enabled { "Enabled" } else { "Disabled" }.into(),
        )
    }));
    fields.push((
        "Music playback".into(),
        if music_playback_available() {
            "Included; device opens only on Play"
        } else {
            "Unavailable in this build"
        }
        .into(),
    ));
    fields.push((
        "Acoustic support".into(),
        if cfg!(feature = "acoustic") {
            "Included; experimental and disabled by default"
        } else {
            "Not compiled; rebuild with --features acoustic"
        }
        .into(),
    ));
    let bluetooth_requirements = if cfg!(target_os = "linux") {
        "BlueZ system service and a Bluetooth Classic adapter; Refresh in Bluetooth transfers checks paired devices"
    } else if cfg!(windows) {
        "Bluetooth Classic adapter and system drivers; Refresh in Bluetooth transfers checks paired devices"
    } else {
        "Allow Bluetooth access for your terminal; Refresh in Bluetooth transfers checks paired devices"
    };
    fields.push((
        "Bluetooth requirements".into(),
        bluetooth_requirements.into(),
    ));
    fields.push((
        "Bluetooth device".into(),
        "Not probed; open Bluetooth transfers and Refresh".into(),
    ));
    if !input.bluetooth.is_empty() {
        fields.push(("Bluetooth session status".into(), input.bluetooth));
    }
    if cancel.cancelled() {
        return fields;
    }
    fields.extend(crate::video::diagnostics(&input.video, cancel));
    if cancel.cancelled() {
        return fields;
    }
    fields.extend(crate::dos::diagnostics(&input.dos, cancel));
    fields.push((
        "Hardware encoders".into(),
        "Open Video Merge Settings to verify encoders for your output".into(),
    ));
    fields.push(("Setup help".into(),"Configure executable paths in DOS launcher or Video Merge Settings; install missing tools through your system package manager".into()));
    fields
}
