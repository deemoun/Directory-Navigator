//! Shared desktop/transfer PTY scenarios. No real audio or virtual-console hardware.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::{fs, io};

#[test]
fn copy_move_and_cancel_preserve_captured_sources_in_both_languages() -> io::Result<()> {
    use dn::localization::Language;
    for language in [Language::English, Language::Russian] {
        let preferences = format!("[regional]\nlanguage='{}'\n", language.key());
        let mut s = Session::with_preferences("copy-move-localized", &preferences)?;
        s.wait("manager ready", |s| {
            s.command_prompt_active() && s.text().contains("item-000.")
        })?;
        let expected = format!("captured source {}: Ёжик\n", language.key()).into_bytes();
        fs::write(s.root.join("item-000.txt"), &expected)?;
        s.send(b"\x12", "refresh unique source", |s| {
            s.command_prompt_active()
        })?;
        s.select_file_with_language("item-000.txt", language)?;
        s.send(b"\x1b[17~", "move dialog", |s| {
            s.text().contains(language.text("Rename/move"))
        })?;
        s.send(b"cancelled.txt", "draft move destination", |s| {
            s.text().contains("cancelled.txt")
        })?;
        s.send(b"\x1b", "cancel move", |s| {
            s.command_prompt_active() && !s.text().contains(language.text("Rename/move"))
        })?;
        assert_eq!(fs::read(s.root.join("item-000.txt"))?, expected);
        assert!(!s.root.join("cancelled.txt").exists());

        let completed = |s: &Session| {
            let text = s.text();
            text.contains(language.text("Transfer Center"))
                && text.contains(language.text("Completed"))
                && text.contains(&format!("{}: 0", language.text("Active")))
                && text.contains(&format!("{}: 0", language.text("Queued")))
        };
        s.select_file_with_language("item-000.txt", language)?;
        s.send(b"\x1b[15~", "copy dialog", |s| {
            s.text().contains(language.text("Copy")) && s.text().contains("item-000.txt")
        })?;
        s.send(b"copied.txt\r", "copy completes", |s| {
            completed(s) && fs::read(s.root.join("copied.txt")).is_ok_and(|bytes| bytes == expected)
        })?;
        s.send(b"\x1b", "return after copy", |s| {
            s.command_prompt_active() && !s.text().contains(language.text("Transfer Center"))
        })?;
        s.select_file_with_language("copied.txt", language)?;
        s.send(b"\x1b[17~", "move copied file", |s| {
            s.text().contains(language.text("Rename/move")) && s.text().contains("copied.txt")
        })?;
        s.send(b"moved.txt\r", "move completes", |s| {
            completed(s)
                && !s.root.join("copied.txt").exists()
                && fs::read(s.root.join("moved.txt")).is_ok_and(|bytes| bytes == expected)
        })?;
        s.send(b"\x1b", "return after move", |s| {
            s.command_prompt_active() && !s.text().contains(language.text("Transfer Center"))
        })?;
        assert_eq!(fs::read(s.root.join("item-000.txt"))?, expected);
        assert_eq!(fs::read(s.root.join("moved.txt"))?, expected);
        assert!(!s.root.join("cancelled.txt").exists());
        assert!(!s.root.join("copied.txt").exists());
        s.finish()?;
    }
    Ok(())
}

#[test]
fn system_information_refresh_scroll_resize_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::new("system-diagnostics")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x1buy", "system information", |s| {
        s.text().contains("System Information") && s.text().contains("Architecture")
    })?;
    s.resize(100, 32)?;
    s.send(b"\x1b[F", "last diagnostic rows", |s| {
        s.text().contains("Active directory")
    })?;
    s.send(b"\x1b[15~", "refresh diagnostics", |s| {
        s.text().contains("Application")
            && s.text().contains("Architecture")
            && !s.text().contains("Loading native information")
    })?;
    s.resize(40, 12)?;
    s.resize(80, 25)?;
    s.wait("restored diagnostic window", |s| {
        s.text().contains("System Information") && s.text().contains("Architecture")
    })?;
    s.send(b"\x1b", "close diagnostics", |s| {
        s.command_prompt_active() && !s.text().contains("System Information")
    })?;
    s.finish()
}

#[test]
fn transfer_center_queues_without_blocking_editing_and_restores_terminal() -> io::Result<()> {
    let mut s = Session::new("transfer-center")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active()
            && s.text().contains("item-000.")
            && !s.text().contains("Reading directory")
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1b[15~", "copy form", |s| s.text().contains("Copy file"))?;
    s.send(b"\x1br", "choose explicit Refuse policy", |s| {
        s.text().contains("Refuse existing destinations")
    })?;
    s.send(b"item-001.txt\r", "refused conflict pauses queue", |s| {
        s.text().contains("Transfer Center")
            && s.text().contains("Failed")
            && s.text().contains("Queue paused")
    })?;
    assert_eq!(fs::read(s.root.join("item-001.txt"))?, b"fixture\n");
    for (source, dest) in [
        ("item-000.txt", "transfer-one.txt"),
        ("item-002.txt", "transfer-two.txt"),
    ] {
        s.send(b"\x1b", "hide center without clearing results", |s| {
            s.command_prompt_active() && !s.text().contains("Transfer Center")
        })?;
        s.select_file(source)?;
        s.send(b"\x1b[15~", "queue copy form", |s| {
            s.text().contains("Copy file")
        })?;
        s.send(format!("{dest}\r").as_bytes(), "queued request", |s| {
            s.text().contains("Transfer Center")
                && s.text().contains("Queue paused")
                && s.text().contains(dest)
        })?;
        assert!(!s.root.join(dest).exists());
    }
    s.send(b"\x1b[17;3~", "switch to manager", |s| {
        s.command_prompt_active()
    })?;
    // The last queued request captured item-002; its manager selection remains
    // intact while the floating center still covers part of the background.
    s.send(b"\x1b[14~", "editor while transfers wait", |s| {
        s.text().contains("F2 Save") && s.text().contains("fixture")
    })?;
    s.send("draft 界 ".as_bytes(), "independent editor draft", |s| {
        s.text().contains("draft 界")
    })?;
    s.send(b"\x1b[17;4~", "switch back to center", |s| {
        s.text().contains("Transfer Center") && !s.text().contains("F2 Save")
    })?;
    s.send(b"\t\t\r", "resume queued copies", |s| {
        s.root.join("transfer-one.txt").exists()
            && s.root.join("transfer-two.txt").exists()
            && s.text().contains("Completed")
            && !s.text().contains("Queue paused")
    })?;
    s.send(b"\x1bOP", "owned transfer Help", |s| {
        s.text().contains("Transfer Center Help")
    })?;
    s.send(b"\x1b", "return from transfer Help", |s| {
        s.text().contains("Transfer Center") && !s.text().contains("Transfer Center Help")
    })?;
    s.resize(100, 32)?;
    s.wait("resized center", |s| {
        s.text().contains("Transfer Center") && s.text().contains("Completed")
    })?;
    s.send(b"\x1b[17;3~", "return to retained editor", |s| {
        s.text().contains("F2 Save") && s.text().contains("draft 界")
    })?;
    s.send(b"\x1b", "dirty close guard", |s| {
        s.text().contains("Discard")
    })?;
    s.send(b"d", "discard fixture draft", |s| {
        !s.text().contains("F2 Save") && !s.text().contains("Discard")
    })?;
    if s.text().contains("Transfer Center") {
        s.send(b"\x1b", "close view", |s| s.command_prompt_active())?;
    }
    s.send(b"\x1bmf", "reopen session results from Manager menu", |s| {
        s.text().contains("Transfer Center") && s.text().contains("Completed")
    })?;
    s.send(b"\x1b", "close session results", |s| {
        s.command_prompt_active()
    })?;
    for dest in ["transfer-one.txt", "transfer-two.txt", "item-002.txt"] {
        assert_eq!(fs::read(s.root.join(dest))?, b"fixture\n");
    }
    s.finish()
}

#[test]
fn settings_and_unicode_prompt_workflow_restores_terminal_and_preserves_cancelled_drafts(
) -> io::Result<()> {
    let mut s = Session::new("controllers")?;
    s.wait("manager ready", |s| {
        s.text().contains("item-000.")
            && s.text().contains("F5 Copy")
            && !s.text().contains("Reading directory")
            && s.command_prompt_active()
    })?;
    let config = fs::read(s.root.join("settings/dn/config.conf"))?;
    s.send(b"\x1boci", "interface settings", |s| {
        s.text().contains("Interface Setup")
    })?;
    // Tab and reverse Tab end on the same control before cancellation.
    s.send(b" \t\x1b[Z", "settings traversal", |s| {
        s.text().contains("Interface Setup")
    })?;
    s.send(b"\x1b", "settings cancelled", |s| {
        !s.text().contains("Interface Setup")
    })?;
    assert_eq!(fs::read(s.root.join("settings/dn/config.conf"))?, config);
    s.send(b"\x1b[18~", "mkdir opened", |s| {
        s.text().contains("Make directory")
    })?;
    s.send(
        "\x1b[200~cancelled-界\x1b[201~".as_bytes(),
        "Unicode paste",
        |s| s.text().contains("cancelled-界"),
    )?;
    s.send(b"\x1b[15;5~", "dialog movement", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[C\x1b", "movement rollback", |s| {
        !s.text().contains("Arrows move")
    })?;
    assert!(s.text().contains("cancelled-界") && s.text().contains("Make directory"));
    s.send(b"\x1b", "mkdir cancelled", |s| {
        !s.text().contains("Make directory")
    })?;
    assert!(!s.root.join("cancelled-界").exists());
    s.send(b"\x1b[18~", "mkdir reopened", |s| {
        s.text().contains("Make directory")
    })?;
    s.send("created-界".as_bytes(), "Unicode input", |s| {
        s.text().contains("created-界")
    })?;
    s.send(b"\r", "mkdir submitted", |s| {
        s.root.join("created-界").is_dir() && !s.text().contains("Make directory")
    })?;
    assert_eq!(fs::read(s.root.join("settings/dn/config.conf"))?, config);
    s.finish()
}

#[test]
fn overlapping_documents_native_dialogs_game_pages_resize_and_output_handoff() -> io::Result<()> {
    let mut s = Session::new("desktop")?;
    s.wait("manager ready", |s| {
        s.text().contains("item-000.")
            && s.text().contains("F5 Copy")
            && !s.text().contains("Reading directory")
            && s.command_prompt_active()
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1b[14~", "editor opened", |s| {
        s.text().contains("F2 Save")
    })?;
    s.send(b"\x1bz", "editor restored", |s| {
        s.text().contains("F5 Copy")
    })?;
    s.send(b"\x1b[15;5~", "move editor", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(
        b"\x1b[200~SHOULD_NOT_EDIT\x1b[201~\x1b[C\x1b[B\r",
        "movement accepted",
        |s| !s.text().contains("Arrows move"),
    )?;
    assert!(!s.text().contains("SHOULD_NOT_EDIT"));
    s.send(b"\x1b[<0;1;2M\x1b[<0;1;2m", "exposed manager", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1b[13~", "viewer opened", |s| {
        s.text().contains("Viewer")
    })?;
    s.send(
        b"\x1b[<0;78;1M\x1b[<32;77;5M\x1b[<0;77;5m",
        "viewer title drag",
        |s| s.text().contains("F5 Copy"),
    )?;
    s.send(b"\x1b[15;5~", "move viewer", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[H\x1b[5~\r", "align viewer", |s| {
        !s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[<0;75;9M\x1b[<0;75;9m", "raise editor", |s| {
        s.text().contains("F2 Save")
    })?;
    s.send(b"\x1b[13~", "native file dialog", |s| {
        s.text().contains("Open file")
    })?;
    s.send(b"\x1b[15;5~", "move native dialog", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[D\r", "accept native dialog position", |s| {
        !s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b", "cancel native dialog", |s| {
        !s.text().contains("Open file")
    })?;
    s.send(b"\x1b", "close editor", |s| !s.text().contains("F2 Save"))?;
    // The function bar is also visible behind a floating viewer. Confirm
    // manager ownership through its command cursor before sending Alt+F9.
    s.send(b"\x1b[<0;79;22M\x1b[<0;79;22m", "manager focused", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1b[20;3~", "game opened", |s| {
        s.text().contains("Navigator's game")
    })?;
    s.send(b"\x1b[15~", "game setup", |s| {
        s.text().contains("Setup game")
    })?;
    s.send(b"\x1b[15;5~", "move game page", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[D\r", "page movement accepted", |s| {
        !s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b", "cancel game page", |s| {
        !s.text().contains("Setup game")
    })?;
    s.send(b"\x1b", "close game", |s| {
        !s.text().contains("Navigator's game") && s.command_prompt_active()
    })?;
    s.send(b"\x1bOP", "native help", |s| {
        s.text().contains("Directory Navigator — Help")
    })?;
    s.send(b"\x1b[15;5~", "resize help", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[1;2C\r", "accept help size", |s| {
        !s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b", "close help", |s| {
        !s.text().contains("Directory Navigator — Help")
    })?;
    let exits = s.occurrences(b"\x1b[?1049l");
    s.send_output(b"\x0f", "output handoff", |s| {
        s.occurrences(b"\x1b[?1049l") > exits
    })?;
    let enters = s.occurrences(b"\x1b[?1049h");
    s.send(b"\r", "return to desktop", |s| {
        s.occurrences(b"\x1b[?1049h") > enters && s.text().contains("F5 Copy")
    })?;
    s.resize(100, 32)?;
    s.wait("resize redraw", |s| s.command_prompt_active())?;
    for n in 0..65 {
        assert_eq!(
            fs::read(s.root.join(format!("item-{n:03}.txt")))?,
            b"fixture\n"
        );
    }
    s.finish()
}

#[test]
fn colors_window_edits_cancel_saves_moves_and_restores_terminal() -> io::Result<()> {
    use dn::palette::theme::Slot;
    let mut s = Session::new("colors")?;
    s.wait("manager ready", |s| {
        s.text().contains("item-000.")
            && s.text().contains("F5 Copy")
            && !s.text().contains("Reading directory")
            && s.command_prompt_active()
    })?;
    let config = s.root.join("settings/dn/config.conf");
    let original = fs::read(&config)?;
    s.send(b"\x1boo", "colors opened", |s| {
        s.text().contains("Foreground") && s.text().contains("Background")
    })?;
    s.send(b"f\x1b[C\x1b", "color preview cancelled", |s| {
        !s.text().contains("Foreground")
    })?;
    assert_eq!(fs::read(&config)?, original);
    s.send(b"\x1boo", "colors reopened", |s| {
        s.text().contains("Foreground")
    })?;
    s.send(b"f\x1b[C\r", "color accepted", |s| {
        !s.text().contains("Foreground")
    })?;
    s.send(b"\x1bos", "save colors", |_| {
        dn::config::Config::read(&config).is_ok_and(|c| c.palette.slot(Slot::TimerColor) == 0x72)
    })?;
    s.send(b"\x1boo", "saved colors reopened", |s| {
        s.text().contains("Foreground")
    })?;
    s.send(b"c", "preview default colors", |s| {
        s.text().contains("[X] Default colors set")
    })?;
    s.resize(100, 32)?;
    // A queued pre-resize frame can still contain the title. Wait for the
    // function bar on the new bottom row before starting manipulation; resize
    // deliberately cancels any capture begun against the old viewport.
    s.wait("resized colors", |s| {
        let text = s.text();
        text.contains("Foreground")
            && text
                .lines()
                .last()
                .is_some_and(|row| row.contains("F10 Quit"))
    })?;
    s.send(b"\x1b[15;5~", "move colors", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[C\r", "accept color window position", |s| {
        !s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b", "cancel default preview", |s| {
        !s.text().contains("Foreground")
    })?;
    assert_eq!(
        dn::config::Config::read(&config)?
            .palette
            .slot(Slot::TimerColor),
        0x72
    );
    s.finish()
}

#[test]
fn tile_and_cascade_from_window_list_preserve_documents_and_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("arrangement")?;
    s.wait("manager ready", |s| {
        s.text().contains("item-000.")
            && s.text().contains("F5 Copy")
            && !s.text().contains("Reading directory")
            && s.command_prompt_active()
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1b[14~", "editor opened", |s| {
        s.text().contains("F2 Save")
    })?;
    s.send(b"\x1bz", "editor restored", |s| {
        s.text().contains("F5 Copy")
    })?;
    s.send(b"\x1b[<0;1;2M\x1b[<0;1;2m", "manager focused", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1b[13~", "viewer opened", |s| {
        s.text().contains("Viewer")
    })?;
    s.send(b"\x1b0", "window list", |s| {
        s.text().contains("Open windows") && s.text().contains("Cascade")
    })?;
    // Home selects the manager row, disabling Size/Move and Zoom. Tab visits
    // Switch, Tile, then Cascade without activating that selected manager row.
    s.send(b"\x1b[H\t\t\r", "tile accepted", |s| {
        !s.text().contains("Open windows")
            && s.text().contains("F2 Save")
            && s.text().contains("F2 Wrap")
    })?;
    s.send(b"\x1b0", "window list again", |s| {
        s.text().contains("Open windows")
    })?;
    s.send(b"\x1b[H\t\t\t\r", "cascade accepted", |s| {
        !s.text().contains("Open windows") && s.text().contains("F2 Wrap")
    })?;
    for n in 0..65 {
        assert_eq!(
            fs::read(s.root.join(format!("item-{n:03}.txt")))?,
            b"fixture\n"
        );
    }
    s.finish()
}

#[test]
fn game_and_editor_arrangements_are_visible_and_game_board_keeps_its_size() -> io::Result<()> {
    let mut s = Session::new("arrange-game")?;
    s.wait("manager ready", |s| s.text().contains("F5 Copy"))?;
    s.resize(160, 40)?;
    s.wait("large desktop ready", |s| {
        s.text()
            .lines()
            .last()
            .is_some_and(|r| r.contains("F10 Quit"))
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1b[14~", "editor opened", |s| {
        s.text().contains("F2 Save")
    })?;
    s.send(b"\x1bz", "editor restored", |s| {
        s.text().contains("F5 Copy")
    })?;
    s.send(b"\x1b[<0;1;2M\x1b[<0;1;2m", "manager focused", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1b[20;3~", "game opened", |s| {
        s.text().contains("Navigator's game")
    })?;
    s.send(b"\x1b0", "game window list", |s| {
        s.text().contains("Open windows")
    })?;
    s.send(b"\x1b[H\t\t\r", "editor and game tiled", |s| {
        let text = s.text();
        !text.contains("Open windows")
            && text.contains("Navigator's game")
            && text.contains("F2 Save")
            && text
                .lines()
                .nth(1)
                .is_some_and(|r| r.starts_with('╔') && r.chars().nth(80) == Some('╔'))
    })?;
    s.send(b"\x1b0", "game list again", |s| {
        s.text().contains("Open windows")
    })?;
    s.send(b"\x1b[H\t\t\t\r", "editor and game cascaded", |s| {
        let text = s.text();
        !text.contains("Open windows")
            && text.contains("Navigator's game")
            && text
                .lines()
                .nth(2)
                .is_some_and(|r| r.chars().nth(1) == Some('╔') && r.chars().nth(54) == Some('╗'))
    })?;
    s.send(b"\x1b[15~", "arranged game setup", |s| {
        s.text().contains("Setup game")
    })?;
    s.send(b"\x1b", "cancel game setup", |s| {
        !s.text().contains("Setup game")
    })?;
    for n in 0..65 {
        assert_eq!(
            fs::read(s.root.join(format!("item-{n:03}.txt")))?,
            b"fixture\n"
        );
    }
    s.finish()
}

#[test]
fn held_buttons_shift_and_release_outside_cancels_in_a_real_terminal() -> io::Result<()> {
    let mut s = Session::new("button-feedback")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active()
            && s.text().contains("item-000.")
            && s.text().contains("F5 Copy")
            && !s.text().contains("Reading directory")
    })?;
    s.send(b"\x1b[18~", "mkdir opened", |s| {
        s.text().contains("Make directory")
    })?;
    fn cancel_position(text: &str) -> Option<(usize, usize)> {
        text.lines().enumerate().find_map(|(y, line)| {
            if !line.contains("OK") || !line.contains("Help") {
                return None;
            }
            let at = line.find("Cancel")?;
            Some((line[..at].chars().count(), y))
        })
    }
    let (x, y) = cancel_position(&s.text()).expect("Cancel button");
    let down = format!("\x1b[<0;{};{}M", x + 1, y + 1);
    s.send(down.as_bytes(), "button held", |s| {
        cancel_position(&s.text()) == Some((x + 1, y))
    })?;
    assert!(s.text().contains("Make directory"));
    s.send(b"\x1b[<32;1;1M", "pointer outside", |s| {
        cancel_position(&s.text()) == Some((x, y))
    })?;
    s.send(b"\x1b[<0;1;1m", "outside release", |s| {
        s.text().contains("Make directory")
    })?;
    s.send(down.as_bytes(), "button pressed again", |s| {
        cancel_position(&s.text()) == Some((x + 1, y))
    })?;
    let up = format!("\x1b[<0;{};{}m", x + 1, y + 1);
    s.send(up.as_bytes(), "inside release", |s| {
        !s.text().contains("Make directory") && s.command_prompt_active()
    })?;
    s.finish()
}

#[test]
fn tree_and_calculator_share_menus_switching_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::new("tool-windows")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active()
            && s.text().contains("item-000.")
            && s.text().contains("F5 Copy")
            && !s.text().contains("Reading directory")
    })?;
    s.send(b"\x14", "tree document", |s| {
        s.text().contains("Directory tree")
    })?;
    s.send(b"\x1b[20~", "File menu above tree", |s| {
        s.text().contains("Copy") && s.text().contains("Rename")
    })?;
    s.send(b"\x1b", "tree menu closed", |s| {
        s.text().contains("Directory tree")
    })?;
    s.send(b"\x1buc", "Calculator document", |s| {
        s.text().contains("Calculator")
    })?;
    s.send(b"\x1b[17;3~", "cycle to manager", |s| {
        // A short native path puts the command cursor behind the floating tree.
        // The cursor is correctly hidden; document activation is authoritative.
        s.manager_document_ready()
    })?;
    assert!(s.text().contains("Directory tree") && s.text().contains("Calculator"));
    s.send(b"\x1b[17;3~", "cycle to tree", |s| {
        !s.command_prompt_active() && s.text().contains("Directory tree")
    })?;
    s.send(b"\x1b[20~", "tree menu after switching", |s| {
        s.text().contains("Rename")
    })?;
    s.send(b"\x1b", "close menu", |s| {
        s.text().contains("Directory tree")
    })?;
    s.send(b"\x1b0", "window list with tools", |s| {
        s.text().contains("Open windows")
            && s.text().contains("Calculator")
            && s.text().contains("Directory tree")
    })?;
    s.send(b"\x1b", "close window list", |s| {
        !s.text().contains("Switching keeps")
    })?;
    s.resize(100, 32)?;
    s.wait("tool resize", |s| s.text().contains("Directory tree"))?;
    s.finish()
}

#[test]
fn advanced_find_highlight_drafts_and_parameter_review_work_in_terminal() -> io::Result<()> {
    let mut s = Session::new("dn-workflows")?;
    fs::write(
        s.root.join("dn.mnu"),
        ">1 ~P~arameter test\n? value 'Value' 'start'\nprintf '%s' '{param:value}'\n",
    )?;
    s.wait("manager ready", |s| {
        s.command_prompt_active()
            && s.text().contains("item-000.")
            && !s.text().contains("Reading directory")
    })?;
    let config = fs::read(s.root.join("settings/dn/config.conf"))?;
    s.send(b"\x1b[18;5~", "advanced Find", |s| {
        s.text().contains("Containing text") && s.text().contains("Entry type")
    })?;
    s.send(b"item-000.txt\tfixture", "find criteria", |s| {
        s.text().contains("item-000.txt") && s.text().contains("fixture")
    })?;
    s.send(b"\r", "content result", |s| {
        s.text().contains("Find results") && s.text().contains("item-000.txt:1")
    })?;
    s.send(b"\x1b", "close results", |s| s.command_prompt_active())?;
    s.send(b"\x1bofh", "highlight groups", |s| {
        s.text().contains("Highlight Groups") && s.text().contains("Filename masks")
    })?;
    s.send(b"\t \t;*.custom", "group draft", |s| {
        s.text().contains("*.custom")
    })?;
    // Confirm the final color action, rather than an intermediate mask frame,
    // before queuing a terminal resize and Esc.
    s.send(b"\t\x1b[C", "group color", |s| {
        s.text()
            .lines()
            .any(|line| line.contains("*.custom") && line.contains("09"))
    })?;
    s.resize(100, 32)?;
    s.wait("highlight resize", |s| s.text().contains("*.custom"))?;
    s.send(b"\x1b", "cancel group changes", |s| {
        s.command_prompt_active()
    })?;
    assert_eq!(fs::read(s.root.join("settings/dn/config.conf"))?, config);
    s.send(b"\x1bOQ", "user menu", |s| {
        s.text().contains("Parameter test")
    })?;
    s.send(b"\r", "parameter dialog", |s| {
        s.text().contains("User command parameters")
    })?;
    s.send(
        "\x1b[200~界 value\x1b[201~".as_bytes(),
        "parameter Unicode",
        |s| s.text().contains("界 value"),
    )?;
    s.send(b"\r", "review before Run", |s| {
        s.text().contains("Run user command") && s.text().contains("Program:")
    })?;
    let exits = s.occurrences(b"\x1b[?1049l");
    s.send(b"\x1b", "cancel command review", |s| {
        s.text().contains("User menu") && !s.text().contains("Run user command")
    })?;
    assert_eq!(s.occurrences(b"\x1b[?1049l"), exits);
    s.send(b"\x1b", "close menu", |s| s.command_prompt_active())?;
    s.finish()
}

#[test]
fn help_has_one_working_expand_restore_control_in_a_real_terminal() -> io::Result<()> {
    fn zoom(s: &Session, icon: &str) -> (usize, usize) {
        let wanted = icon.chars().collect::<Vec<_>>();
        let hits = s
            .text()
            .lines()
            .enumerate()
            .flat_map(|(y, line)| {
                line.chars()
                    .collect::<Vec<_>>()
                    .windows(3)
                    .enumerate()
                    .filter_map(|(x, g)| (g == wanted.as_slice()).then_some((x + 2, y + 1)))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(hits.len(), 1, "one visible {icon}");
        hits[0]
    }
    let mut s = Session::new("help-zoom")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active()
            && s.text().contains("item-000.")
            && !s.text().contains("Reading directory")
    })?;
    s.send(b"\x1bOP", "Help opened", |s| s.text().contains("[↑]"))?;
    for _ in 0..2 {
        let (x, y) = zoom(&s, "[↑]");
        let click = format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m");
        s.send(click.as_bytes(), "Help expanded", |s| {
            s.text().contains("[↕]") && !s.text().contains("[▲]")
        })?;
        let (x, y) = zoom(&s, "[↕]");
        let click = format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m");
        s.send(click.as_bytes(), "Help restored", |s| {
            s.text().contains("[↑]") && !s.text().contains("[↕]")
        })?;
    }
    s.send(b"\x1b", "close Help", |s| s.command_prompt_active())?;
    s.finish()
}

#[test]
fn quick_directories_stays_in_place_during_idle_and_focus_changes() -> io::Result<()> {
    let mut s = Session::new("quick-directories-stable")?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.send(b"\x1bpq\r", "quick directories opened", |s| {
        s.text().contains("<empty>")
    })?;
    s.send(b"\t\t\r", "one bookmark assigned", |s| {
        s.text().matches("<empty>").count() == 8
    })?;
    fn positions(s: &Session) -> Vec<usize> {
        s.text()
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                (line.contains("Quick directories")
                    || line.contains("<empty>")
                    || line.contains("1  ")
                    || line.contains("Set Current")
                    || line.contains("Ctrl+1–9 set"))
                .then_some(index)
            })
            .collect()
    }
    let before = positions(&s);
    assert_eq!(
        before.len(),
        12,
        "title, nine slots, buttons and hint\n{}",
        s.text()
    );
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    s.wait("idle redraws", |_| std::time::Instant::now() >= until)?;
    assert_eq!(positions(&s), before);
    for _ in 0..12 {
        s.send(b"\t", "bookmark focus changed", |s| {
            s.text().contains("Quick directories")
        })?;
        assert_eq!(positions(&s), before);
    }
    s.send(b"\x1b", "quick directories closed", |s| {
        !s.text().contains("Quick directories")
    })?;
    s.finish()
}

#[test]
fn calculator_green_title_close_works_after_movement() -> io::Result<()> {
    let mut s = Session::new("calculator-title-close")?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.send(b"\x1buc", "calculator opened", |s| {
        s.text().contains("Calculator")
    })?;
    s.send(b"2+3\r", "calculator evaluated", |s| {
        s.text().contains("2+3")
    })?;
    let title_position = |s: &Session| {
        s.text().lines().enumerate().find_map(|(y, line)| {
            line.contains("Calculator")
                .then(|| (line.chars().position(|c| c == '■').unwrap(), y))
        })
    };
    let (close_x, title_y) = title_position(&s).unwrap();
    let (x, y) = (close_x + 9, title_y + 1);
    let drag = format!(
        "\x1b[<0;{x};{y}M\x1b[<32;{};{}M\x1b[<0;{};{}m",
        x + 1,
        y + 1,
        x + 1,
        y + 1
    );
    s.send(drag.as_bytes(), "calculator dragged by title", |s| {
        title_position(s) == Some((close_x + 1, title_y + 1)) && !s.text().contains("Arrows move")
    })?;
    let text = s.text();
    let (y, line) = text
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Calculator"))
        .unwrap();
    let x = line
        .chars()
        .position(|c| c == '■')
        .expect("Green title close glyph");
    let click = format!("\x1b[<0;{};{}M\x1b[<0;{};{}m", x + 1, y + 1, x + 1, y + 1);
    s.send(click.as_bytes(), "calculator closed by title", |s| {
        !s.text().contains("Calculator") && s.command_prompt_active()
    })?;
    s.finish()
}

#[test]
fn quick_directories_go_navigates_and_closes_in_the_release_ui() -> io::Result<()> {
    let mut s = Session::new("bookmark-go-close")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    // The inactive pane can paint its filenames before the active scan finishes.
    // Resolve the fixture by name through the shared model-readiness boundary.
    s.select_file("inside")?;
    s.send(b"\r", "fixture directory opened", |s| {
        s.text().contains("child.txt") && s.command_prompt_active()
    })?;
    s.send(b"\x1bpq\r", "bookmarks opened", |s| {
        s.text().contains("<empty>")
    })?;
    s.send(b"\t\t\r", "bookmark assigned", |s| {
        s.text().matches("<empty>").count() == 8
    })?;
    s.send(b"\x1b", "bookmarks closed", |s| {
        !s.text().contains("Quick directories")
    })?;
    s.send(b"\x7f", "parent directory", |s| {
        s.text().contains("item-000.") && s.command_prompt_active()
    })?;
    s.send(b"\x1bpq\r", "bookmarks reopened", |s| {
        s.text().contains("Quick directories") && s.text().contains("<empty>")
    })?;
    s.send(b"\r", "Go To navigated and closed bookmarks", |s| {
        !s.text().contains("Quick directories")
            && s.text().contains("child.txt")
            && s.command_prompt_active()
    })?;
    s.finish()
}

#[test]
fn welcome_expand_restore_resize_and_start_preserve_terminal() -> io::Result<()> {
    let mut s = Session::new("welcome-zoom-resize")?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.send(b"\x1b[<0;3;1M\x1b[<0;3;1mw", "Welcome opened", |s| {
        s.text().contains("Welcome to Directory Navigator") && s.text().contains("[↑]")
    })?;
    fn zoom(s: &Session, icon: &str) -> (usize, usize) {
        let text = s.text();
        let (y, line) = text
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains(icon))
            .expect("Welcome zoom control");
        (line[..line.find(icon).unwrap()].chars().count() + 2, y + 1)
    }
    let (x, y) = zoom(&s, "[↑]");
    let click = format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m");
    s.send(click.as_bytes(), "Welcome expanded", |s| {
        s.text().contains("[↕]")
    })?;
    s.send(b"\x1bz", "Welcome restored", |s| {
        s.text().contains("[↑]") && !s.text().contains("[↕]")
    })?;
    s.send(b"\x1b[15;5~", "Welcome Size/Move", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[1;2C\r", "Welcome resized", |s| {
        !s.text().contains("Arrows move") && s.text().contains("Welcome to Directory Navigator")
    })?;
    s.send(b"\r", "Welcome Start", |s| s.command_prompt_active())?;
    s.finish()
}

#[test]
fn desktop_menu_above_multiple_documents_opens_on_first_click() -> io::Result<()> {
    fn menu_visible(s: &Session) -> bool {
        s.text()
            .lines()
            .next()
            .is_some_and(|line| line.contains("Utilities"))
    }
    let mut s = Session::new("floating-desktop-menu")?;
    s.wait("manager menu ready", |s| {
        s.command_prompt_active() && menu_visible(s)
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1b[14~", "fullscreen editor", |s| {
        s.text().contains("fixture") && !menu_visible(s)
    })?;
    s.send(
        b"\x1bz",
        "floating editor retains desktop menu",
        menu_visible,
    )?;
    s.send(
        b"\x1bq",
        "SmartPad above another editor retains desktop menu",
        |s| s.text().contains("SmartPad") && menu_visible(s),
    )?;
    let before = s.text().lines().next().unwrap().to_owned();
    s.wait("idle redraw retains desktop menu", |s| {
        menu_visible(s) && s.text().lines().next().is_some_and(|line| line != before)
    })?;
    s.send(
        b"\x1b[<0;2;1M\x1b[<0;2;1m",
        "desktop menu opens on first click",
        |s| s.text().contains("About...") && s.text().contains("Screensaver"),
    )?;
    s.send(b"\x1b", "menu closes back to SmartPad", |s| {
        !s.text().contains("About...") && menu_visible(s) && !s.command_prompt_active()
    })?;
    s.send(b"\x1bz", "fullscreen SmartPad owns its menu", |s| {
        !menu_visible(s)
    })?;
    s.send(b"\x1bz", "restore desktop menu", menu_visible)?;
    s.send(b"\x1bq", "return focus to editor", |s| {
        s.text().contains("fixture") && menu_visible(s)
    })?;
    s.send(b"\x1bq", "return focus to SmartPad", |s| {
        s.text().contains("SmartPad") && menu_visible(s)
    })?;
    s.send(b"\x1b", "close empty SmartPad", |s| {
        !s.text().contains("SmartPad") && menu_visible(s)
    })?;
    s.send(b"\x1b", "close unmodified editor", |s| {
        s.command_prompt_active() && menu_visible(s)
    })?;
    s.finish()
}

#[test]
fn temp_drive_real_copy_cancel_clear_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::with_preferences("temp-drive", "\n[file_manager]\nuse_trash=false\n")?;
    let root = s.root.join("settings/dn/temp-drive");
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1bft", "quick copy targets Temp Drive", |s| {
        s.text().contains("Copy file") && s.text().contains("temp-drive")
    })?;
    s.send(b"\x1b", "cancel quick copy", |s| s.command_prompt_active())?;
    assert!(fs::read_dir(&root)?.next().is_none());
    s.send(b"\x1bft", "reopen quick copy", |s| {
        s.text().contains("Copy file")
    })?;
    s.send(b"\r", "copy finishes in Transfer Center", |s| {
        s.text().contains("Transfer Center") && s.text().contains("Completed")
    })?;
    assert_eq!(fs::read(root.join("item-000.txt"))?, b"fixture\n");
    assert_eq!(fs::read(s.root.join("item-000.txt"))?, b"fixture\n");
    s.send(b"\x1b", "hide transfers", |s| s.command_prompt_active())?;
    s.send(b"\t\x1but", "open Temp Drive in right panel", |s| {
        s.command_prompt_active()
            && s.text().contains("temp-drive")
            && s.text().contains("item-000.")
    })?;
    s.send(b"\t", "return to source panel", |s| {
        s.command_prompt_active()
    })?;
    s.select_file("item-001.txt")?;
    s.send(b"\x1b[15~", "ordinary F5 targets Temp Drive", |s| {
        s.text().contains("Copy file")
    })?;
    s.send(b"\r", "ordinary copy finishes", |s| {
        s.text().contains("Completed") && root.join("item-001.txt").exists()
    })?;
    s.send(b"\x1b", "hide ordinary transfer", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1bud", "clear confirmation", |s| {
        s.text().contains("Clear Temp Drive") && s.text().contains("This cannot be undone.")
    })?;
    s.send(b"\r", "Enter cancels by default", |s| {
        s.command_prompt_active()
    })?;
    assert!(root.join("item-000.txt").exists());
    assert!(root.join("item-001.txt").exists());
    s.send(b"\x1bud", "reopen clear confirmation", |s| {
        s.text().contains("Clear Temp Drive")
    })?;
    s.send(b"\x1b[Z\r", "confirm clear", |s| {
        s.command_prompt_active()
            && fs::read_dir(&root).is_ok_and(|mut entries| entries.next().is_none())
    })?;
    assert!(root.is_dir());
    assert_eq!(fs::read(s.root.join("item-000.txt"))?, b"fixture\n");
    assert_eq!(fs::read(s.root.join("item-001.txt"))?, b"fixture\n");
    s.send(b"\x1bud", "empty Temp Drive", |s| {
        s.text().contains("Temp Drive is empty.")
    })?;
    s.send(b"\r", "acknowledge empty Temp Drive", |s| {
        s.command_prompt_active()
    })?;
    s.finish()
}

#[test]
fn archive_pane_selected_extraction_and_return_restore_terminal() -> io::Result<()> {
    use std::io::Write;
    let mut s = Session::new("archive-pane-extraction")?;
    let path = s.root.join("fixture.zip");
    let mut archive = zip::ZipWriter::new(fs::File::create(&path)?);
    for (name, text) in [
        ("docs/a.txt", b"archive alpha\n".as_slice()),
        ("docs/b.txt", b"archive beta\n"),
    ] {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(text)?;
    }
    archive.add_directory("empty/", zip::write::SimpleFileOptions::default())?;
    archive.finish()?;
    s.wait("initial manager", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x12", "refresh fixture", |s| {
        s.command_prompt_active() && s.text().contains("fixture.zip")
    })?;
    s.select_file("fixture.zip")?;
    s.send(b"\r", "archive opens in pane", |s| {
        s.command_prompt_active() && s.text().contains(" :: /") && s.text().contains("docs")
    })?;
    s.select_file("docs")?;
    s.send(b"\r", "internal folder", |s| {
        s.command_prompt_active() && s.text().contains(" :: /docs") && s.text().contains("a.txt")
    })?;
    s.select_file("a.txt")?;
    s.send(b"\x1b[13~", "member viewer", |s| {
        s.text().contains("archive alpha")
    })?;
    s.send(b"\x1b", "return to archive pane", |s| {
        s.command_prompt_active() && s.text().contains(" :: /docs")
    })?;
    s.select_file("a.txt")?;
    s.send(b" ", "mark first member", |s| {
        s.text().contains("selected file")
    })?;
    s.send(b" ", "mark second member", |s| {
        s.text().contains("selected files")
    })?;
    s.send(b"\x1b[15~", "selected extraction dialog", |s| {
        s.text().contains("Extract 2 selected archive")
    })?;
    s.send(b"\r", "extraction completes", |s| {
        s.text().contains("Transfer Center")
            && s.text().contains("Completed")
            && s.root.join("a.txt").exists()
            && s.root.join("b.txt").exists()
    })?;
    assert_eq!(fs::read(s.root.join("a.txt"))?, b"archive alpha\n");
    assert_eq!(fs::read(s.root.join("b.txt"))?, b"archive beta\n");
    s.send(b"\x1b", "close transfer center", |s| {
        s.command_prompt_active() && s.text().contains(" :: /docs")
    })?;
    s.resize(120, 35)?;
    s.wait("archive resize", |s| {
        fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
            .is_ok_and(|d| d.contains("viewport=(120, 35)"))
    })?;
    s.send(b"\x1b[5;5~", "exit archive", |s| {
        s.command_prompt_active()
            && !s.text().contains(" :: /docs")
            && s.text().contains("fixture.zip")
    })?;
    s.finish()
}

#[test]
fn larger_files_and_confirmed_replacement_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("large-files-overwrite")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    let text = "large-text-fixture abcdefghijklmnopqrstuvwxyz 0123456789\n".repeat(60_000);
    assert!(text.len() > 2 * 1024 * 1024);
    fs::write(s.root.join("large.txt"), &text)?;
    fs::write(s.root.join("item-001.txt"), b"old destination")?;
    s.send(b"\x12", "refresh large fixture", |s| {
        s.command_prompt_active()
    })?;
    s.select_file("large.txt")?;
    s.send(b"\x1b[13~", "large viewer", |s| {
        s.text().contains("F3 Viewer") && s.text().contains("large-text-fixture")
    })?;
    s.resize(100, 32)?;
    s.wait("resized large viewer", |s| {
        s.text().contains("large-text-fixture")
    })?;
    s.resize(80, 25)?;
    s.send(b"\x1b", "close large viewer", |s| s.command_prompt_active())?;
    s.send(b"\x1b[14~", "large editor", |s| {
        s.text().contains("F2 Save") && s.text().contains("large-text-fixture")
    })?;
    s.send(b"\x1b", "close unchanged large editor", |s| {
        s.command_prompt_active() && !s.text().contains("F2 Save")
    })?;
    assert_eq!(fs::read_to_string(s.root.join("large.txt"))?, text);
    for replace in [false, true] {
        // Completing a transfer starts a panel refresh. Wait for its published
        // readiness before opening the next Copy form, then capture the source.
        s.select_file("item-000.txt")?;
        s.send(b"\x1b[15~", "copy form", |s| {
            s.text().contains("Ask before replacing")
        })?;
        s.send(b"item-001.txt\r", "overwrite decision", |s| {
            s.text().contains("Replace existing file?") && s.text().contains("Replace all")
        })?;
        if replace {
            s.send(b"\x1b[Z", "focus Replace", |s| {
                s.text().contains("Replace existing file?")
            })?;
        }
        s.send(b"\r", "transfer completed", |s| {
            s.text().contains("Transfer Center")
                && s.text().contains("Completed")
                && s.text().contains("Active: 0")
                && s.text().contains("Queued: 0")
                && !s.text().contains("Replace existing file?")
                && (!replace
                    || fs::read(s.root.join("item-001.txt"))
                        .is_ok_and(|bytes| bytes == b"fixture\n"))
        })?;
        assert_eq!(
            fs::read(s.root.join("item-001.txt"))?,
            if replace {
                b"fixture\n".as_slice()
            } else {
                b"old destination".as_slice()
            }
        );
        s.send(b"\x1b", "return to file panel", |s| {
            s.command_prompt_active() && !s.text().contains("Transfer Center")
        })?;
    }
    s.finish()
}

#[test]
fn txt_preference_settings_enter_cancel_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::with_preferences(
        "txt-preference",
        "\n[file_manager]\nopen_txt_in_viewer=false\n",
    )?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1bofs", "file manager setup", |s| {
        s.text().contains("File Manager Setup")
            && s.text().contains("Open TXT files in internal viewer")
    })?;
    s.send(b"x", "TXT draft enabled", |s| {
        s.text().contains("[X] Open TXT files in internal viewer")
    })?;
    s.send(b"\x1b", "cancel TXT draft", |s| {
        s.command_prompt_active() && !s.text().contains("File Manager Setup")
    })?;
    s.send(b"\x1bofs", "reopen unmodified TXT setting", |s| {
        s.text().contains("[ ] Open TXT files in internal viewer")
    })?;
    s.send(b"x\r", "apply TXT preference", |s| {
        s.command_prompt_active() && !s.text().contains("File Manager Setup")
    })?;
    s.send(b"\r", "TXT opens in viewer", |s| {
        s.text().contains("F3 Viewer") && s.text().contains("fixture")
    })?;
    s.resize(100, 32)?;
    s.wait("resized TXT viewer", |s| {
        s.text().contains("F3 Viewer") && s.text().contains("fixture")
    })?;
    s.send(b"\x1b", "return to panel", |s| {
        s.command_prompt_active() && !s.text().contains("F3 Viewer")
    })?;
    s.send(b"\x1b[14~", "explicit TXT editor", |s| {
        s.text().contains("F2 Save") && s.text().contains("fixture")
    })?;
    s.send(b"\x1b", "close unchanged editor", |s| {
        s.command_prompt_active() && !s.text().contains("F2 Save")
    })?;
    assert_eq!(fs::read(s.root.join("item-000.txt"))?, b"fixture\n");
    s.finish()
}
