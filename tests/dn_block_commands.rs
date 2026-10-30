mod support;
use crossterm::event::{KeyCode as K, KeyModifiers as M};
use dn::{
    editor::{
        self,
        buffer::Buffer,
        highlight::{Cache, Language},
        Case, EditorAction as E,
    },
    pattern,
    viewer::{self, ViewMode, Viewer},
};
use std::{fs, io};
fn selected(text: &str, start: usize, end: usize) -> Buffer {
    let mut b = Buffer::new(text.into()).unwrap();
    b.move_to(start, false);
    b.move_to(end, true);
    b
}
#[test]
fn block_indent_preserves_direction_eols_and_has_one_undo() {
    let original = "one\r\ntwo\nlast";
    let mut b = selected(original, 8, 1);
    b.tab_size = 4;
    let before = (b.cursor, b.anchor);
    b.indent_block(false, true).unwrap();
    assert_eq!(b.text, "    one\r\n    two\nlast");
    assert!(b.cursor < b.anchor.unwrap());
    let after = (b.cursor, b.anchor);
    b.undo(false);
    assert_eq!(b.text, original);
    assert_eq!((b.cursor, b.anchor), before);
    assert!(!b.dirty());
    b.undo(true);
    assert_eq!((b.cursor, b.anchor), after);
    b.indent_block(true, true).unwrap();
    assert_eq!(b.text, original);
}
#[test]
fn indentation_excludes_next_line_start_and_dedent_is_partial() {
    let mut b = selected("  a\n\tb\nc", 0, 4);
    b.tab_size = 4;
    b.indent_block(true, true).unwrap();
    assert_eq!(b.text, "a\n\tb\nc");
    b.move_to(2, false);
    b.indent_block(true, false).unwrap();
    assert_eq!(b.text, "a\nb\nc");
    b.indent_block(false, false).unwrap();
    assert_eq!(b.text, "a\n\tb\nc");
    let mut b = selected("abc", 0, 3);
    let before = (b.cursor, b.anchor);
    b.indent_block(true, true).unwrap();
    assert!(!b.dirty());
    assert_eq!((b.cursor, b.anchor), before);
}
#[test]
fn unicode_case_expansion_undo_and_capitalization_keep_selection() {
    let mut b = selected(
        "Straße ПРИВЕТ e\u{301}LAN!",
        0,
        "Straße ПРИВЕТ e\u{301}LAN!".len(),
    );
    let original = b.text.clone();
    b.change_case(Case::Upper).unwrap();
    assert_eq!(b.text, "STRASSE ПРИВЕТ E\u{301}LAN!");
    assert_eq!(b.selected(), 0..b.text.len());
    b.undo(false);
    assert_eq!(b.text, original);
    assert!(!b.dirty());
    b.change_case(Case::Capitalize).unwrap();
    assert_eq!(b.text, "Straße Привет E\u{301}lan!");
    b.change_case(Case::Lower).unwrap();
    assert_eq!(b.text, "straße привет e\u{301}lan!");
    b.move_to(0, false);
    assert!(b.change_case(Case::Upper).is_err());
}
#[test]
fn transformations_reject_size_growth_atomically_and_noops_preserve_selection() {
    let text = "ΐ".repeat(dn::editor::buffer::MAX_BYTES / 2);
    let mut b = selected(&text, 0, text.len());
    let before = (b.cursor, b.anchor);
    assert!(b.change_case(Case::Upper).is_err());
    assert_eq!(b.text, text);
    assert!(!b.dirty());
    assert_eq!((b.cursor, b.anchor), before);
    let mut b = selected("ABC", 3, 0);
    b.change_case(Case::Upper).unwrap();
    assert!(!b.dirty());
    assert_eq!(b.cursor, 0);
    assert_eq!(b.anchor, Some(3));
}
#[test]
fn sort_preserves_position_eols_final_newline_and_one_undo() {
    let original = "z\r\na\nb\rlast";
    let mut b = selected(original, 0, 7);
    b.sort_lines().unwrap();
    assert_eq!(b.text, "a\r\nb\nz\rlast");
    assert_eq!(b.selected(), 0..7);
    b.undo(false);
    assert_eq!(b.text, original);
    assert!(!b.dirty());
    b.undo(true);
    assert_eq!(b.text, "a\r\nb\nz\rlast");
    let mut b = selected("z\na", 3, 0);
    b.sort_lines().unwrap();
    assert_eq!(b.text, "a\nz");
    assert_eq!(b.cursor, 0);
    assert_eq!(b.anchor, Some(3));
    b.move_to(0, false);
    assert!(b.sort_lines().is_err());
}
#[test]
fn brackets_respect_nested_structure_and_lexical_strings_comments() {
    for language in [
        Language::Plain,
        Language::Rust,
        Language::C,
        Language::Python,
    ] {
        let mut b = Buffer::new("({[界]})".into()).unwrap();
        let mut c = Cache::new(language);
        c.refresh(&b);
        assert_eq!(b.matching_bracket(&c), Some(b.text.len() - 1));
        b.move_to(b.text.len(), false);
        assert_eq!(b.matching_bracket(&c), Some(0));
        assert!(!b.dirty());
    }
    for (language, text) in [
        (Language::Rust, "(r###\" ) ] \"### /* ) */ [x])"),
        (Language::Python, "(\"\"\" ) \"\"\" # )\n[])"),
        (Language::Pascal, "( ' ) ' { ) } [])"),
    ] {
        let b = Buffer::new(text.into()).unwrap();
        let mut c = Cache::new(language);
        c.refresh(&b);
        assert_eq!(b.matching_bracket(&c), Some(text.len() - 1));
    }
    let b = Buffer::new("([)]".into()).unwrap();
    let mut c = Cache::new(Language::Plain);
    c.refresh(&b);
    assert_eq!(b.matching_bracket(&c), None);
    let mut b = Buffer::new("\"()\"".into()).unwrap();
    b.move_to(1, false);
    let mut c = Cache::new(Language::Rust);
    c.refresh(&b);
    assert_eq!(b.matching_bracket(&c), None);
}
#[test]
fn editor_controls_share_block_commands_and_shortcuts_without_saving() -> io::Result<()> {
    let f = support::Fixture::new("block-controller")?;
    let mut a = f.app()?;
    a.config.editor.expand_tabs = false;
    editor::open(&mut a, true);
    editor::paste(&mut a, "z\na\n");
    editor::action(&mut a, E::SelectAll);
    support::key(&mut a, K::Tab);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "\tz\n\ta\n"
    );
    support::key(&mut a, K::BackTab);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "z\na\n"
    );
    editor::action(&mut a, E::SortLines);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "a\nz\n"
    );
    editor::action(&mut a, E::Undo);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "z\na\n"
    );
    assert!(!f.0.join("untitled").exists());
    Ok(())
}
#[test]
fn related_marks_use_group_identity_and_keep_hidden_marks() -> io::Result<()> {
    let f = support::Fixture::new("related-marks")?;
    for name in ["a.rs", "b.RS", "c.txt", "d.md", "e.py", "plain"] {
        fs::write(f.0.join(name), "")?;
    }
    fs::create_dir(f.0.join("directory.rs"))?;
    let mut a = f.app()?;
    let p = &mut a.panels[0];
    p.cursor = p.entries.iter().position(|e| e.name == "a.rs").unwrap();
    p.selected.insert("filtered.txt".into());
    let mut g = dn::highlight_groups::Groups::default();
    assert_eq!(pattern::related(p, &g, true, true), 2);
    assert!(p.selected.contains(std::ffi::OsStr::new("b.RS")));
    assert!(!p.selected.contains(std::ffi::OsStr::new("directory.rs")));
    assert_eq!(pattern::related(p, &g, true, false), 2);
    assert!(p.selected.contains(std::ffi::OsStr::new("filtered.txt")));
    g.0[0].enabled = true;
    g.0[0].masks = "*.rs;*.txt".into();
    g.0[1].enabled = true;
    g.0[1].masks = "*.py".into();
    g.0[1].color = g.0[0].color;
    assert_eq!(pattern::related(p, &g, false, true), 3);
    assert!(!p.selected.contains(std::ffi::OsStr::new("e.py")));
    assert!(!p.selected.contains(std::ffi::OsStr::new("..")));
    Ok(())
}
#[test]
fn selection_modifier_bindings_leave_plain_plus_and_minus_as_prompts() -> io::Result<()> {
    let f = support::Fixture::new("related-shortcuts")?;
    fs::write(f.0.join("a.rs"), "")?;
    fs::write(f.0.join("b.rs"), "")?;
    let mut a = f.app()?;
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "a.rs")
        .unwrap();
    support::modified(&mut a, K::Char('+'), M::CONTROL | M::SHIFT);
    assert_eq!(a.panels[0].selected.len(), 2);
    assert!(a.modals.pattern().is_none());
    support::modified(&mut a, K::Char('-'), M::CONTROL | M::SHIFT);
    assert!(a.panels[0].selected.is_empty());
    support::key(&mut a, K::Char('+'));
    assert!(a.modals.pattern().is_some());
    support::key(&mut a, K::Esc);
    support::key(&mut a, K::Char('-'));
    assert!(a.modals.pattern().is_some());
    Ok(())
}
fn viewer(f: &support::Fixture, bytes: &[u8], mode: ViewMode) -> io::Result<Viewer> {
    let path = f.0.join("view");
    fs::write(&path, bytes)?;
    let mut v = Viewer::open_mode(path, mode)?;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while v.contents.is_none() {
        v.poll();
        assert!(std::time::Instant::now() < end);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    v.layout(20);
    Ok(v)
}
#[test]
fn viewer_goto_validates_text_lines_and_wrapped_mapping() -> io::Result<()> {
    let f = support::Fixture::new("goto-lines")?;
    let mut v = viewer(
        &f,
        b"first long long long long line\nsecond\nthird",
        ViewMode::Text,
    )?;
    v.wrap = true;
    v.layout(6);
    v.go_to("3").unwrap();
    assert_eq!(v.anchor().0, 2);
    let offset = v.offset;
    for bad in ["0", "4", "-1", "0x2", "999999999999999999999999", "界"] {
        assert!(v.go_to(bad).is_err());
        assert_eq!(v.offset, offset);
    }
    v.go_to(" 1 ").unwrap();
    assert_eq!(v.anchor().0, 0);
    assert_eq!(
        fs::read(f.0.join("view"))?,
        b"first long long long long line\nsecond\nthird"
    );
    Ok(())
}
#[test]
fn viewer_hex_goto_is_exact_bounded_and_handles_empty_files() -> io::Result<()> {
    let f = support::Fixture::new("goto-hex")?;
    let mut v = viewer(&f, &[42; 33], ViewMode::Hex)?;
    v.go_to("0x20").unwrap();
    assert_eq!(v.offset, 2);
    assert_eq!(v.current.as_ref().unwrap().range, 10..12);
    v.go_to("31").unwrap();
    assert_eq!(v.offset, 1);
    assert_eq!(v.current.as_ref().unwrap().range, 55..57);
    assert!(v.go_to("33").is_err());
    assert!(v.go_to("0x").is_err());
    assert!(v.go_to("-1").is_err());
    let mut v = viewer(&f, b"", ViewMode::Hex)?;
    v.go_to("0").unwrap();
    assert!(v.current.is_none());
    assert!(v.go_to("1").is_err());
    let contents = viewer::from_bytes(vec![0; viewer::MAX_BYTES + 1], ViewMode::Hex);
    assert_eq!(contents.loaded_bytes, 100_000 * 16);
    assert!(contents.truncated);
    Ok(())
}
#[test]
fn viewer_goto_owned_controls_keep_drafts_on_error_and_isolate_paste() -> io::Result<()> {
    let f = support::Fixture::new("goto-dialog")?;
    fs::write(f.0.join("view"), "one\ntwo\nthree")?;
    let mut a = f.app()?;
    a.show_viewer(Viewer::open_mode(f.0.join("view"), ViewMode::Text)?);
    support::wait(&mut a);
    support::draw(&mut a, 80, 25)?;
    support::modified(&mut a, K::Char('g'), M::CONTROL);
    assert_eq!(dn::focus::current(&a), dn::focus::Focus::ViewerSearch);
    viewer::paste(&mut a, "999");
    support::key(&mut a, K::Enter);
    assert_eq!(
        a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_goto()
            .unwrap()
            .field,
        "999"
    );
    support::modified(&mut a, K::Char('a'), M::CONTROL);
    viewer::paste(&mut a, "3");
    support::key(&mut a, K::Tab);
    viewer::paste(&mut a, "ignored");
    assert_eq!(
        a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_goto()
            .unwrap()
            .field,
        "3"
    );
    support::key(&mut a, K::Enter);
    support::draw(&mut a, 80, 25)?;
    assert!(a
        .desktop
        .active_viewer()
        .unwrap()
        .modals
        .viewer_goto()
        .is_none());
    assert_eq!(a.desktop.active_viewer().unwrap().anchor().0, 2);
    viewer::action(&mut a, viewer::Action::GoTo);
    support::modified(&mut a, K::Tab, M::SHIFT);
    assert_eq!(
        a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_goto()
            .unwrap()
            .focus,
        2
    );
    support::key(&mut a, K::Enter);
    assert!(a
        .desktop
        .active_viewer()
        .unwrap()
        .modals
        .viewer_goto()
        .is_none());
    Ok(())
}

#[test]
fn goto_render_mouse_resize_and_retired_owner_are_consistent() -> io::Result<()> {
    use dn::{
        layout,
        localization::Language,
        mouse::{self, Target},
        palette::PaletteProfile,
        presentation::layout::Rect,
        ui,
    };
    let f = support::Fixture::new("goto-frame")?;
    for language in [Language::English, Language::Russian] {
        for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            let mut a = f.app()?;
            a.config.language = language;
            a.config.palette = palette;
            a.config.palette = palette.with_slot(dn::palette::theme::Slot::DialogText, 0x1e);
            a.show_viewer(Viewer::from_contents(
                "preview".into(),
                viewer::from_bytes(b"one\ntwo\nthree".to_vec(), ViewMode::Text),
            ));
            viewer::action(&mut a, viewer::Action::GoTo);
            viewer::paste(&mut a, "2");
            for (w, h) in [(80, 25), (132, 40)] {
                let snap = layout::prepare(&mut a, Rect::new(0, 0, w, h));
                let first = ui::render(&a, &snap);
                let second = ui::render(&a, &snap);
                assert_eq!(
                    format!("{:?}", dn::testing::Buffer::rendered(first)),
                    format!("{:?}", dn::testing::Buffer::rendered(second))
                );
                support::draw(&mut a, w, h)?;
                assert_eq!(
                    a.desktop
                        .active_viewer()
                        .unwrap()
                        .modals
                        .viewer_goto()
                        .unwrap()
                        .field,
                    "2"
                );
            }
            support::click(&mut a, Target::Viewer(viewer::Action::Submit));
            assert!(a
                .desktop
                .active_viewer()
                .unwrap()
                .modals
                .viewer_goto()
                .is_none());
            viewer::action(&mut a, viewer::Action::GoTo);
            support::draw(&mut a, 80, 25)?;
            let bounds = a
                .mouse
                .regions
                .iter()
                .find(|(_, t)| *t == Target::Viewer(viewer::Action::Submit))
                .unwrap()
                .0;
            // Publish a different owner before sending a click to the retired form.
            viewer::action(&mut a, viewer::Action::Close);
            editor::open(&mut a, true);
            mouse::handle(
                &mut a,
                crossterm::event::MouseEvent {
                    kind: crossterm::event::MouseEventKind::Down(
                        crossterm::event::MouseButton::Left,
                    ),
                    column: bounds.x,
                    row: bounds.y,
                    modifiers: M::NONE,
                },
            );
            assert!(a.desktop.active_editor().is_some());
            assert!(a
                .desktop
                .active_editor()
                .unwrap()
                .buffer
                .as_ref()
                .unwrap()
                .text
                .is_empty());
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn related_extensions_preserve_native_undecodable_names() -> io::Result<()> {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let f = support::Fixture::new("native-extension")?;
    for name in ["a", "b", "c"] {
        fs::write(f.0.join(name), "")?;
    }
    let mut a = f.app()?;
    let p = &mut a.panels[0];
    // APFS rejects undecodable names: exercise the domain entries without creating them.
    for e in &mut p.entries {
        for (name, bytes) in [
            ("a", b"a.\xff".as_slice()),
            ("b", b"b.\xff"),
            ("c", b"c.\xfe"),
        ] {
            if e.name == name {
                e.name = OsStr::from_bytes(bytes).to_owned();
                e.path = f.0.join(&e.name);
                break;
            }
        }
    }
    p.cursor = p
        .entries
        .iter()
        .position(|e| e.name == OsStr::from_bytes(b"a.\xff"))
        .unwrap();
    assert_eq!(pattern::related(p, &Default::default(), true, true), 2);
    assert!(p.selected.contains(OsStr::from_bytes(b"b.\xff")));
    assert!(!p.selected.contains(OsStr::from_bytes(b"c.\xfe")));
    Ok(())
}

#[test]
fn selection_submenu_keeps_every_panel_action_reachable_at_minimum_size() -> io::Result<()> {
    use dn::{
        menu::{self, Action, MenuId, RootId},
        mouse::Target,
    };
    let f = support::Fixture::new("selection-menu")?;
    fs::write(f.0.join("a.rs"), "")?;
    fs::write(f.0.join("b.rs"), "")?;
    let mut a = f.app()?;
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "a.rs")
        .unwrap();
    menu::open_root(&mut a, RootId::Panel);
    support::draw(&mut a, 80, 25)?;
    let root = menu::root_index(RootId::Panel).unwrap();
    let last = menu::items(root).len() - 1;
    assert!(a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::MenuItem(0, last)));
    let row = menu::items(root)
        .iter()
        .position(|i| i.action == Action::Submenu(MenuId::Selection))
        .unwrap();
    support::click(&mut a, Target::MenuItem(0, row));
    support::draw(&mut a, 80, 25)?;
    let row = menu::submenu_items(MenuId::Selection)
        .iter()
        .position(|i| {
            i.action
                == Action::SelectRelated {
                    extension: true,
                    selecting: true,
                }
        })
        .unwrap();
    support::click(&mut a, Target::MenuItem(1, row));
    assert_eq!(a.panels[0].selected.len(), 2);
    assert!(a.modals.menu().is_none());
    Ok(())
}
#[test]
fn bracket_controller_ignores_strings_when_colors_are_off_and_mac_goto_works() -> io::Result<()> {
    let f = support::Fixture::new("bracket-controller")?;
    let text = "fn main() { let x = \"}\"; /* } */ }";
    fs::write(f.0.join("main.rs"), text)?;
    let mut a = f.app()?;
    a.config.editor.syntax = false;
    editor::open_path(&mut a, f.0.join("main.rs"));
    support::wait(&mut a);
    let pos = text.find('{').unwrap();
    a.desktop
        .active_editor_mut()
        .unwrap()
        .buffer
        .as_mut()
        .unwrap()
        .move_to(pos, false);
    support::modified(&mut a, K::Char(']'), M::CONTROL);
    let b = a.desktop.active_editor().unwrap().buffer.as_ref().unwrap();
    assert_eq!(b.cursor, text.len() - 1);
    assert!(!b.dirty());
    a.show_viewer(Viewer::from_contents(
        "preview".into(),
        viewer::from_bytes(b"one\ntwo".to_vec(), ViewMode::Text),
    ));
    a.config.shortcuts = dn::shortcuts::Profile::Mac;
    support::modified(&mut a, K::Char('o'), M::CONTROL);
    support::key(&mut a, K::Char('g'));
    assert!(a
        .desktop
        .active_viewer()
        .unwrap()
        .modals
        .viewer_goto()
        .is_some());
    Ok(())
}

#[test]
fn sorting_never_fuses_separate_cr_and_lf_blank_lines() {
    let original = "b\ra\n\n\nc";
    let mut b = selected(original, 0, original.len());
    assert!(b.sort_lines().is_err());
    assert_eq!(b.text, original);
    assert!(!b.dirty());
    assert_eq!(b.selected(), 0..original.len());
}
