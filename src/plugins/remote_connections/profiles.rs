//! Owned saved-profile pages. Targets are retired whenever a page changes.
use super::*;
use crossterm::event::{KeyEvent, MouseButton, MouseEventKind};
use std::sync::atomic::{AtomicUsize, Ordering};
const ROWS: usize = 9;
const ROW: usize = 32;
const FIELD: usize = 0;
const USE: usize = 1;
const EDIT: usize = 2;
const DELETE: usize = 3;
const NEW: usize = 4;
const CLOSE: usize = 5;
const CONFIRM: usize = 6;
const BACK: usize = 7;
const SCROLL: usize = 16;
impl RemoteConnections {
    fn advance_profile_page(&mut self) {
        static NEXT_PAGE: AtomicUsize = AtomicUsize::new(10_000);
        self.profile_page = NEXT_PAGE.fetch_add(256, Ordering::Relaxed);
        self.last_profile_click = None;
    }
    fn page_target(&self, ctx: &Context, id: usize) -> Target {
        ctx.target(self.profile_page + id)
    }
    fn fit_profiles(&mut self) {
        crate::list_state::ListViewport::fit(
            &mut self.profile_index,
            &mut self.profile_offset,
            ROWS,
            Some(self.profiles.len().saturating_sub(1)),
            Some(self.profiles.len().saturating_sub(ROWS)),
        );
    }
    pub(super) fn show_profiles(&mut self) {
        if self.task.is_some() || self.observed.is_some() {
            return;
        }
        let loaded = self
            .storage
            .as_ref()
            .ok_or_else(|| std::io::Error::other("Configuration path unavailable"))
            .and_then(|p| crate::network::profiles::load(p));
        match loaded {
            Ok(profiles) => {
                self.profiles = profiles;
                if let Some(index) = self
                    .editing
                    .as_ref()
                    .and_then(|p| self.profiles.iter().position(|v| v.name == p.name))
                {
                    self.profile_index = index;
                }
                self.fit_profiles();
                self.profiles_open = true;
                self.delete_profile = None;
                self.focus = FIELD;
                self.notice.clear();
                self.advance_profile_page();
            }
            Err(error) => self.notice = error.to_string(),
        }
    }
    pub(super) fn save_profile(
        &mut self,
        language: crate::localization::Language,
    ) -> std::io::Result<Option<String>> {
        let profile = self.profile()?;
        let path = self
            .storage
            .as_ref()
            .ok_or_else(|| std::io::Error::other("Configuration path unavailable"))?;
        let warning = match &self.editing {
            Some(original) => crate::network::profiles::replace(path, original, &profile)?,
            None => crate::network::profiles::create(path, &profile)?,
        };
        self.editing = Some(profile.clone());
        match crate::network::profiles::load(path) {
            Ok(profiles) => {
                self.profiles = profiles;
                self.profile_index = self
                    .profiles
                    .iter()
                    .position(|p| p.name == profile.name)
                    .unwrap_or(0);
                self.fit_profiles();
                Ok(warning)
            }
            Err(error) => Ok(Some(format!(
                "{}{}: {error}",
                warning.map_or(String::new(), |w| format!("{w}\n")),
                language.text("Profile saved; cannot refresh the saved-profile list")
            ))),
        }
    }
    fn profiles_action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        if self.delete_profile.is_some() {
            match id {
                BACK => {
                    self.delete_profile = None;
                    self.focus = DELETE;
                    self.advance_profile_page();
                }
                CONFIRM => {
                    let original = self.delete_profile.as_ref().unwrap().clone();
                    let result = self
                        .storage
                        .as_ref()
                        .ok_or_else(|| std::io::Error::other("Configuration path unavailable"))
                        .and_then(|p| crate::network::profiles::delete(p, &original));
                    match result {
                        Ok(warning) => {
                            self.profiles.retain(|p| p.name != original.name);
                            let warning = match self
                                .storage
                                .as_ref()
                                .map(|p| crate::network::profiles::load(p))
                            {
                                Some(Ok(profiles)) => {
                                    self.profiles = profiles;
                                    warning
                                }
                                Some(Err(error)) => Some(format!(
                                    "{}{}: {error}",
                                    warning.map_or(String::new(), |w| format!("{w}\n")),
                                    ctx.language.text(
                                        "Profile deleted; cannot refresh the saved-profile list"
                                    )
                                )),
                                None => warning,
                            };
                            if self
                                .editing
                                .as_ref()
                                .is_some_and(|p| p.name == original.name)
                            {
                                self.editing = None;
                            }
                            self.fit_profiles();
                            self.delete_profile = None;
                            self.focus = FIELD;
                            self.notice = warning.unwrap_or_else(|| {
                                "Saved profile deleted. Open connections are unchanged.".into()
                            });
                            self.advance_profile_page();
                        }
                        Err(error) => self.notice = error.to_string(),
                    }
                }
                _ => {}
            }
            return vec![];
        }
        match id {
            CLOSE => {
                self.profiles_open = false;
                self.focus = NEXT;
                self.advance_profile_page();
            }
            USE | EDIT => {
                if let Some(profile) = self.profiles.get(self.profile_index).cloned() {
                    self.apply(profile.clone());
                    self.editing = Some(profile);
                    self.profiles_open = false;
                    self.focus = if id == EDIT { 0 } else { 6 };
                    self.notice.clear();
                    self.advance_profile_page();
                }
            }
            NEW => {
                self.apply(Self::new_profile());
                self.editing = None;
                self.profiles_open = false;
                self.focus = 0;
                self.notice.clear();
                self.advance_profile_page();
            }
            DELETE => {
                if let Some(profile) = self.profiles.get(self.profile_index).cloned() {
                    self.delete_profile = Some(profile);
                    self.focus = 1;
                    self.notice.clear();
                    self.advance_profile_page();
                }
            }
            FIELD => self.focus = FIELD,
            SCROLL..=19 => {
                let code = [K::Up, K::Down, K::PageUp, K::PageDown][id - SCROLL];
                self.profile_key(KeyEvent::new(code, M::NONE));
            }
            ROW.. if id < ROW + self.profiles.len() => {
                self.profile_index = id - ROW;
                self.focus = FIELD;
                self.fit_profiles();
            }
            _ => {}
        }
        vec![]
    }
    fn profile_key(&mut self, key: KeyEvent) {
        if self.delete_profile.is_some() {
            return;
        }
        if self.focus == FIELD {
            if let Some(cursor) = crate::turbo::widgets::list_key(
                self.profile_index,
                self.profiles.len(),
                ROWS,
                key.code,
            ) {
                self.profile_index = cursor;
                self.fit_profiles();
                self.last_profile_click = None;
            }
        }
    }
    pub(super) fn profiles_event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        match event {
            Event::Key(key) if key.code == K::F(1) => {
                return vec![Effect::Help {
                    title: " Remote connection help ",
                    text: crate::network::HELP.into(),
                }]
            }
            Event::Action(ActionId(id)) => {
                if let Some(local) = id.checked_sub(self.profile_page).filter(|id| *id < 256) {
                    return self.profiles_action(local, ctx);
                }
            }
            Event::Focus(focus) => self.focus = focus,
            Event::Key(key) if self.delete_profile.is_none() && key.code == K::Delete => {
                return self.profiles_action(DELETE, ctx)
            }
            Event::Key(key) if self.delete_profile.is_none() && key.code == K::F(2) => {
                return self.profiles_action(EDIT, ctx)
            }
            Event::Key(key) => self.profile_key(key),
            Event::Mouse(mouse, target) => {
                if self.delete_profile.is_none()
                    && matches!(
                        mouse.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    )
                {
                    self.profile_index = self
                        .profile_index
                        .saturating_add_signed(crate::list_state::wheel_delta(mouse.kind));
                    self.fit_profiles();
                    self.focus = FIELD;
                    self.last_profile_click = None;
                } else if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                    if let Some(ActionId(id)) = target {
                        if let Some(local) =
                            id.checked_sub(self.profile_page).filter(|id| *id < 256)
                        {
                            let row = local
                                .checked_sub(ROW)
                                .filter(|row| *row < self.profiles.len());
                            let double = row.is_some_and(|row| {
                                crate::list_state::ClickTracker::STRICT.matches(
                                    self.last_profile_click.as_ref(),
                                    &row,
                                    ctx.now,
                                )
                            });
                            let effects = self.profiles_action(local, ctx);
                            if let Some(row) = row {
                                self.last_profile_click = Some((row, ctx.now));
                                if double {
                                    return self.profiles_action(USE, ctx);
                                }
                            }
                            return effects;
                        }
                    }
                }
            }
            _ => {}
        }
        vec![]
    }
    pub(super) fn profiles_controls(&self, ctx: &Context) -> DialogSpec<'_> {
        let deleting = self.delete_profile.is_some();
        let r = centered(ctx.area, 76, if deleting { 12 } else { 19 });
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let mut spec = ctx.dialog(
            r,
            self.focus,
            ctx.language.text(if deleting {
                "Delete saved profile"
            } else {
                "Saved connection profiles"
            }),
            self.profile_page
                + if deleting {
                    BACK
                } else if self.profiles.is_empty() {
                    CLOSE
                } else {
                    USE
                },
            self.profile_page + if deleting { BACK } else { CLOSE },
        );
        if let Some(profile) = &self.delete_profile {
            spec.caption(
                Paragraph::new(format!(
                    "{}\n{}\n{}",
                    ctx.language.text("Delete this saved connection profile?"),
                    crate::formatting::tail(&profile.name, r.width - 6),
                    crate::formatting::tail(&profile.label(), r.width - 6)
                ))
                .style(style(0x70)),
                Rect::new(r.x + 3, r.y + 2, r.width - 6, 4),
            );
            spec.caption(
                Paragraph::new(
                    ctx.language
                        .text("Open connections and files will not be deleted."),
                )
                .style(style(0x70)),
                Rect::new(r.x + 3, r.y + 6, r.width - 6, 1),
            );
            spec.caption(
                Paragraph::new(ctx.language.text(&self.notice)).style(style(0x70)),
                Rect::new(r.x + 3, r.y + 7, r.width - 6, 2),
            );
            for (id, local, label, x) in [(0, CONFIRM, "Delete", 10), (1, BACK, "Cancel", 42)] {
                spec.control(Control::new(
                    id,
                    Rect::new(r.x + x, r.bottom() - 3, 22, 1),
                    ControlKind::Button(ctx.language.text(label).into()),
                    self.page_target(ctx, local),
                ));
            }
            return spec;
        }
        let bounds = Rect::new(r.x + 3, r.y + 3, r.width - 7, ROWS as u16);
        spec.caption(
            Paragraph::new(
                ctx.language
                    .text("Choose a profile; Enter selects, F2 edits, Del deletes."),
            )
            .style(style(0x70)),
            Rect::new(r.x + 3, r.y + 1, r.width - 6, 1),
        );
        let items = self
            .profiles
            .iter()
            .map(|p| {
                format!(
                    "{}  ·  {}",
                    crate::filesystem::display_name(&p.name),
                    p.label()
                )
            })
            .collect();
        let mut list = Control::new(
            FIELD,
            bounds,
            ControlKind::List {
                items,
                cursor: self.profile_index,
                offset: self.profile_offset,
            },
            self.page_target(ctx, FIELD),
        );
        list.dn_list = true;
        list.hits = (self.profile_offset..self.profiles.len().min(self.profile_offset + ROWS))
            .map(|row| {
                (
                    Rect::new(
                        bounds.x,
                        bounds.y + (row - self.profile_offset) as u16,
                        bounds.width,
                        1,
                    ),
                    self.page_target(ctx, ROW + row),
                )
            })
            .collect();
        if self.profiles.is_empty() {
            list.hits.push((bounds, self.page_target(ctx, FIELD)));
        }
        spec.control(list);
        if self.profiles.is_empty() {
            spec.caption(
                Paragraph::new(ctx.language.text("No saved connection profiles"))
                    .style(style(0x78)),
                Rect::new(bounds.x, bounds.y, bounds.width, 1),
            );
        }
        spec.native(
            crate::turbo::widgets::scrollbar(
                self.profile_index,
                self.profiles.len().saturating_sub(1),
                ROWS,
            ),
            Rect::new(bounds.right(), bounds.y, 1, ROWS as u16),
        );
        for (dy, h, command) in [
            (0, 1, SCROLL),
            (8, 1, SCROLL + 1),
            (1, 3, SCROLL + 2),
            (4, 4, SCROLL + 3),
        ] {
            spec.hit(
                Rect::new(bounds.right(), bounds.y + dy, 1, h),
                self.page_target(ctx, command),
            );
        }
        spec.caption(
            Paragraph::new(ctx.language.text(&self.notice)).style(style(0x70)),
            Rect::new(r.x + 3, r.y + 13, r.width - 6, 2),
        );
        for (id, label, x, width) in [
            (USE, "Select", 2, 12),
            (EDIT, "Edit", 16, 12),
            (DELETE, "Delete", 30, 12),
            (NEW, "New", 44, 12),
            (CLOSE, "Close", 58, 14),
        ] {
            let mut control = Control::new(
                id,
                Rect::new(r.x + x, r.bottom() - 3, width, 1),
                ControlKind::Button(ctx.language.text(label).into()),
                self.page_target(ctx, id),
            );
            control.enabled = !self.profiles.is_empty() || matches!(id, NEW | CLOSE);
            spec.control(control);
        }
        spec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::remote_connections::tests::{context, frame};
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> std::io::Result<(Temp, RemoteConnections, Context)> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-profile-ui-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        let mut ctx = context();
        ctx.storage = Some(path.join("config.conf"));
        for i in 0..30 {
            let mut p = RemoteConnections::new_profile();
            p.name = format!("Profile {i:02} Ёж 世界");
            p.host = format!("host{i:02}.invalid");
            crate::network::profiles::create(&path.join("remote-connections.conf"), &p)?;
        }
        let mut p = RemoteConnections::default();
        p.open(&ctx);
        Ok((Temp(path), p, ctx))
    }
    fn command(p: &mut RemoteConnections, id: usize, c: &Context) {
        let target = p.profile_page + id;
        p.event(Event::Action(ActionId(target)), c);
    }
    #[test]
    fn selection_edit_rename_and_cancel_preserve_drafts_and_never_connect() -> std::io::Result<()> {
        let (_t, mut p, c) = fixture()?;
        p.fields[6] = "draft-secret".into();
        p.cursors[6] = p.fields[6].len();
        let before = p.fields.clone();
        p.action(NEXT, &c);
        assert!(p.profiles_open);
        p.event(Event::Key(KeyEvent::new(K::End, M::NONE)), &c);
        assert_eq!(p.profile_index, 29);
        assert_eq!(p.profile_offset, 21);
        command(&mut p, CLOSE, &c);
        assert_eq!(p.fields, before);
        assert_eq!(p.focus, NEXT);
        p.action(NEXT, &c);
        command(&mut p, ROW + 14, &c);
        command(&mut p, EDIT, &c);
        assert!(!p.profiles_open);
        assert_eq!(p.fields[0], "Profile 14 Ёж 世界");
        assert!(p.fields[6].is_empty());
        assert_eq!(p.focus, 0);
        assert!(p.task.is_none());
        p.fields[0] = "Renamed profile".into();
        p.action(SAVE, &c);
        let saved = crate::network::profiles::load(p.storage.as_ref().unwrap())?;
        assert_eq!(saved.len(), 30);
        assert!(saved.iter().any(|v| v.name == "Renamed profile"));
        assert!(!saved.iter().any(|v| v.name == "Profile 14 Ёж 世界"));
        p.action(NEXT, &c);
        command(&mut p, USE, &c);
        assert_eq!(p.focus, 6);
        assert!(p.task.is_none());
        Ok(())
    }
    #[test]
    fn deletion_defaults_to_cancel_retires_stale_answers_and_clamps_selection(
    ) -> std::io::Result<()> {
        let (_t, mut p, c) = fixture()?;
        p.action(NEXT, &c);
        command(&mut p, ROW + 29, &c);
        command(&mut p, DELETE, &c);
        assert_eq!(p.focus, 1);
        let first = p.profile_page + CONFIRM;
        assert_eq!(p.controls(&c).default, c.target(p.profile_page + BACK));
        command(&mut p, BACK, &c);
        assert_eq!(
            crate::network::profiles::load(p.storage.as_ref().unwrap())?.len(),
            30
        );
        command(&mut p, DELETE, &c);
        p.event(Event::Action(ActionId(first)), &c);
        assert_eq!(p.profiles.len(), 30);
        command(&mut p, CONFIRM, &c);
        assert_eq!(p.profiles.len(), 29);
        assert_eq!(p.profile_index, 28);
        assert!(p.delete_profile.is_none());
        let original = p.profiles[28].clone();
        command(&mut p, DELETE, &c);
        let mut changed = original.clone();
        changed.host = "another.invalid".into();
        crate::network::profiles::replace(p.storage.as_ref().unwrap(), &original, &changed)?;
        command(&mut p, CONFIRM, &c);
        assert!(p.delete_profile.is_some());
        assert!(p.notice.contains("changed"));
        assert_eq!(
            crate::network::profiles::load(p.storage.as_ref().unwrap())?.len(),
            29
        );
        Ok(())
    }
    #[test]
    fn profile_pages_fit_localize_mask_secrets_and_draw_immutably_with_mouse_and_keyboard(
    ) -> std::io::Result<()> {
        let (_t, mut p, mut c) = fixture()?;
        p.fields[6] = "do-not-show".into();
        p.action(NEXT, &c);
        for language in [
            crate::localization::Language::English,
            crate::localization::Language::Russian,
        ] {
            c.language = language;
            for palette in [
                crate::palette::PaletteProfile::ClassicDn,
                crate::palette::PaletteProfile::Terminal,
            ] {
                c.palette = palette;
                for area in [
                    Rect::new(0, 0, 80, 25),
                    Rect::new(0, 0, 132, 40),
                    Rect::new(0, 0, 80, 25),
                ] {
                    c.area = area;
                    let first = frame(&p, &c);
                    assert!(!first.contains("do-not-show"));
                    for _ in 0..3 {
                        assert_eq!(first, frame(&p, &c));
                    }
                    let spec = p.controls(&c);
                    assert!(
                        spec.bounds.right() <= area.right()
                            && spec.bounds.bottom() <= area.bottom()
                    );
                    assert!(spec
                        .controls
                        .iter()
                        .all(|v| v.bounds.right() < spec.bounds.right()
                            && v.bounds.bottom() < spec.bounds.bottom()));
                }
            }
        }
        let mouse = crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 7,
            modifiers: M::NONE,
        };
        let row = p.profile_page + ROW + 3;
        p.event(Event::Mouse(mouse, Some(ActionId(row))), &c);
        assert_eq!(p.profile_index, 3);
        command(&mut p, DELETE, &c);
        let first = frame(&p, &c);
        for _ in 0..3 {
            assert_eq!(first, frame(&p, &c));
        }
        command(&mut p, BACK, &c);
        while !p.profiles.is_empty() {
            command(&mut p, DELETE, &c);
            command(&mut p, CONFIRM, &c);
        }
        let spec = p.controls(&c);
        assert!(!spec.controls.iter().find(|v| v.id == EDIT).unwrap().enabled);
        assert!(
            !spec
                .controls
                .iter()
                .find(|v| v.id == DELETE)
                .unwrap()
                .enabled
        );
        command(&mut p, NEW, &c);
        assert!(p.editing.is_none());
        assert_eq!(p.fields[0], "New connection");
        assert!(p.fields[6].is_empty());
        Ok(())
    }
}
