//! Android package workbench. The host owns documents and panel navigation;
//! this plugin owns bounded tasks, SDK processes and draft decision pages.
use super::*;
use crate::{
    apk::{
        self,
        project::Project,
        tools::{Device, DeviceAction, Logcat, Tools},
        Package,
    },
    painting::{NativeFrame, Paragraph},
    task::Task,
    turbo::{
        dialog_spec::{centered, Control, ControlKind},
        file_dialog::{Mode, Outcome, Picker},
    },
    windowing::geometry::Capabilities,
};
use crossterm::event::{KeyCode as K, KeyModifiers as M, MouseButton, MouseEventKind};
use std::{collections::BTreeSet, io, sync::Arc};
use zeroize::{Zeroize, Zeroizing};
const HELP:&str="APK Workshop: inspect APKs without apktool.\nOverview / Files / Manifest / DEX / Changes / Device.\nEnter opens folders or previews a file/method. F3 previews; F4 edits working files; F5 extracts.\nOpen accepts .apk or apk-project.toml. Project creates a persistent copy in a new folder; Files dir opens its editable files in DN. Add/delete files there, then Refresh. Original APKs are never modified.\nManifest edits: debuggable, version code/name and ordinary permissions. DEX, native libraries and compiled resources are read-only. Java decompilation, split APKs and Smali rebuilding are not supported.\nExport creates a new unsigned APK. Sign requires SDK zipalign, apksigner.jar and Java, plus your keystore or an explicitly created test key. Test key alias: androiddebugkey; password: android. Passwords are never saved.\nDevice requires ADB. Select a connected device, install a signed export, launch, stop or view logcat. Filter applies to displayed log lines. This is log/crash debugging, not a breakpoint debugger.\nEsc cancels the current task/page; closing stops owned logcat processes. F1: Help.";
const PAGE: u64 = 1;
const PICKER: u64 = 2;
const LIST: usize = 200;
const FILTER: usize = 201;
const ROW: usize = 1000;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Overview,
    Files,
    Manifest,
    Dex,
    Changes,
    Device,
}
const SECTIONS: [Section; 6] = [
    Section::Overview,
    Section::Files,
    Section::Manifest,
    Section::Dex,
    Section::Changes,
    Section::Device,
];
const TITLES: [&str; 6] = ["Overview", "Files", "Manifest", "DEX", "Changes", "Device"];
#[derive(Clone, Copy, PartialEq, Eq)]
enum FormKind {
    Manifest,
    Extract,
    Sign,
    Tools,
    TestKey,
    Restore,
}
struct Form {
    kind: FormKind,
    labels: Vec<&'static str>,
    fields: Vec<String>,
    cursors: Vec<usize>,
    focus: usize,
    secret: Option<usize>,
    error: String,
}
impl Drop for Form {
    fn drop(&mut self) {
        if let Some(i) = self.secret {
            for field in self.fields.iter_mut().skip(i) {
                field.zeroize();
            }
        }
    }
}
impl Form {
    fn new(
        kind: FormKind,
        labels: Vec<&'static str>,
        fields: Vec<String>,
        secret: Option<usize>,
    ) -> Self {
        Self {
            cursors: fields.iter().map(String::len).collect(),
            kind,
            labels,
            fields,
            focus: 0,
            secret,
            error: String::new(),
        }
    }
}
enum Reply {
    Loaded(
        Arc<Package>,
        Option<Project>,
        Option<apk::manifest::Manifest>,
    ),
    Project(Project, apk::manifest::Manifest),
    Rows(Vec<String>),
    Preview(String, Vec<u8>, bool),
    Devices(Vec<Device>),
    Notice(String),
    Export(PathBuf, bool),
}
#[derive(Clone)]
enum Item {
    None,
    Member(usize),
    Method(usize, usize),
    Device(usize),
    Change(String),
    Text(String),
}
pub(crate) struct ApkWorkshop {
    package: Option<Arc<Package>>,
    project: Option<Project>,
    effective_manifest: Option<apk::manifest::Manifest>,
    task: Option<Task<io::Result<Reply>>>,
    requests: crate::requests::RequestScope,
    section: Section,
    focus: usize,
    cursor: usize,
    offset: usize,
    filter: String,
    filter_cursor: usize,
    prefix: usize,
    rows: Vec<(String, Item)>,
    detail: Vec<String>,
    notice: String,
    base: PathBuf,
    destination: Option<PathBuf>,
    form: Option<Form>,
    picker: Option<Picker>,
    picker_job: usize,
    tools: Tools,
    devices: Vec<Device>,
    serial: Option<String>,
    log: Option<Logcat>,
    exported: Option<PathBuf>,
    signing_destination: Option<PathBuf>,
    signed: bool,
    key: Option<PathBuf>,
    wake: Instant,
}
impl Default for ApkWorkshop {
    fn default() -> Self {
        Self {
            package: None,
            project: None,
            effective_manifest: None,
            task: None,
            requests: Default::default(),
            section: Section::Overview,
            focus: LIST,
            cursor: 0,
            offset: 0,
            filter: String::new(),
            filter_cursor: 0,
            prefix: 0,
            rows: vec![],
            detail: vec![],
            notice: String::new(),
            base: PathBuf::new(),
            destination: None,
            form: None,
            picker: None,
            picker_job: 10,
            tools: Tools::default(),
            devices: vec![],
            serial: None,
            log: None,
            exported: None,
            signing_destination: None,
            signed: false,
            key: None,
            wake: Instant::now(),
        }
    }
}
impl ApkWorkshop {
    fn start(
        &mut self,
        run: impl FnOnce(crate::task::Cancellation) -> io::Result<Reply> + Send + 'static,
    ) {
        if self.task.is_some() {
            return;
        }
        match self.requests.spawn("apk-workshop", run) {
            Ok(t) => {
                self.task = Some(t);
                self.notice = "Working... Esc cancels".into();
            }
            Err(e) => self.notice = e.to_string(),
        }
    }
    fn load(&mut self, path: PathBuf) {
        if self.task.is_some() {
            self.notice = "An APK task is already running".into();
            return;
        }
        self.log = None;
        self.start(move |cancel| {
            if path.file_name().is_some_and(|s| s == "apk-project.toml") {
                let p = Project::open(path.parent().unwrap().to_owned(), &cancel)?;
                let package = Package::open(p.catalog.path.clone(), &cancel)?;
                let manifest = p.manifest(&cancel)?;
                Ok(Reply::Loaded(Arc::new(package), Some(p), Some(manifest)))
            } else {
                Ok(Reply::Loaded(
                    Arc::new(Package::open(path, &cancel)?),
                    None,
                    None,
                ))
            }
        });
    }
    fn pick(&mut self, job: usize, ctx: &Context) {
        if self.task.is_some() {
            return;
        }
        self.picker_job = job;
        self.picker = Some(
            Picker::new(
                if matches!(job, 11 | 14 | 21 | 35) {
                    Mode::SaveAs
                } else {
                    Mode::Open
                },
                self.base.clone(),
                ctx.language,
                true,
            )
            .with_target(ctx.target(900)),
        );
    }
    fn refresh(&mut self) {
        self.detail.clear();
        if self.section == Section::Changes {
            if let Some(p) = self.project.clone() {
                self.start(move |cancel| Ok(Reply::Rows(p.changes(&cancel)?)));
            }
        }
    }
    fn form(&mut self, kind: FormKind) {
        let form = match kind {
            FormKind::Restore => return,
            FormKind::Manifest => {
                let (Some(p), Some(a)) = (&self.project, &self.package) else {
                    self.notice = "Create a project first".into();
                    return;
                };
                let m = &a.manifest;
                Form::new(
                    kind,
                    vec![
                        "Debuggable (true/false)",
                        "Version code",
                        "Version name",
                        "Permissions (; separated)",
                    ],
                    vec![
                        p.changes.debuggable.unwrap_or(m.debuggable).to_string(),
                        p.changes
                            .version_code
                            .map(|v| v.to_string())
                            .unwrap_or(m.version_code.clone()),
                        p.changes
                            .version_name
                            .clone()
                            .unwrap_or(m.version_name.clone()),
                        p.changes
                            .permissions
                            .as_ref()
                            .unwrap_or(&m.permissions)
                            .join(";"),
                    ],
                    None,
                )
            }
            FormKind::Extract => Form::new(
                kind,
                vec!["Filesystem destination"],
                vec![self
                    .destination
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default()],
                None,
            ),
            FormKind::Sign => Form::new(
                kind,
                vec![
                    "Keystore",
                    "Key alias",
                    "Password",
                    "Key password (blank: same password)",
                ],
                vec![
                    self.key
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    if self.key.is_some() {
                        "androiddebugkey".into()
                    } else {
                        String::new()
                    },
                    String::new(),
                    String::new(),
                ],
                Some(2),
            ),
            FormKind::Tools => Form::new(
                kind,
                vec![
                    "ADB executable",
                    "zipalign executable",
                    "apksigner.jar",
                    "Java executable",
                    "keytool executable",
                ],
                [
                    &self.tools.adb,
                    &self.tools.zipalign,
                    &self.tools.signer,
                    &self.tools.java,
                    &self.tools.keytool,
                ]
                .into_iter()
                .map(|p| {
                    p.as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect(),
                None,
            ),
            FormKind::TestKey => Form::new(
                kind,
                vec!["New test keystore path"],
                vec![self
                    .base
                    .join("dn-apk-test.p12")
                    .to_string_lossy()
                    .into_owned()],
                None,
            ),
        };
        self.form = Some(form);
    }
    fn submit(&mut self) -> Vec<Effect> {
        let Some(mut f) = self.form.take() else {
            return vec![];
        };
        let result = (|| -> io::Result<Vec<Effect>> {
            match f.kind {
                FormKind::Restore => {
                    let p = self
                        .project
                        .clone()
                        .ok_or_else(|| io::Error::other("Create a project first"))?;
                    let name = f.fields[0].clone();
                    self.start(move |c| {
                        if name == "AndroidManifest.xml" {
                            let mut p = p;
                            p.save_changes(Default::default(), &c)?;
                            let m = p.manifest(&c)?;
                            Ok(Reply::Project(p, m))
                        } else {
                            p.restore(&name, &c)?;
                            Ok(Reply::Rows(p.changes(&c)?))
                        }
                    });
                }
                FormKind::Tools => {
                    let previous = [
                        &self.tools.adb,
                        &self.tools.zipalign,
                        &self.tools.signer,
                        &self.tools.java,
                        &self.tools.keytool,
                    ];
                    let mut paths = f
                        .fields
                        .iter()
                        .zip(previous)
                        .map(|(s, p)| {
                            if s.trim().is_empty() {
                                None
                            } else if p
                                .as_ref()
                                .is_some_and(|p| p.to_string_lossy() == s.as_str())
                            {
                                p.clone()
                            } else {
                                Some(PathBuf::from(s))
                            }
                        })
                        .collect::<Vec<_>>()
                        .into_iter();
                    self.tools = Tools {
                        adb: paths.next().flatten(),
                        zipalign: paths.next().flatten(),
                        signer: paths.next().flatten(),
                        java: paths.next().flatten(),
                        keytool: paths.next().flatten(),
                    };
                    self.notice = "Android tool paths applied for this session".into();
                }
                FormKind::Manifest => {
                    let mut p = self
                        .project
                        .clone()
                        .ok_or_else(|| io::Error::other("Create a project first"))?;
                    let m = &self.package.as_ref().unwrap().manifest;
                    let flag = match f.fields[0].as_str() {
                        "true" => true,
                        "false" => false,
                        _ => return Err(io::Error::other("Debuggable must be true or false")),
                    };
                    let code = if f.fields[1] == m.version_code {
                        None
                    } else {
                        Some(f.fields[1].parse::<u32>().map_err(|_| {
                            io::Error::other("Version code must be an unsigned integer")
                        })?)
                    };
                    let permissions = f.fields[3]
                        .split(';')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>();
                    let old = m.permissions.iter().cloned().collect::<BTreeSet<_>>();
                    let changes = apk::manifest::Changes {
                        debuggable: (flag != m.debuggable).then_some(flag),
                        version_code: code,
                        version_name: (f.fields[2] != m.version_name).then(|| f.fields[2].clone()),
                        permissions: (permissions.iter().cloned().collect::<BTreeSet<_>>() != old)
                            .then_some(permissions),
                    };
                    self.start(move |c| {
                        p.save_changes(changes, &c)?;
                        {
                            let manifest = p.manifest(&c)?;
                            Ok(Reply::Project(p, manifest))
                        }
                    });
                }
                FormKind::Extract => {
                    let package = self
                        .package
                        .as_ref()
                        .ok_or_else(|| io::Error::other("Open an APK first"))?;
                    let Item::Member(id) = self.selected() else {
                        return Err(io::Error::other("Select an APK member"));
                    };
                    let destination = PathBuf::from(&f.fields[0]);
                    if !destination.is_absolute() {
                        return Err(io::Error::other(
                            "Choose an absolute filesystem destination",
                        ));
                    }
                    let selection = crate::archive::Extraction {
                        catalog: package.catalog.clone(),
                        directory: self.prefix,
                        members: vec![crate::panel::archive::MemberId {
                            generation: package.catalog.generation,
                            index: id,
                        }],
                        password: None,
                    };
                    selection.validate()?;
                    return Ok(vec![Effect::ApkExtract {
                        selection,
                        destination,
                    }]);
                }
                FormKind::Sign => {
                    let p = self
                        .project
                        .clone()
                        .ok_or_else(|| io::Error::other("Create a project first"))?;
                    let destination = self
                        .signing_destination
                        .take()
                        .ok_or_else(|| io::Error::other("Choose a signed output path"))?;
                    let tools = self.tools.clone();
                    let key = PathBuf::from(&f.fields[0]);
                    let alias = f.fields[1].clone();
                    let password = Zeroizing::new(std::mem::take(&mut f.fields[2]));
                    let key_password = Zeroizing::new(std::mem::take(&mut f.fields[3]));
                    self.start(move |c| {
                        tools.sign_with_key(
                            &p,
                            &destination,
                            &apk::tools::SigningKey {
                                path: &key,
                                alias: &alias,
                                password: &password,
                                key_password: (!key_password.is_empty())
                                    .then_some(key_password.as_str()),
                            },
                            &c,
                        )?;
                        Ok(Reply::Export(destination, true))
                    });
                }
                FormKind::TestKey => {
                    let tools = self.tools.clone();
                    let key = PathBuf::from(&f.fields[0]);
                    self.key = Some(key.clone());
                    self.start(move |c| {
                        tools.create_test_key(&key, &c)?;
                        Ok(Reply::Notice(
                            "Test key created. Alias: androiddebugkey; password: android".into(),
                        ))
                    });
                }
            }
            Ok(vec![])
        })();
        match result {
            Ok(e) => e,
            Err(e) => {
                f.error = e.to_string();
                self.form = Some(f);
                vec![]
            }
        }
    }
    fn selected(&self) -> Item {
        self.rows
            .get(self.cursor)
            .map(|(_, i)| i.clone())
            .unwrap_or(Item::None)
    }
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        if id == 0 {
            if self.task.take().is_some() {
                self.notice = "Operation cancelled".into();
                return vec![];
            }
            if self.form.take().is_some() || self.picker.take().is_some() {
                return vec![];
            }
            return vec![Effect::Close];
        }
        if id == 1 {
            return vec![Effect::Help {
                title: "APK Workshop Help",
                text: ctx.language.text(HELP).into(),
            }];
        }
        if self.task.is_some() {
            return vec![];
        }
        if self.form.is_some() {
            if id == 800 {
                return self.submit();
            }
            return vec![];
        }
        if (100..106).contains(&id) {
            self.section = SECTIONS[id - 100];
            self.cursor = 0;
            self.offset = 0;
            self.detail.clear();
            self.filter.clear();
            self.filter_cursor = 0;
            self.focus = LIST;
            self.refresh();
            self.rebuild();
            return vec![];
        }
        if id >= ROW {
            self.cursor = (id - ROW).min(self.rows.len().saturating_sub(1));
            self.focus = LIST;
            return vec![];
        }
        match id {
            LIST => self.focus = LIST,
            FILTER => self.focus = FILTER,
            10 => self.pick(10, ctx),
            11 => {
                if let Some(p) = &self.project {
                    return vec![Effect::ApkDirectory(p.root.join("files"))];
                }
                self.pick(11, ctx);
            }
            13 => {
                self.refresh();
                self.rebuild();
            }
            14 => self.pick(14, ctx),
            21 => {
                if self.tools.signing_available() {
                    self.pick(21, ctx);
                } else {
                    self.notice = "Signing requires zipalign, apksigner.jar and Java".into();
                }
            }
            22 => self.form(FormKind::Tools),
            23 => self.form(FormKind::TestKey),
            2 => {
                if let Item::Text(text) = self.selected() {
                    return vec![Effect::ApkPreview {
                        unicode: true,
                        label: "APK decoded text".into(),
                        data: text.into_bytes(),
                    }];
                }
                if self.section == Section::Manifest || self.section == Section::Overview {
                    if let Some(a) = self.package.clone() {
                        let changes = self.project.as_ref().map(|p| p.changes.clone());
                        self.start(move |c| {
                            let xml = if let Some(changes) =
                                changes.filter(|v| *v != apk::manifest::Changes::default())
                            {
                                let data = a.manifest.patch(&changes, &c)?;
                                apk::manifest::Manifest::parse(&data, &c)?.xml
                            } else {
                                a.manifest.xml.clone()
                            };
                            Ok(Reply::Preview(
                                "AndroidManifest.xml (decoded)".into(),
                                xml.into_bytes(),
                                true,
                            ))
                        });
                    }
                } else {
                    match self.selected() {
                        Item::Member(id) => {
                            if let Some(a) = self.package.clone() {
                                let n = &a.catalog.nodes[id];
                                if n.kind == crate::archive::reader::Kind::Directory {
                                    self.prefix = id;
                                    self.cursor = 0;
                                    self.offset = 0;
                                    self.rebuild();
                                } else {
                                    self.start(move |c| {
                                        Ok(Reply::Preview(
                                            String::from_utf8_lossy(&a.catalog.nodes[id].path)
                                                .into_owned(),
                                            crate::archive::reader::zip_bytes(
                                                &a.catalog,
                                                id,
                                                2 * 1024 * 1024,
                                                &c,
                                            )?,
                                            false,
                                        ))
                                    });
                                }
                            }
                        }
                        Item::Method(d, m) => {
                            if let Some(a) = self.package.clone() {
                                self.start(move |c| {
                                    let dex = a.dex[d]
                                        .1
                                        .as_ref()
                                        .map_err(|e| io::Error::other(e.clone()))?;
                                    Ok(Reply::Preview(
                                        dex.methods[m].name.clone(),
                                        dex.instructions(m, &c)?.join("\n").into_bytes(),
                                        true,
                                    ))
                                });
                            }
                        }
                        Item::Device(i) => {
                            if let Some(device) = self.devices.get(i) {
                                if device.state == "device" {
                                    self.serial = Some(device.serial.clone());
                                    self.notice = format!("Selected device: {}", device.serial);
                                } else {
                                    self.notice = format!("Device is {}", device.state);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            3 => {
                if let (Some(p), Some(a), Item::Member(id)) =
                    (&self.project, &self.package, self.selected())
                {
                    let name = String::from_utf8_lossy(&a.catalog.nodes[id].path);
                    match p.working_path(&name) {
                        Ok(path) => return vec![Effect::ApkEdit(path)],
                        Err(e) => self.notice = e.to_string(),
                    }
                } else {
                    self.notice = "Create a project and select an editable file".into();
                }
            }
            4 => self.form(FormKind::Extract),
            5 => self.form(FormKind::Manifest),
            6 => {
                if self.project.is_some() {
                    let name = if matches!(self.section, Section::Overview | Section::Manifest) {
                        Some("AndroidManifest.xml".into())
                    } else {
                        match self.selected() {
                            Item::Change(n) => Some(n),
                            Item::Member(id) => self.package.as_ref().map(|a| {
                                String::from_utf8_lossy(&a.catalog.nodes[id].path).into_owned()
                            }),
                            _ => None,
                        }
                    };
                    if let Some(name) = name {
                        let mut f = Form::new(
                            FormKind::Restore,
                            vec!["Discard changes to this file?"],
                            vec![name],
                            None,
                        );
                        f.focus = 2;
                        self.form = Some(f);
                    }
                }
            }
            7 => {
                if let Some(a) = self.package.clone() {
                    let query = self.filter.clone();
                    if query.is_empty() {
                        self.notice = "Enter a string query in Filter first".into();
                    } else {
                        self.start(move |c| {
                            let mut rows = Vec::new();
                            for (name, dex) in &a.dex {
                                if let Ok(dex) = dex {
                                    rows.push(name.clone());
                                    rows.extend(dex.string_uses(&query, &c)?);
                                }
                            }
                            Ok(Reply::Rows(rows))
                        });
                    }
                }
            }
            30 => {
                self.log = None;
                let tools = self.tools.clone();
                self.start(move |c| Ok(Reply::Devices(tools.devices(&c)?)));
            }
            31..=33 => {
                let Some(serial) = self.serial.clone() else {
                    self.notice = "Select a connected device first".into();
                    return vec![];
                };
                let action = match id {
                    31 => {
                        if !self.signed {
                            self.notice = "Export and sign an APK first".into();
                            return vec![];
                        }
                        let Some(path) = self.exported.clone() else {
                            self.notice = "Export and sign an APK first".into();
                            return vec![];
                        };
                        DeviceAction::Install(path)
                    }
                    32 => {
                        let Some(m) = self.package.as_ref().map(|a| &a.manifest) else {
                            return vec![];
                        };
                        let Some(activity) = m.launcher.clone() else {
                            self.notice = "No launcher activity found".into();
                            return vec![];
                        };
                        DeviceAction::Start {
                            package: m.package.clone(),
                            activity,
                        }
                    }
                    _ => {
                        let Some(m) = self.package.as_ref().map(|a| &a.manifest) else {
                            return vec![];
                        };
                        DeviceAction::Stop(m.package.clone())
                    }
                };
                let tools = self.tools.clone();
                self.start(move |c| Ok(Reply::Notice(tools.device_action(&serial, action, &c)?)));
            }
            34 => {
                if self.log.take().is_none() {
                    if let Some(serial) = &self.serial {
                        match self.tools.logcat(serial) {
                            Ok(log) => self.log = Some(log),
                            Err(e) => self.notice = e.to_string(),
                        }
                    } else {
                        self.notice = "Select a connected device first".into();
                    }
                }
            }
            35 => self.pick(35, ctx),
            _ => {}
        }
        vec![]
    }
    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        match self.section {
            Section::Overview => {
                if let Some(a) = &self.package {
                    rows.extend(
                        a.overview_manifest(
                            self.effective_manifest.as_ref().unwrap_or(&a.manifest),
                        )
                        .into_iter()
                        .map(|s| (s, Item::None)),
                    );
                }
            }
            Section::Manifest => {
                if let Some(a) = &self.package {
                    rows.extend(
                        self.effective_manifest
                            .as_ref()
                            .unwrap_or(&a.manifest)
                            .xml
                            .lines()
                            .map(|s| (s.into(), Item::None)),
                    );
                    if let Some(resources) = &a.resources {
                        rows.extend(
                            resources
                                .rows()
                                .into_iter()
                                .map(|s| (s.clone(), Item::Text(s))),
                        );
                    }
                }
            }
            Section::Files => {
                if let Some(a) = &self.package {
                    let c = &a.catalog;
                    if self.prefix != 0 {
                        rows.push(("..".into(), Item::Member(c.nodes[self.prefix].parent)));
                    }
                    for &id in &c.nodes[self.prefix].children {
                        let n = &c.nodes[id];
                        rows.push((
                            format!(
                                "{} {}  {}",
                                if n.kind == crate::archive::reader::Kind::Directory {
                                    "[D]"
                                } else {
                                    "   "
                                },
                                n.name(),
                                n.size
                            ),
                            Item::Member(id),
                        ));
                    }
                }
            }
            Section::Dex => {
                if let Some(a) = &self.package {
                    for (d, (name, value)) in a.dex.iter().enumerate() {
                        match value {
                            Ok(dex) => {
                                rows.push((
                                    format!(
                                        "{name}: {} classes, {} methods",
                                        dex.classes.len(),
                                        dex.methods.len()
                                    ),
                                    Item::None,
                                ));
                                rows.extend(
                                    dex.methods
                                        .iter()
                                        .enumerate()
                                        .map(|(m, v)| (v.name.clone(), Item::Method(d, m))),
                                );
                                rows.extend(
                                    dex.strings
                                        .iter()
                                        .map(|s| (format!("String: {s:?}"), Item::Text(s.clone()))),
                                );
                            }
                            Err(e) => rows.push((format!("{name}: {e}"), Item::None)),
                        }
                    }
                }
            }
            Section::Changes => rows.extend(self.detail.iter().map(|s| {
                (
                    s.clone(),
                    Item::Change(s.split_once(": ").map_or(s.as_str(), |(_, n)| n).into()),
                )
            })),
            Section::Device => {
                if let Some(log) = &self.log {
                    rows.extend(log.lines.iter().map(|s| (s.clone(), Item::None)));
                } else {
                    rows.extend(self.devices.iter().enumerate().map(|(i, d)| {
                        (
                            format!(
                                "{} {} {}{}",
                                d.serial,
                                d.state,
                                d.description,
                                if self.serial.as_ref() == Some(&d.serial) {
                                    " *"
                                } else {
                                    ""
                                }
                            ),
                            Item::Device(i),
                        )
                    }));
                }
            }
        }
        if !self.detail.is_empty() && self.section == Section::Dex {
            rows = self
                .detail
                .iter()
                .map(|s| (s.clone(), Item::None))
                .collect();
        }
        let filter = self.filter.to_lowercase();
        self.rows = rows
            .into_iter()
            .filter(|(s, _)| filter.is_empty() || s.to_lowercase().contains(&filter))
            .take(100_000)
            .collect();
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        crate::list_state::ListViewport::fit(
            &mut self.cursor,
            &mut self.offset,
            10,
            Some(self.rows.len().saturating_sub(1)),
            None,
        );
    }
    fn poll(&mut self, ctx: &Context) -> Vec<Effect> {
        if let Some(log) = &mut self.log {
            log.poll();
            if log.disconnected {
                self.notice = "Logcat stopped or device disconnected".into();
            }
        }
        self.wake = ctx.now + Duration::from_millis(50);
        let Some(reply) = crate::task::take_reply(&mut self.task) else {
            if self.log.is_some() {
                self.rebuild();
            }
            return vec![];
        };
        let reply = reply.unwrap_or_else(|_| Err(io::Error::other("APK task stopped")));
        self.notice.clear();
        match reply {
            Err(e) => self.notice = e.to_string(),
            Ok(Reply::Loaded(a, p, manifest)) => {
                self.effective_manifest = manifest;
                self.package = Some(a);
                self.project = p;
                self.prefix = 0;
                self.cursor = 0;
                self.offset = 0;
                self.section = Section::Overview;
                self.detail.clear();
                self.filter.clear();
                self.filter_cursor = 0;
                self.exported = None;
            }
            Ok(Reply::Project(p, manifest)) => {
                self.effective_manifest = Some(manifest);
                self.project = Some(p);
                self.notice = "Project saved".into();
            }
            Ok(Reply::Rows(rows)) => self.detail = rows,
            Ok(Reply::Preview(label, data, unicode)) => {
                if ctx.active {
                    return vec![Effect::ApkPreview {
                        label,
                        data,
                        unicode,
                    }];
                }
                self.notice = "Preview ready; activate Workshop and request View again".into();
            }
            Ok(Reply::Devices(devices)) => {
                self.devices = devices;
                self.section = Section::Device;
                if !self
                    .devices
                    .iter()
                    .any(|d| Some(&d.serial) == self.serial.as_ref() && d.state == "device")
                {
                    self.serial = None;
                }
            }
            Ok(Reply::Notice(text)) => self.notice = text,
            Ok(Reply::Export(path, signed)) => {
                self.exported = Some(path);
                self.signed = signed;
                self.notice = "APK exported successfully".into();
            }
        }
        self.rebuild();
        vec![]
    }
}
impl ToolPlugin for ApkWorkshop {
    fn definition(&self) -> Definition {
        Definition::builtin(APK_WORKSHOP, "APK Workshop", Presentation::Document)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, ctx: &Context) {
        self.requests = crate::requests::RequestScope::new(match ctx.binding.owner {
            Owner::Document(id) => crate::requests::Owner::Document(id),
            Owner::Modal(id) => crate::requests::Owner::Modal(id),
        });
        self.tools = Tools::discover();
        self.notice = "Open an APK or an APK project".into();
    }
    fn close(&mut self) {
        self.task = None;
        self.log = None;
        self.picker = None;
        self.form = None;
        self.package = None;
        self.project = None;
        self.effective_manifest = None;
        self.exported = None;
    }
    fn next_wake(&self, now: Instant) -> Option<Duration> {
        (self.task.is_some() || self.log.is_some())
            .then_some(self.wake.saturating_duration_since(now))
    }
    fn uses_controls(&self, k: KeyEvent) -> bool {
        self.picker.is_none()
            && !(self.form.is_none()
                && self.focus == LIST
                && matches!(
                    k.code,
                    K::Up
                        | K::Down
                        | K::PageUp
                        | K::PageDown
                        | K::Home
                        | K::End
                        | K::Enter
                        | K::Backspace
                ))
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        if self.picker.is_some() {
            let p = self.picker.as_mut().unwrap();
            let outcome = match event {
                Event::Key(k) => p.key(k),
                Event::Paste(s) => {
                    p.paste(s);
                    None
                }
                Event::Mouse(e, _) => p.mouse(e, (0, 0)),
                _ => None,
            };
            if let Some(outcome) = outcome {
                self.picker = None;
                if let Outcome::Selected(_, path) = outcome {
                    self.base = path.parent().unwrap_or(&self.base).to_owned();
                    match self.picker_job {
                        10 => self.load(path),
                        11 => {
                            if let Some(a) = self.package.clone() {
                                self.start(move |c| {
                                    let p = Project::create(&a, path, &c)?;
                                    let package = Package::open(p.catalog.path.clone(), &c)?;
                                    Ok(Reply::Loaded(Arc::new(package), Some(p), None))
                                });
                            }
                        }
                        14 => {
                            if let Some(p) = self.project.clone() {
                                self.start(move |c| {
                                    p.export(&path, &c)?;
                                    Ok(Reply::Export(path, false))
                                });
                            }
                        }
                        21 => {
                            self.signing_destination = Some(path);
                            self.form(FormKind::Sign);
                        }
                        35 => {
                            if let Some(log) = &self.log {
                                self.notice = match log.save(&path) {
                                    Ok(()) => "Log saved".into(),
                                    Err(e) => e.to_string(),
                                };
                            }
                        }
                        _ => {}
                    }
                }
            }
            return vec![];
        }
        match event {
            Event::ApkContext {
                base,
                destination,
                path,
            } => {
                self.base = base;
                self.destination = destination;
                if let Some(path) = path {
                    self.load(path);
                } else if self.package.is_none() {
                    self.pick(10, ctx);
                }
                vec![]
            }
            Event::OpenApk(path) => {
                self.picker = None;
                self.load(path);
                vec![]
            }
            Event::Tick => self.poll(ctx),
            Event::Focus(id) => {
                if let Some(f) = &mut self.form {
                    f.focus = id;
                } else {
                    self.focus = id;
                }
                vec![]
            }
            Event::Action(ActionId(id)) => self.action(id, ctx),
            Event::Key(k) if k.code == K::Esc => self.action(0, ctx),
            Event::Key(k) if k.code == K::F(1) => self.action(1, ctx),
            Event::Key(k) if self.form.is_some() => {
                let f = self.form.as_mut().unwrap();
                if f.kind != FormKind::Restore
                    && f.focus < f.fields.len()
                    && !k.modifiers.intersects(M::CONTROL | M::ALT)
                {
                    let i = f.focus;
                    if f.fields[i].len() < 16384 || !matches!(k.code, K::Char(_)) {
                        crate::dialog::edit_input(&mut f.fields[i], &mut f.cursors[i], k.code);
                    }
                }
                vec![]
            }
            Event::Paste(s) => {
                if let Some(f) = &mut self.form {
                    let i = f.focus;
                    if f.kind != FormKind::Restore
                        && i < f.fields.len()
                        && f.fields[i].len() + s.len() <= 16384
                    {
                        crate::dialog::paste_input(&mut f.fields[i], &mut f.cursors[i], s);
                    }
                } else if self.focus == FILTER && self.filter.len() + s.len() < 1024 {
                    crate::dialog::paste_input(&mut self.filter, &mut self.filter_cursor, s);
                    self.rebuild();
                }
                vec![]
            }
            Event::Key(k) => {
                if self.focus == FILTER && !k.modifiers.intersects(M::CONTROL | M::ALT) {
                    if self.filter.len() < 1024 || !matches!(k.code, K::Char(_)) {
                        crate::dialog::edit_input(
                            &mut self.filter,
                            &mut self.filter_cursor,
                            k.code,
                        );
                        self.detail.clear();
                        self.cursor = 0;
                        self.rebuild();
                    }
                    return vec![];
                }
                let id = match k.code {
                    K::F(3) => 2,
                    K::F(4) => 3,
                    K::F(5) => 4,
                    K::Enter if self.focus == LIST => 2,
                    K::Backspace if self.section == Section::Files => {
                        if let Some(a) = &self.package {
                            self.prefix = a.catalog.nodes[self.prefix].parent;
                            self.cursor = 0;
                            self.rebuild();
                        }
                        return vec![];
                    }
                    K::Up => {
                        self.cursor = self.cursor.saturating_sub(1);
                        return vec![];
                    }
                    K::Down => {
                        self.cursor = (self.cursor + 1).min(self.rows.len().saturating_sub(1));
                        return vec![];
                    }
                    K::PageUp => {
                        self.cursor = self.cursor.saturating_sub(10);
                        return vec![];
                    }
                    K::PageDown => {
                        self.cursor = (self.cursor + 10).min(self.rows.len().saturating_sub(1));
                        return vec![];
                    }
                    K::Home => {
                        self.cursor = 0;
                        return vec![];
                    }
                    K::End => {
                        self.cursor = self.rows.len().saturating_sub(1);
                        return vec![];
                    }
                    _ => return vec![],
                };
                self.action(id, ctx)
            }
            Event::Mouse(e, Some(ActionId(id)))
                if e.kind == MouseEventKind::Down(MouseButton::Left) =>
            {
                self.action(id, ctx)
            }
            Event::Mouse(e, _)
                if matches!(
                    e.kind,
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                ) =>
            {
                self.cursor = self
                    .cursor
                    .saturating_add_signed(crate::list_state::wheel_delta(e.kind))
                    .min(self.rows.len().saturating_sub(1));
                vec![]
            }
            _ => vec![],
        }
    }
    fn prepare(&mut self, ctx: &Context) {
        if let Some(p) = &mut self.picker {
            p.prepare(
                crate::presentation::layout::Size::new(ctx.area.width, ctx.area.height),
                ctx.palette,
            );
        }
        crate::list_state::ListViewport::fit(
            &mut self.cursor,
            &mut self.offset,
            10,
            Some(self.rows.len().saturating_sub(1)),
            None,
        );
    }
    fn native_input_page(&self) -> Option<u64> {
        self.picker.as_ref().map(|_| PICKER)
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        if let Some(picker) = &self.picker {
            let mut s = ctx.dialog(picker.bounds(), 0, ctx.language.text("Open file"), 900, 0);
            let mut c = Control::new(900, s.bounds, ControlKind::Surface, ctx.target(900));
            c.paint = false;
            s.control(c);
            s
        } else if self.form.is_some() {
            self.form_spec(ctx)
        } else {
            self.main_spec(ctx)
        }
    }
    fn windows(&self, ctx: &Context) -> Vec<LocalWindow> {
        let r = centered(ctx.area, 78, 23);
        let mut windows = vec![LocalWindow {
            id: 0,
            parent: None,
            bounds: r,
            caps: Capabilities::fixed_zoom(r.width, r.height),
        }];
        if self.form.is_some() {
            let r = self.form_spec(ctx).bounds;
            windows.push(LocalWindow {
                id: PAGE,
                parent: Some(0),
                bounds: r,
                caps: Capabilities::fixed(r.width, r.height),
            });
        }
        if let Some(p) = &self.picker {
            let r = p.bounds();
            windows.push(LocalWindow {
                id: PICKER,
                parent: Some(0),
                bounds: r,
                caps: Capabilities::fixed(r.width, r.height),
            });
        }
        windows
    }
    fn view(&self, ctx: &Context) -> View<'_> {
        let spec = self.main_spec(ctx);
        let mut frame = NativeFrame::new(ctx.area, ctx.palette);
        use crate::painting::Frame;
        frame.render_widget(
            crate::turbo::widgets::dialog(ctx.language.text("APK Workshop")),
            spec.bounds,
        );
        let mut layers = vec![Layer {
            id: 0,
            interactive: self.form.is_none() && self.picker.is_none(),
            scene: frame.finish(),
            controls: Some(spec),
        }];
        if self.form.is_some() {
            let s = self.form_spec(ctx);
            let mut f = NativeFrame::new(ctx.area, ctx.palette);
            f.render_widget(
                crate::turbo::widgets::dialog(ctx.language.text("APK Workshop")),
                s.bounds,
            );
            layers.push(Layer {
                id: PAGE,
                interactive: true,
                scene: f.finish(),
                controls: Some(s),
            });
        }
        if let Some(p) = &self.picker {
            if let Some(mut scene) = p.scene().cloned() {
                scene.regions = vec![(p.bounds(), ctx.target(900))];
                layers.push(Layer {
                    id: PICKER,
                    interactive: true,
                    scene,
                    controls: None,
                });
            }
        }
        View::Layers(layers)
    }
}
impl ApkWorkshop {
    fn main_spec(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = centered(ctx.area, 78, 23);
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let mut s = ctx.dialog(r, self.focus, ctx.language.text("APK Workshop"), 2, 0);
        for (i, title) in TITLES.iter().enumerate() {
            let mut c = Control::new(
                100 + i,
                Rect::new(r.x + 2 + i as u16 * 12, r.y + 2, 11, 1),
                ControlKind::Button(ctx.language.text(title).into()),
                ctx.target(100 + i),
            );
            c.enter_action = true;
            s.control(c);
        }
        s.caption(
            Paragraph::new(ctx.language.text("Filter")).style(style(0x70)),
            Rect::new(r.x + 2, r.y + 4, 9, 1),
        );
        s.control(Control::new(
            FILTER,
            Rect::new(r.x + 12, r.y + 4, 63, 1),
            ControlKind::ScrolledInput {
                value: self.filter.clone(),
                scroll: crate::presentation::text::Line::from(&self.filter[..self.filter_cursor])
                    .width()
                    .saturating_sub(62),
            },
            ctx.target(FILTER),
        ));
        if self.focus == FILTER {
            s.cursor = Some((
                r.x + 12
                    + (crate::presentation::text::Line::from(&self.filter[..self.filter_cursor])
                        .width()
                        .min(62) as u16),
                r.y + 4,
            ));
        }
        let items = self
            .rows
            .iter()
            .map(|(s, _)| crate::filesystem::display_name(s))
            .collect::<Vec<_>>();
        let mut list = Control::new(
            LIST,
            Rect::new(r.x + 2, r.y + 6, 73, 10),
            ControlKind::List {
                items,
                cursor: self.cursor,
                offset: self.offset,
            },
            ctx.target(LIST),
        );
        list.dn_list = true;
        list.enter_action = false;
        list.hits = (0..10)
            .filter_map(|row| {
                self.rows.get(self.offset + row).map(|_| {
                    (
                        Rect::new(r.x + 2, r.y + 6 + row as u16, 73, 1),
                        ctx.target(ROW + self.offset + row),
                    )
                })
            })
            .collect();
        s.control(list);
        let main = [
            (10, "Open…"),
            (
                11,
                if self.project.is_some() {
                    "Work dir"
                } else {
                    "Project…"
                },
            ),
            (14, "Export…"),
            (21, "Sign…"),
            (22, "Tools…"),
            (0, "Close"),
        ];
        let second = match self.section {
            Section::Device => vec![
                (30, "Devices"),
                (31, "Install"),
                (32, "Launch"),
                (33, "Stop"),
                (
                    34,
                    if self.log.is_some() {
                        "Stop log"
                    } else {
                        "Logcat"
                    },
                ),
                (35, "Save log"),
            ],
            Section::Files => vec![
                (2, "View"),
                (3, "Edit"),
                (4, "Extract…"),
                (6, "Restore"),
                (13, "Refresh"),
                (1, "Help"),
            ],
            Section::Dex => vec![
                (2, "View"),
                (7, "Uses"),
                (13, "Refresh"),
                (23, "Test key"),
                (1, "Help"),
            ],
            Section::Changes => vec![
                (6, "Restore"),
                (13, "Refresh"),
                (5, "Edit"),
                (23, "Test key"),
                (1, "Help"),
            ],
            _ => vec![
                (2, "View XML"),
                (5, "Edit"),
                (6, "Reset"),
                (23, "Test key"),
                (1, "Help"),
            ],
        };
        for (y, buttons) in [(17, main.to_vec()), (19, second)] {
            for (i, (id, label)) in buttons.iter().enumerate() {
                let mut c = Control::new(
                    *id,
                    Rect::new(r.x + 2 + i as u16 * 12, r.y + y, 11, 1),
                    ControlKind::Button(ctx.language.text(label).into()),
                    ctx.target(*id),
                );
                c.enter_action = true;
                c.enabled = self.task.is_none() || *id == 0 || *id == 1;
                if matches!(id, 14 | 21 | 3 | 5 | 6) {
                    c.enabled &= self.project.is_some();
                }
                if *id == 11 {
                    c.enabled &= self.package.is_some();
                }
                if *id == 21 {
                    c.enabled &= self.tools.signing_available();
                }
                if *id == 2 {
                    c.enabled &= self.package.is_some();
                }
                if *id == 4 {
                    c.enabled &= self.section == Section::Files
                        && matches!(self.selected(),Item::Member(n) if n!=self.prefix && self.package.as_ref().is_some_and(|a|n!=a.catalog.nodes[self.prefix].parent && n!=0));
                }
                if *id == 3 {
                    c.enabled &= matches!(self.selected(),Item::Member(n) if self.package.as_ref().is_some_and(|a|a.catalog.nodes[n].kind==crate::archive::reader::Kind::File && apk::project::editable(&String::from_utf8_lossy(&a.catalog.nodes[n].path))));
                }
                if *id == 7 {
                    c.enabled &= !self.filter.is_empty();
                }
                if *id == 23 {
                    c.enabled &= self.tools.keytool.is_some();
                }
                if (30..=34).contains(id) {
                    c.enabled &= self.tools.adb.is_some();
                }
                if (31..=34).contains(id) {
                    c.enabled &= self.serial.is_some();
                }
                if *id == 31 {
                    c.enabled &= self.signed && self.exported.is_some();
                }
                if *id == 32 {
                    c.enabled &= self
                        .package
                        .as_ref()
                        .is_some_and(|a| a.manifest.launcher.is_some());
                }
                if *id == 33 {
                    c.enabled &= self.package.is_some();
                }
                if *id == 35 {
                    c.enabled &= self.log.is_some();
                }

                s.control(c);
            }
        }
        let notice = if self.notice.is_empty() {
            self.project
                .as_ref()
                .map(|p| p.root.display().to_string())
                .or_else(|| {
                    self.package
                        .as_ref()
                        .map(|p| p.catalog.path.display().to_string())
                })
                .unwrap_or_else(|| ctx.language.text("Open an APK or an APK project").into())
        } else {
            ctx.language.text(&self.notice).into()
        };
        s.caption(
            Paragraph::new(crate::formatting::tail(&notice, 74)).style(style(0x74)),
            Rect::new(r.x + 2, r.y + 21, 74, 1),
        );
        s
    }
    fn form_spec(&self, ctx: &Context) -> DialogSpec<'static> {
        let f = self.form.as_ref().unwrap();
        let height = (f.fields.len() * 2 + 6) as u16;
        let r = centered(ctx.area, 74, height);
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let mut s = ctx.dialog(
            r,
            f.focus,
            ctx.language.text("APK Workshop"),
            if f.kind == FormKind::Restore { 0 } else { 800 },
            0,
        );
        for (i, (label, value)) in f.labels.iter().zip(&f.fields).enumerate() {
            let y = r.y + 2 + i as u16 * 2;
            s.caption(
                Paragraph::new(ctx.language.text(label)).style(style(0x70)),
                Rect::new(r.x + 2, y, 69, 1),
            );
            let value = if f.secret.is_some_and(|start| i >= start) {
                "*".repeat(value.chars().count())
            } else {
                value.clone()
            };
            let column = if f.secret.is_some_and(|start| i >= start) {
                f.fields[i][..f.cursors[i]].chars().count()
            } else {
                crate::presentation::text::Line::from(&f.fields[i][..f.cursors[i]]).width()
            };
            let mut c = Control::new(
                i,
                Rect::new(r.x + 2, y + 1, 69, 1),
                ControlKind::ScrolledInput {
                    value,
                    scroll: column.saturating_sub(68),
                },
                ctx.target(700 + i),
            );
            c.enter_action = false;
            c.enabled = f.kind != FormKind::Restore;
            s.control(c);
            if f.focus == i {
                s.cursor = Some((r.x + 2 + column.min(68) as u16, y + 1));
            }
        }
        s.caption(
            Paragraph::new(crate::formatting::tail(&f.error, 69)).style(style(0x74)),
            Rect::new(r.x + 2, r.bottom() - 3, 69, 1),
        );
        for (id, x, label) in [(800, 14, "OK"), (0, 43, "Cancel")] {
            let mut c = Control::new(
                f.fields.len() + usize::from(id == 0),
                Rect::new(r.x + x, r.bottom() - 2, 15, 1),
                ControlKind::Button(ctx.language.text(label).into()),
                ctx.target(id),
            );
            c.enter_action = true;
            s.control(c);
        }
        s
    }
}
