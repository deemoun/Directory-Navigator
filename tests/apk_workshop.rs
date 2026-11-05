#[path = "support/apk.rs"]
mod fixture;
mod support;
use dn::{
    apk::{
        self,
        manifest::{Changes, Manifest},
        project::Project,
    },
    task::Cancellation,
};
use std::{fs, io};
#[test]
fn binary_manifest_roundtrip_keeps_unknown_attributes_and_unicode() -> io::Result<()> {
    let c = Cancellation::default();
    let original = fixture::manifest();
    let m = Manifest::parse(&original, &c)?;
    assert_eq!(m.package, "org.dn.fixture");
    assert_eq!(m.launcher.as_deref(), Some(".MainActivity"));
    assert!(m.xml.contains("Тест 界"));
    let changes = Changes {
        debuggable: Some(true),
        version_code: Some(u32::MAX),
        version_name: Some("Новое 👩‍💻".into()),
        permissions: Some(vec![
            "android.permission.INTERNET".into(),
            "android.permission.CAMERA".into(),
        ]),
    };
    let data = m.patch(&changes, &c)?;
    let p = Manifest::parse(&data, &c)?;
    assert!(p.debuggable);
    assert_eq!(p.version_code, u32::MAX.to_string());
    assert_eq!(p.version_name, "Новое 👩‍💻");
    assert_eq!(p.permissions.len(), 2);
    assert!(p.xml.contains("maxSdkVersion=\"35\""));
    assert_eq!(fixture::manifest(), original);
    let removed = p.patch(
        &Changes {
            permissions: Some(vec![]),
            ..Default::default()
        },
        &c,
    )?;
    assert!(Manifest::parse(&removed, &c)?.permissions.is_empty());
    Ok(())
}
#[test]
fn dex_inspection_resolves_strings_methods_and_usages_without_execution() -> io::Result<()> {
    let c = Cancellation::default();
    let d = apk::dex::Dex::parse(fixture::dex(), &c)?;
    assert_eq!(d.classes, ["Lorg/dn/fixture/Main;"]);
    assert!(d
        .instructions(0, &c)?
        .join("\n")
        .contains("https://example.test/界"));
    assert!(d.string_uses("example.test", &c)?[0].contains("->run()V"));
    for n in 0..112 {
        assert!(apk::dex::Dex::parse(fixture::dex()[..n].to_vec(), &c).is_err());
    }
    Ok(())
}
#[test]
fn project_reopen_edit_restore_export_preserves_source_and_protected_members() -> io::Result<()> {
    let t = support::Fixture::new("apk-project")?.canonicalized()?;
    let c = Cancellation::default();
    let source = t.0.join("input.apk");
    fixture::write(&source)?;
    let original = fs::read(&source)?;
    let package = apk::Package::open(source.clone(), &c)?;
    assert_eq!(
        package.resources.as_ref().unwrap().values[&0x7f010000][0].1,
        "DN Fixture 界"
    );
    let mut project = Project::create(&package, t.0.join("project"), &c)?;
    fs::write(project.root.join("files/assets/config.txt"), "modified 界")?;
    fs::write(project.root.join("files/assets/new.txt"), "added")?;
    let changes = project.changes(&c)?;
    assert!(changes.contains(&"Modified: assets/config.txt".into()));
    assert!(changes.contains(&"Added: assets/new.txt".into()));
    project.restore("assets/new.txt", &c)?;
    project.save_changes(
        Changes {
            debuggable: Some(true),
            ..Default::default()
        },
        &c,
    )?;
    let project = Project::open(project.root.clone(), &c)?;
    assert!(project.changes.debuggable.unwrap());
    let target = t.0.join("output.apk");
    project.export(&target, &c)?;
    let updated = apk::Package::open(target.clone(), &c)?;
    assert!(updated.manifest.debuggable);
    let mut zip = zip::ZipArchive::new(fs::File::open(target)?).map_err(io::Error::other)?;
    assert!(zip.by_name("META-INF/CERT.SF").is_err());
    assert!(zip.by_name("META-INF/services/example").is_ok());
    assert!(zip.by_name("resources.arsc").is_ok());
    assert_eq!(fs::read(source)?, original);
    project.restore("assets/config.txt", &c)?;
    assert_eq!(
        fs::read(project.root.join("files/assets/config.txt"))?,
        b"original config\n"
    );
    assert!(project.export(&t.0.join("output.apk"), &c).is_err());
    Ok(())
}
#[test]
fn cancellation_bad_binary_and_workspace_links_do_not_publish() -> io::Result<()> {
    let c = Cancellation::default();
    let data = fixture::manifest();
    for n in 0..data.len() {
        assert!(Manifest::parse(&data[..n], &c).is_err());
    }
    let t = support::Fixture::new("apk-safeguards")?.canonicalized()?;
    let source = t.0.join("input.apk");
    fixture::write(&source)?;
    let package = apk::Package::open(source, &c)?;
    let p = Project::create(&package, t.0.join("project"), &c)?;
    assert!(p.working_path("../escape").is_err());
    assert!(p.working_path("classes.dex").is_err());
    fs::write(p.root.join("files/classes.dex"), "protected")?;
    let output = t.0.join("no.apk");
    assert!(p.export(&output, &c).is_err());
    assert!(!output.exists());
    fs::remove_file(p.root.join("files/classes.dex"))?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&t.0, p.root.join("files/link"))?;
        assert!(p.export(&output, &c).is_err());
        fs::remove_file(p.root.join("files/link"))?;
    }
    c.cancel();
    assert!(p.export(&output, &c).is_err());
    assert!(!output.exists());
    assert!(!fs::read_dir(&t.0)?
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().starts_with(".dn-apk-")));
    Ok(())
}

fn wait_plugin(app: &mut dn::app::App) -> io::Result<()> {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        app.poll();
        let text = support::draw(app, 80, 25)?
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        if text.contains("org.dn.fixture") {
            return Ok(());
        }
        if std::time::Instant::now() > end {
            return Err(io::Error::other(format!("APK did not load: {text}")));
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
#[test]
fn plugin_enter_routing_languages_pages_immutable_frames_and_owned_forms() -> io::Result<()> {
    use dn::{
        desktop::WindowKind,
        localization::Language,
        mouse::Target,
        palette::PaletteProfile,
        plugins::{ActionId, APK_WORKSHOP},
    };
    for language in [Language::English, Language::Russian] {
        for palette in [
            PaletteProfile::ClassicDn,
            PaletteProfile::Terminal,
            PaletteProfile::ClassicDn.with_slot(dn::palette::theme::Slot::DialogText, 0x2f),
        ] {
            let t = support::Fixture::new("apk-plugin")?.canonicalized()?;
            fixture::write(&t.0.join("FIXTURE.APK"))?;
            let mut app = t.app()?;
            assert!(!app.config.plugin_enabled(APK_WORKSHOP));
            app.config.set_plugin_enabled(APK_WORKSHOP, true);
            app.config.language = language;
            app.config.palette = palette;
            app.config.show_clock = false;
            app.panels[0].cursor = app.panels[0]
                .entries
                .iter()
                .position(|e| e.name == "FIXTURE.APK")
                .unwrap();
            app.open_current();
            assert_eq!(app.desktop.active().kind, WindowKind::Plugin(APK_WORKSHOP));
            wait_plugin(&mut app)?;
            for section in 0..6 {
                support::draw(&mut app, 80, 25)?;
                support::click(
                    &mut app,
                    Target::Plugin(APK_WORKSHOP, ActionId(100 + section)),
                );
                for (w, h) in [(80, 25), (132, 40), (60, 18), (80, 25)] {
                    let first = support::draw(&mut app, w, h)?;
                    for _ in 0..3 {
                        assert_eq!(first, support::draw(&mut app, w, h)?);
                    }
                }
            }
            support::draw(&mut app, 80, 25)?;
            support::click(&mut app, Target::Plugin(APK_WORKSHOP, ActionId(22)));
            let before = support::draw(&mut app, 80, 25)?;
            assert!(before
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .contains(language.text("ADB executable")));
            support::key(&mut app, crossterm::event::KeyCode::Esc);
            assert!(app
                .modals
                .infos()
                .all(|m| m.focus != dn::focus::Focus::Message));
            support::draw(&mut app, 80, 25)?;
            support::click(&mut app, Target::Plugin(APK_WORKSHOP, ActionId(100)));
            support::draw(&mut app, 80, 25)?;
            support::key(&mut app, crossterm::event::KeyCode::F(1));
            assert!(app.last_dialog().is_some());
            support::key(&mut app, crossterm::event::KeyCode::Esc);
            support::key(&mut app, crossterm::event::KeyCode::Esc);
            assert_eq!(app.desktop.active().kind, WindowKind::Manager);
        }
    }
    Ok(())
}

#[test]
fn malformed_offsets_are_errors_instead_of_panics_or_unbounded_work() {
    let c = Cancellation::default();
    for source in [fixture::manifest(), fixture::dex()] {
        for p in (0..source.len().saturating_sub(4)).step_by(4) {
            let mut changed = source.clone();
            changed[p..p + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            let result = std::panic::catch_unwind(|| {
                if source.starts_with(b"dex") {
                    let _ = apk::dex::Dex::parse(changed, &c);
                } else {
                    let _ = Manifest::parse(&changed, &c);
                }
            });
            assert!(result.is_ok(), "Android parser panicked on offset {p}");
        }
    }
    let invalid = [2, 0, 0xff, 0xff, 8, 0, 0, 0];
    assert!(apk::resources::Resources::parse(&invalid, &c).is_err());
}

#[test]
fn split_packages_are_explicitly_refused() -> io::Result<()> {
    use std::io::Write;
    let t = support::Fixture::new("apk-split")?.canonicalized()?;
    let source = t.0.join("split.apk");
    let mut writer = zip::ZipWriter::new(fs::File::create(&source)?);
    writer
        .start_file(
            "AndroidManifest.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .map_err(io::Error::other)?;
    writer.write_all(&fixture::manifest_with_split(true))?;
    writer.finish().map_err(io::Error::other)?;
    let result = apk::Package::open(source, &Default::default());
    assert!(result.is_err());
    Ok(())
}
