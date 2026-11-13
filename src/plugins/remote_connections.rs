//! Owned connection dialog; protocol work never runs on the UI thread.
mod profiles;
use super::*;
use crate::{
    network::{
        remote::{self, ConnectError, Connection, Credentials, Profile, Protocol, RemotePath},
        task::Task,
    },
    painting::Paragraph,
    requests::RequestScope,
    turbo::dialog_spec::{centered, Control, ControlKind},
};
use crossterm::event::{KeyCode as K, KeyModifiers as M};
use std::sync::Arc;
use zeroize::Zeroize;
const CANCEL: usize = 10;
const HELP: usize = 11;
const CONNECT: usize = 20;
const SAVE: usize = 21;
const NEXT: usize = 22;
const PROTOCOL: usize = 23;
const TRUST: usize = 24;
const TERMINAL: usize = 25;
const TRUST_SAVE: usize = 26;
const LABELS: [&str; 7] = [
    "Profile name",
    "Host",
    "Port",
    "Username",
    "Remote directory",
    "Private key (optional)",
    "Password / key passphrase",
];
#[derive(Default)]
pub struct RemoteConnections {
    fields: Vec<String>,
    cursors: Vec<usize>,
    focus: usize,
    protocol: usize,
    task: Option<Task<Result<Arc<Connection>, ConnectError>>>,
    wake: Option<Instant>,
    scope: RequestScope,
    observed: Option<String>,
    ready: Option<Arc<Connection>>,
    pinned: Option<String>,
    pinned_endpoint: Option<(String, u16)>,
    profiles: Vec<Profile>,
    profile_index: usize,
    profile_offset: usize,
    profile_page: usize,
    profiles_open: bool,
    delete_profile: Option<Profile>,
    editing: Option<Profile>,
    last_profile_click: Option<(usize, Instant)>,
    storage: Option<PathBuf>,
    notice: String,
    ftp_confirmed: bool,
}
impl RemoteConnections {
    fn new_profile() -> Profile {
        Profile {
            name: "New connection".into(),
            protocol: Protocol::Sftp,
            host: String::new(),
            port: 22,
            user: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_default(),
            directory: RemotePath::new("/").unwrap(),
            key: None,
            fingerprint: None,
        }
    }
    fn profile(&self) -> std::io::Result<Profile> {
        let p = Profile {
            name: self.fields[0].clone(),
            host: self.fields[1].clone(),
            port: self.fields[2].parse().map_err(std::io::Error::other)?,
            user: self.fields[3].clone(),
            directory: RemotePath::new(&self.fields[4])?,
            key: (!self.fields[5].is_empty()).then(|| self.fields[5].clone().into()),
            protocol: [Protocol::Sftp, Protocol::Ftps, Protocol::Ftp][self.protocol],
            fingerprint: if self.protocol == 0
                && self.pinned_endpoint.as_ref().is_some_and(|(host, port)| {
                    host == &self.fields[1] && self.fields[2].parse::<u16>().ok() == Some(*port)
                }) {
                self.pinned.clone()
            } else {
                None
            },
        };
        p.validate()?;
        Ok(p)
    }
    fn apply(&mut self, p: Profile) {
        if let Some(secret) = self.fields.get_mut(6) {
            secret.zeroize();
        }
        self.protocol = match p.protocol {
            Protocol::Sftp => 0,
            Protocol::Ftps => 1,
            Protocol::Ftp => 2,
        };
        self.fields = vec![
            p.name,
            p.host,
            p.port.to_string(),
            p.user,
            p.directory.as_str().into(),
            p.key
                .map_or(String::new(), |p| p.to_string_lossy().into_owned()),
            String::new(),
        ];
        self.cursors = self.fields.iter().map(String::len).collect();
        self.pinned = p.fingerprint;
        self.pinned_endpoint = Some((self.fields[1].clone(), self.fields[2].parse().unwrap_or(0)));
        self.observed = None;
        self.ftp_confirmed = false;
    }
    fn connect(&mut self) {
        let result = (|| {
            let profile = self.profile()?;
            let credentials = Credentials {
                profile,
                password: zeroize::Zeroizing::new(self.fields[6].clone()),
            };
            Task::spawn(&self.scope, "dn-remote-connect", move |cancel| {
                remote::connect(credentials, &cancel)
            })
        })();
        match result {
            Ok(task) => {
                self.wake = Some(Instant::now() + Duration::from_millis(30));
                self.task = Some(task);
                self.notice = "Connecting…".into();
            }
            Err(e) => self.notice = e.to_string(),
        }
    }
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        match id {
            CANCEL => {
                self.task = None;
                return vec![Effect::Close];
            }
            HELP => {
                return vec![Effect::Help {
                    title: " Remote connection help ",
                    text: crate::network::HELP.into(),
                }]
            }
            TERMINAL => match self.profile() {
                Ok(p) if p.protocol == Protocol::Sftp => return vec![Effect::RemoteTerminal(p)],
                Ok(_) => self.notice = "SSH terminal requires an SFTP/SSH profile".into(),
                Err(e) => self.notice = e.to_string(),
            },
            CONNECT => {
                if self.task.is_none() {
                    if self.protocol == 2 && !self.ftp_confirmed {
                        self.ftp_confirmed = true;
                        self.notice="FTP sends passwords and data without encryption. Press Connect again to continue, or choose FTPS/SFTP.".into();
                    } else {
                        self.connect();
                    }
                }
            }
            TRUST | TRUST_SAVE => {
                self.pinned = self.observed.take();
                self.pinned_endpoint =
                    Some((self.fields[1].clone(), self.fields[2].parse().unwrap_or(0)));
                if id == TRUST_SAVE {
                    let result = self.save_profile(ctx.language);
                    match result {
                        Ok(Some(warning)) => {
                            self.notice = warning;
                            return vec![];
                        }
                        Err(e) => {
                            self.notice = e.to_string();
                            return vec![];
                        }
                        Ok(None) => {}
                    }
                }
                self.connect();
            }
            SAVE => match self.save_profile(ctx.language) {
                Ok(warning) => {
                    self.notice = warning
                        .unwrap_or_else(|| "Connection profile saved without passwords".into())
                }
                Err(error) => self.notice = error.to_string(),
            },
            NEXT => self.show_profiles(),
            PROTOCOL => {
                self.ftp_confirmed = false;
                self.protocol = (self.protocol + 1) % 3;
                self.fields[2] = if self.protocol == 0 { "22" } else { "21" }.into();
                self.cursors[2] = self.fields[2].len();
                self.notice.clear();
            }
            100..=106 => self.focus = id - 100,
            _ => {}
        }
        let _ = ctx;
        vec![]
    }
}
impl Drop for RemoteConnections {
    fn drop(&mut self) {
        if let Some(secret) = self.fields.get_mut(6) {
            secret.zeroize();
        }
    }
}

impl ToolPlugin for RemoteConnections {
    fn definition(&self) -> Definition {
        Definition::builtin(
            REMOTE_CONNECTIONS,
            "Remote connections",
            Presentation::Dialog,
        )
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, ctx: &Context) {
        self.scope = RequestScope::default();
        self.scope.bind(match ctx.binding.owner {
            Owner::Document(id) => crate::requests::Owner::Document(id),
            Owner::Modal(id) => crate::requests::Owner::Modal(id),
        });
        self.task = None;
        self.ftp_confirmed = false;
        self.focus = 0;
        self.notice.clear();
        self.storage = ctx
            .storage
            .as_ref()
            .map(|p| p.with_file_name("remote-connections.conf"));
        self.profiles = match self
            .storage
            .as_ref()
            .map(|p| crate::network::profiles::load(p))
        {
            Some(Ok(profiles)) => profiles,
            Some(Err(error)) => {
                self.notice = error.to_string();
                Vec::new()
            }
            None => Vec::new(),
        };
        self.profile_index = 0;
        self.editing = self.profiles.first().cloned();
        let p = self.editing.clone().unwrap_or_else(Self::new_profile);
        self.profiles_open = false;
        self.delete_profile = None;
        self.apply(p);
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        if self.profiles_open {
            return self.profiles_event(event, ctx);
        }
        if matches!(event, Event::Tick) {
            self.wake = Some(ctx.now + Duration::from_millis(30));
        }
        if ctx.active {
            if let Some(connection) = self.ready.take() {
                return vec![Effect::RemoteConnected(connection)];
            }
        }
        if let Some(task) = &self.task {
            match task.try_recv() {
                Ok(Ok(connection)) => {
                    self.task = None;
                    if ctx.active {
                        return vec![Effect::RemoteConnected(connection)];
                    }
                    self.ready = Some(connection);
                    self.notice = "Connection ready".into();
                }
                Ok(Err(ConnectError::HostKey(key))) => {
                    self.task = None;
                    self.observed = Some(key);
                    self.focus = CANCEL;
                    self.notice="Verify this SSH host key before trusting it. Credentials have not been sent.".into();
                }
                Ok(Err(ConnectError::Failure(e))) => {
                    self.task = None;
                    self.notice = e.to_string();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.task = None;
                    self.notice = "Connection cancelled or worker stopped".into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        match event {
            Event::Action(ActionId(id)) => self.action(id, ctx),
            Event::Mouse(mouse, Some(ActionId(id)))
                if mouse.kind
                    == crossterm::event::MouseEventKind::Down(
                        crossterm::event::MouseButton::Left,
                    ) =>
            {
                self.action(id, ctx)
            }
            Event::Focus(focus) => {
                self.focus = focus;
                vec![]
            }
            Event::Key(k) if k.code == K::F(1) => self.action(HELP, ctx),
            Event::Key(k)
                if self.focus < 7
                    && self.task.is_none()
                    && self.observed.is_none()
                    && !k.modifiers.intersects(M::ALT | M::CONTROL) =>
            {
                let i = self.focus;
                if self.fields[i].len() < 4096 || !matches!(k.code, K::Char(_)) {
                    crate::dialog::edit_input(&mut self.fields[i], &mut self.cursors[i], k.code);
                }
                vec![]
            }
            Event::Paste(text)
                if self.focus < 7 && self.task.is_none() && self.observed.is_none() =>
            {
                let i = self.focus;
                if self.fields[i].len() + text.len() <= 4096 {
                    crate::dialog::paste_input(&mut self.fields[i], &mut self.cursors[i], text);
                }
                vec![]
            }
            _ => vec![],
        }
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        if self.profiles_open {
            return self.profiles_controls(ctx);
        }
        let r = centered(ctx.area, 76, 23);
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let mut s = ctx.dialog(
            r,
            self.focus,
            ctx.language.text("Remote connections"),
            if self.observed.is_some() {
                CANCEL
            } else {
                CONNECT
            },
            CANCEL,
        );
        if let Some(key) = &self.observed {
            s.caption(
                Paragraph::new(format!(
                    "{}\n{}\n\n{}",
                    ctx.language.text("SSH host fingerprint"),
                    key,
                    ctx.language.text(&self.notice)
                ))
                .style(style(0x70)),
                Rect::new(r.x + 3, r.y + 3, r.width - 6, 10),
            );
            for (id, label, x) in [
                (TRUST, "Trust once", 2),
                (TRUST_SAVE, "Trust and save", 26),
                (CANCEL, "Cancel", 54),
            ] {
                s.control(Control::new(
                    id,
                    Rect::new(
                        r.x + x,
                        r.bottom() - 3,
                        if id == TRUST_SAVE {
                            25
                        } else if id == CANCEL {
                            18
                        } else {
                            22
                        },
                        1,
                    ),
                    ControlKind::Button(ctx.language.text(label).into()),
                    ctx.target(id),
                ));
            }
            return s;
        }
        for (i, label) in LABELS.iter().enumerate() {
            let y = r.y + 2 + i as u16 * 2;
            s.caption(
                Paragraph::new(ctx.language.text(label)).style(style(0x70)),
                Rect::new(r.x + 2, y, r.width - 4, 1),
            );
            let value = if i == 6 {
                "*".repeat(self.fields[i].chars().count())
            } else {
                self.fields[i].clone()
            };
            let column = if i == 6 {
                self.fields[i][..self.cursors[i]].chars().count()
            } else {
                crate::presentation::text::Line::from(&self.fields[i][..self.cursors[i]]).width()
            };
            let width = r.width - 5;
            let mut c = Control::new(
                i,
                Rect::new(r.x + 2, y + 1, width, 1),
                ControlKind::ScrolledInput {
                    value,
                    scroll: column.saturating_sub(usize::from(width - 1)),
                },
                ctx.target(100 + i),
            );
            c.enter_action = false;
            c.enabled = self.task.is_none();
            s.control(c);
            if self.focus == i {
                s.cursor = Some((r.x + 2 + column.min(usize::from(width - 1)) as u16, y + 1));
            }
        }
        s.caption(
            Paragraph::new(ctx.language.text(&self.notice)).style(style(0x70)),
            Rect::new(r.x + 2, r.y + 16, r.width - 4, 2),
        );
        for (id, label, x, width, y) in [
            (PROTOCOL, ["SFTP", "FTPS", "FTP"][self.protocol], 2, 12, 18),
            (NEXT, "Profiles", 16, 12, 18),
            (SAVE, "Save profile", 28, 22, 18),
            (TERMINAL, "SSH terminal", 52, 20, 18),
            (
                CONNECT,
                if self.task.is_some() {
                    "Connecting…"
                } else {
                    "Connect"
                },
                2,
                19,
                20,
            ),
            (CANCEL, "Cancel", 25, 18, 20),
            (HELP, "Help", 47, 18, 20),
        ] {
            let mut c = Control::new(
                id,
                Rect::new(r.x + x, r.y + y, width, 1),
                ControlKind::Button(ctx.language.text(label).into()),
                ctx.target(id),
            );
            c.enabled = (self.task.is_none() || id == CANCEL || id == HELP)
                && (id != TERMINAL || self.protocol == 0);
            s.control(c);
        }
        s
    }
    fn next_wake(&self, now: Instant) -> Option<Duration> {
        self.task
            .as_ref()
            .map(|_| self.wake.unwrap_or(now).saturating_duration_since(now))
    }
    fn close(&mut self) {
        self.task = None;
        if let Some(secret) = self.fields.get_mut(6) {
            secret.zeroize();
        }
        self.observed = None;
        self.ready = None;
        self.profiles_open = false;
        self.delete_profile = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn context() -> Context {
        Context {
            binding: Binding {
                plugin: REMOTE_CONNECTIONS,
                owner: Owner::Document(crate::desktop::WindowId::MANAGER),
            },
            scope: crate::turbo::dialog_spec::ControlScope::Document {
                window: crate::desktop::WindowId::MANAGER,
                revision: 0,
            },
            area: Rect::new(0, 0, 80, 25),
            language: crate::localization::Language::English,
            palette: crate::palette::PaletteProfile::ClassicDn,
            active: true,
            now: Instant::now(),
            storage: None,
            mp3: Default::default(),
            nearby: Default::default(),
        }
    }
    pub(super) fn frame(p: &RemoteConnections, c: &Context) -> String {
        let mut f = crate::painting::NativeFrame::new(c.area, c.palette);
        p.controls(c).append(&mut f);
        let scene = f.finish();
        format!(
            "{:?}",
            scene
                .cells
                .iter()
                .map(|(pos, g)| (*pos, g.text.clone(), g.attribute))
                .collect::<Vec<_>>()
        )
    }
    #[test]
    fn connection_controls_mask_secrets_fit_and_remain_immutable_in_both_languages() {
        let mut c = context();
        let mut p = RemoteConnections::default();
        p.open(&c);
        p.fields[6] = "тайный-secret".into();
        p.cursors[6] = p.fields[6].len();
        for language in [
            crate::localization::Language::English,
            crate::localization::Language::Russian,
        ] {
            c.language = language;
            for area in [
                Rect::new(0, 0, 80, 25),
                Rect::new(0, 0, 132, 40),
                Rect::new(0, 0, 80, 25),
            ] {
                c.area = area;
                for palette in [
                    crate::palette::PaletteProfile::ClassicDn,
                    crate::palette::PaletteProfile::Terminal,
                ] {
                    c.palette = palette;
                    let s = p.controls(&c);
                    assert!(s.bounds.right() <= area.right() && s.bounds.bottom() <= area.bottom());
                    assert!(s
                        .controls
                        .iter()
                        .all(|x| x.bounds.right() < s.bounds.right()
                            && x.bounds.bottom() < s.bounds.bottom()));
                    assert!(s.controls.iter().any(|x| x.id == CONNECT && x.enabled));
                    let password = s.controls.iter().find(|x| x.id == 6).unwrap();
                    assert!(
                        matches!(&password.kind,ControlKind::ScrolledInput{value,..} if value==&"*".repeat(p.fields[6].chars().count()))
                    );
                    let first = frame(&p, &c);
                    assert!(!first.contains("тайный-secret"));
                    for _ in 0..3 {
                        assert_eq!(first, frame(&p, &c));
                    }
                }
            }
        }
        p.observed = Some("SHA256:fixture".into());
        p.focus = CANCEL;
        let s = p.controls(&c);
        assert_eq!(s.default, c.target(CANCEL));
        assert_eq!(s.cancel, c.target(CANCEL));
        assert!(s.controls.iter().any(|x| x.id == TRUST_SAVE));
        p.close();
        assert!(p.fields[6].is_empty());
    }
    #[test]
    fn connection_poll_deadline_becomes_due_without_keyboard_activity() {
        let c = context();
        let mut p = RemoteConnections::default();
        p.open(&c);
        p.task = Some(
            Task::spawn(&p.scope, "dn-test-wakeup", |_| {
                Err(ConnectError::Failure(std::io::Error::other("fixture")))
            })
            .unwrap(),
        );
        p.wake = Some(c.now + Duration::from_millis(30));
        assert_eq!(p.next_wake(c.now), Some(Duration::from_millis(30)));
        assert_eq!(
            p.next_wake(c.now + Duration::from_millis(30)),
            Some(Duration::ZERO)
        );
    }
    #[test]
    fn saved_pin_survives_cursor_navigation_but_never_follows_another_endpoint() {
        let c = context();
        let mut p = RemoteConnections::default();
        p.open(&c);
        p.fields[1] = "fixture.invalid".into();
        p.pinned = Some("SHA256:fixture".into());
        p.pinned_endpoint = Some(("fixture.invalid".into(), 22));
        p.cursors[1] = p.fields[1].len();
        p.focus = 1;
        p.event(
            Event::Key(crossterm::event::KeyEvent::new(K::Left, M::NONE)),
            &c,
        );
        assert_eq!(
            p.profile().unwrap().fingerprint.as_deref(),
            Some("SHA256:fixture")
        );
        p.fields[1] = "another.invalid".into();
        assert!(p.profile().unwrap().fingerprint.is_none());
        p.fields[1] = "fixture.invalid".into();
        p.fields[2] = "2222".into();
        assert!(p.profile().unwrap().fingerprint.is_none());
    }
}
