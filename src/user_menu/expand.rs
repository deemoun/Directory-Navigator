//! Capture pane context once, before parameter entry; expand only whole argv items.
use super::{
    runner::{CommandSpec, Plan, SelectedList},
    selected,
};
use crate::app::App;
use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf};
pub struct Context {
    cwd: PathBuf,
    other_dir: PathBuf,
    file: Option<OsString>,
    name: Option<OsString>,
    other_file: Option<OsString>,
    selected: Result<Vec<PathBuf>, String>,
    other_selected: Result<Vec<PathBuf>, String>,
}
impl Context {
    pub fn capture(app: &App) -> Self {
        let a = &app.panels[app.active];
        let b = &app.panels[app.active ^ 1];
        Self {
            cwd: a.path.clone(),
            other_dir: b.path.clone(),
            file: a
                .entries
                .get(a.cursor)
                .filter(|e| !e.parent)
                .map(|e| e.path.clone().into_os_string()),
            name: a
                .entries
                .get(a.cursor)
                .filter(|e| !e.parent)
                .map(|e| e.name.clone()),
            other_file: b
                .entries
                .get(b.cursor)
                .filter(|e| !e.parent)
                .map(|e| e.path.clone().into_os_string()),
            selected: selected(a).map_err(|e| e.to_string()),
            other_selected: selected(b).map_err(|e| e.to_string()),
        }
    }
    pub fn plan(
        &self,
        commands: &[Vec<String>],
        parameters: &BTreeMap<String, String>,
    ) -> io::Result<Plan> {
        let mut plan = Plan {
            cwd: self.cwd.clone(),
            commands: Vec::new(),
            list_files: Vec::new(),
            env: vec![
                ("DN_DIR".into(), self.cwd.clone().into_os_string()),
                (
                    "DN_OTHER_DIR".into(),
                    self.other_dir.clone().into_os_string(),
                ),
                ("DN_FILE".into(), self.file.clone().unwrap_or_default()),
                (
                    "DN_OTHER_FILE".into(),
                    self.other_file.clone().unwrap_or_default(),
                ),
            ],
        };
        let mut lists = [None, None];
        let mut total = 0usize;
        for command in commands {
            let mut args = Vec::<OsString>::new();
            for (position, token) in command.iter().enumerate() {
                let value = match token.as_str() {
                    "{selected}"
                    | "{other_selected}"
                    | "{selected_list}"
                    | "{other_selected_list}" => {
                        if position == 0 {
                            return Err(io::Error::other("A selection cannot be an executable"));
                        }
                        let side = usize::from(token.starts_with("{other_"));
                        let paths = if side == 0 {
                            &self.selected
                        } else {
                            &self.other_selected
                        }
                        .as_ref()
                        .map_err(|e| io::Error::other(e.clone()))?;
                        if paths.is_empty() || paths.len() > 2048 {
                            return Err(io::Error::other(
                                "Selection must contain 1–2048 visible entries",
                            ));
                        }
                        if token.ends_with("_list}") {
                            let index = if let Some(i) = lists[side] {
                                i
                            } else {
                                let i = plan.list_files.len();
                                plan.list_files.push(SelectedList::new(paths.clone())?);
                                lists[side] = Some(i);
                                i
                            };
                            plan.list_files[index].path().as_os_str().to_owned()
                        } else {
                            args.extend(paths.iter().map(|p| p.as_os_str().to_owned()));
                            continue;
                        }
                    }
                    "{dir}" => self.cwd.as_os_str().to_owned(),
                    "{other_dir}" => self.other_dir.as_os_str().to_owned(),
                    "{file}" => self
                        .file
                        .clone()
                        .ok_or_else(|| io::Error::other("Highlight a file or folder first"))?,
                    "{name}" => self
                        .name
                        .clone()
                        .ok_or_else(|| io::Error::other("Highlight a file or folder first"))?,
                    "{other_file}" => self
                        .other_file
                        .clone()
                        .ok_or_else(|| io::Error::other("Other panel has no highlighted entry"))?,
                    "{shell}" => std::env::var_os(if cfg!(windows) { "COMSPEC" } else { "SHELL" })
                        .unwrap_or_else(|| {
                            if cfg!(windows) {
                                "cmd.exe".into()
                            } else {
                                "/bin/sh".into()
                            }
                        }),
                    _ => {
                        if let Some(name) = token
                            .strip_prefix("{param:")
                            .and_then(|s| s.strip_suffix('}'))
                        {
                            parameters
                                .get(name)
                                .ok_or_else(|| {
                                    io::Error::other(format!("Missing parameter: {name}"))
                                })?
                                .into()
                        } else {
                            if [
                                "{selected}",
                                "{other_selected}",
                                "{selected_list}",
                                "{other_selected_list}",
                                "{dir}",
                                "{other_dir}",
                                "{file}",
                                "{name}",
                                "{other_file}",
                                "{shell}",
                                "{param:",
                            ]
                            .iter()
                            .any(|p| token.contains(p))
                            {
                                return Err(io::Error::other(
                                    "Placeholders must occupy a whole argument",
                                ));
                            }
                            token.into()
                        }
                    }
                };
                args.push(value);
            }
            if args.is_empty() || args[0].is_empty() || args.len() > 2048 {
                return Err(io::Error::other(
                    "Empty executable or more than 2048 arguments",
                ));
            }
            let bytes = args
                .iter()
                .map(|s| s.as_encoded_bytes().len() + 1)
                .sum::<usize>();
            total = total.saturating_add(bytes);
            if bytes > 64 * 1024 || total > 256 * 1024 {
                return Err(io::Error::other(
                    "Expanded command exceeds 64 KiB, or menu action exceeds 256 KiB",
                ));
            }
            let program = args.remove(0);
            plan.commands.push(CommandSpec { program, args });
        }
        if plan.commands.is_empty() {
            return Err(io::Error::other("Menu item has no commands"));
        }
        Ok(plan)
    }
}
