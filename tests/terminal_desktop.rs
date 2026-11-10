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
        s.text().contains("Temp Drive directory")
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
        s.command_prompt_active() && s.text().contains("Transfers 0/0")
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
            && s.text().contains("Transfers 0/0")
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

#[test]
fn zip_updates_edit_add_remove_cancel_and_terminal_restoration() -> io::Result<()> {
    use std::io::{Read, Write};
    fn member(path: &std::path::Path, name: &str) -> Option<String> {
        let mut zip = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
        let mut value = String::new();
        zip.by_name(name).ok()?.read_to_string(&mut value).ok()?;
        Some(value)
    }
    let mut s = Session::new("zip-updates")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    let path = s.root.join("fixture.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
    for (name, text) in [("root.txt", "original"), ("keep.txt", "keep")] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())?;
        zip.write_all(text.as_bytes())?;
    }
    zip.finish()?;
    fs::write(s.root.join("add.txt"), "added")?;
    s.send(b"\x12", "refresh ZIP fixture", |s| {
        s.command_prompt_active()
    })?;
    s.select_file("fixture.zip")?;
    s.send(b"\r", "archive pane", |s| {
        s.text().contains("root.txt") && s.text().contains(":: /")
    })?;
    s.select_file("root.txt")?;
    s.send(b"\x1b[14~", "ZIP member editor", |s| {
        s.text().contains("F2 Save") && s.text().contains("original")
    })?;
    s.paste("edited ", "ZIP draft", |s| {
        s.text().contains("edited original")
    })?;
    s.send(b"\x1b[12~", "save ZIP member", |_| {
        member(&path, "root.txt").as_deref() == Some("edited original")
    })?;
    s.send(b"\x1b", "close saved ZIP editor", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\t", "switch to disk pane", |s| s.command_prompt_active())?;
    s.select_file("add.txt")?;
    s.send(b"\x1b[15~", "ZIP update confirmation", |s| {
        s.text().contains("Update ZIP") && s.text().contains("Matching files")
    })?;
    s.send(b"\r", "safe default cancels ZIP update", |s| {
        s.command_prompt_active() && !s.text().contains("Update ZIP")
    })?;
    assert!(member(&path, "add.txt").is_none());
    s.send(b"\x1b[15~", "ZIP update again", |s| {
        s.text().contains("Update ZIP")
    })?;
    s.send(b"\x1b[Z\r", "confirm ZIP update", |_| {
        member(&path, "add.txt").as_deref() == Some("added")
    })?;
    s.send(b"\x1b", "close ZIP transfer", |s| {
        s.command_prompt_active() && s.text().contains("Transfers 0/0")
    })?;
    s.send(b"\t", "return to ZIP pane", |s| s.command_prompt_active())?;
    s.select_file("add.txt")?;
    s.send(b"\x1b[19~", "ZIP removal confirmation", |s| {
        s.text().contains("Remove ZIP members")
    })?;
    s.send(b"\r", "safe default cancels ZIP removal", |s| {
        s.command_prompt_active()
    })?;
    assert_eq!(member(&path, "add.txt").as_deref(), Some("added"));
    s.send(b"\x1b[19~", "ZIP removal again", |s| {
        s.text().contains("Remove ZIP members")
    })?;
    s.send(b"\x1b[Z\r", "confirm ZIP removal", |_| {
        member(&path, "add.txt").is_none()
    })?;
    s.send(b"\x1b", "close removal transfer", |s| {
        // Publication precedes the worker completion. Wait for the host to
        // acknowledge completion before asking it to quit.
        s.command_prompt_active() && s.text().contains("Transfers 0/0")
    })?;
    assert_eq!(member(&path, "keep.txt").as_deref(), Some("keep"));
    assert_eq!(fs::read_to_string(s.root.join("add.txt"))?, "added");
    s.finish()
}

#[test]
fn streaming_viewer_reaches_bytes_beyond_preview_limit_and_restores_terminal() -> io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    let mut s = Session::new("streaming-viewer")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    let mut file = fs::File::create(s.root.join("large.bin"))?;
    let offset = 65 * 1024 * 1024u64;
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(b"STREAM-END")?;
    drop(file);
    s.send(b"\x12", "refresh large fixture", |s| {
        s.command_prompt_active()
    })?;
    s.select_file("large.bin")?;
    s.send(b"\x1b[13~", "streaming F3", |s| {
        s.text().contains("Streaming") && s.text().contains("00000000")
    })?;
    s.send(b"\x07", "streaming byte Go to", |s| {
        s.text().contains("Go to")
    })?;
    s.send(
        format!("{offset}\r").as_bytes(),
        "read beyond old preview limit",
        |s| s.text().contains("STREAM-END"),
    )?;
    s.resize(100, 32)?;
    s.send(b"\x1b[H", "streaming Home", |s| {
        s.text().contains("00000000") && !s.text().contains("STREAM-END")
    })?;
    s.send(b"\x1b[F", "streaming End", |s| {
        s.text().contains("STREAM-END")
    })?;
    s.send(b"\x1b", "close streaming viewer", |s| {
        s.command_prompt_active()
    })?;
    s.finish()
}

#[test]
fn session_save_restart_documents_and_no_session_restore_terminal() -> io::Result<()> {
    let mut s = Session::with_preferences("session-restart", "[session]\nrestore='documents'\n")?;
    s.wait("manager ready", |s| {
        s.manager_document_ready() && s.text().contains("item-000.")
    })?;
    s.select_file("inside")?;
    s.send(b"\r", "enter remembered folder", |s| {
        s.command_prompt_active() && s.text().contains("child.txt")
    })?;
    s.select_file("child.txt")?;
    s.send(b"\x1b[14~", "open remembered editor", |s| {
        s.text().contains("F2 Save") && s.text().contains("1 child")
    })?;
    let config = s.root.join("settings/dn/config.conf");
    s.send(b"\x1bo", "editor Options menu", |s| {
        s.text().contains("Autoindent") && s.text().contains("Tab width")
    })?;
    s.send(b"s", "save desktop session", |_| {
        dn::config::Config::read(&config)
            .is_ok_and(|c| c.session.is_some_and(|s| s.documents.len() == 1))
    })?;
    s.send(b"\x1b", "close editor after explicit snapshot", |s| {
        s.command_prompt_active()
    })?;
    s.finish()?;
    let mut s = s.restart(false)?;
    s.wait("restore editor and panes", |s| {
        s.text().contains("child")
            && s.text().contains("F2 Save")
            && fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
                .is_ok_and(|v| v.contains("focus=Editor"))
    })?;
    s.resize(100, 32)?;
    s.send(b"\x1b", "close restored editor", |s| {
        s.command_prompt_active() && s.text().contains("/inside")
    })?;
    s.finish()?;
    let mut s = s.restart(true)?;
    s.wait("fresh launch bypasses saved session", |s| {
        s.manager_document_ready() && s.text().contains("item-000.")
    })?;
    assert!(!s.text().contains("F2 Save"));
    assert!(!s.text().contains("/inside"));
    s.finish()
}

#[test]
fn accurate_copy_help_and_corrupt_archive_error_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("help-errors")?;
    s.wait("manager ready", |s| {
        s.manager_document_ready() && s.text().contains("item-000.")
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1b[15~", "copy dialog", |s| {
        s.text().contains("Copy file")
    })?;
    s.send(b"\x1bOP", "current copy help", |s| {
        s.text().contains("Ask before replacing is the default")
    })?;
    s.send(b"\x1b", "return to copy draft", |s| {
        s.text().contains("Copy file") && !s.text().contains("Ask before replacing is the default")
    })?;
    s.send(b"\x1b", "cancel copy", |s| s.command_prompt_active())?;
    fs::write(s.root.join("broken.zip"), b"not a zip")?;
    s.send(b"\x12", "refresh corrupt fixture", |s| {
        s.command_prompt_active()
    })?;
    s.select_file("broken.zip")?;
    s.send(b"\r", "archive error explains action", |s| {
        s.text().contains("Cannot read the archive.")
    })?;
    s.send(b"\x1b[F", "scroll to original cause", |s| {
        s.text().contains("Cause")
    })?;
    s.resize(100, 32)?;
    s.send(b"\x1b", "dismiss owned archive error", |s| {
        s.command_prompt_active()
    })?;
    assert_eq!(fs::read(s.root.join("broken.zip"))?, b"not a zip");
    assert_eq!(fs::read(s.root.join("item-000.txt"))?, b"fixture\n");
    s.finish()
}

#[test]
fn tree_usage_analysis_stop_order_navigation_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::new("tree-usage")?;
    let root = s.root.join("usage-dir");
    for (name, size) in [("large", 8192), ("small", 1024), (".hidden", 256)] {
        fs::create_dir_all(root.join(name))?;
        fs::write(root.join(name).join("data"), vec![b'x'; size])?;
    }
    fs::write(root.join("direct"), vec![b'x'; 512])?;
    for i in 0..2000 {
        fs::create_dir_all(root.join("cancel-set").join(format!("d{i:04}")))?;
    }
    s.wait("manager ready", |s| {
        s.manager_document_ready() && s.text().contains("item-000.")
    })?;
    s.send(b"\x12", "refresh analysis fixture", |s| {
        s.manager_document_ready()
    })?;
    s.select_file("usage-dir")?;
    s.send(b"\r", "enter analysis root", |s| {
        s.manager_document_ready() && s.text().contains("large")
    })?;
    s.send(b"\x14", "tree with volume capacity", |s| {
        // Analyze is painted before Locate has established a selected row.
        // Wait for the tree's completed branch-loading hint before sending F5.
        s.text().contains("Directory tree")
            && s.text().contains("Analyze")
            && s.text().contains("Right/+ expand")
    })?;
    s.send(b"\x1b[15~", "start recursive analysis", |s| {
        s.text().contains("Directory tree")
            && (s.text().contains("Stop") || s.text().contains("Complete"))
    })?;
    s.send(b"\x1b", "stop or close completed analysis", |s| {
        s.text().contains("Stopped — partial") || s.manager_document_ready()
    })?;
    if s.manager_document_ready() {
        // Esc closes a scan that completed before the key was dispatched.
        // Closing retires the tree session and its cache; a fresh tree has no
        // snapshot. Active-job cancellation is checked by the controller test.
        s.send(b"\x14", "reopen tree after completed scan", |s| {
            s.text().contains("Directory tree")
                && s.text().contains("No size snapshot for this directory")
                && s.text().contains("Analyze")
                && s.text().contains("Right/+ expand")
        })?;
    }
    s.send(b"\x1b[15~", "complete recursive analysis", |s| {
        s.text().contains("Complete") && s.text().contains("9.7 KiB")
    })?;
    s.send(b"\x1b[17~", "largest-first ordering", |s| {
        s.text().contains("Order: Size")
    })?;
    s.resize(100, 32)?;
    s.wait("analysis survives resize", |s| {
        s.text().contains("Order: Size") && s.text().contains("Volume available / total")
    })?;
    s.send(b"\x1b[C", "select largest child", |s| {
        s.text().contains("Apparent size: 8.0 KiB")
    })?;
    s.send(b"\r", "open result in originating panel", |s| {
        s.manager_document_ready()
    })?;
    // Reopening activates the retained tree and its measured selection.
    s.send(b"\x14", "retained analysis", |s| {
        s.text().contains("Directory tree") && s.text().contains("8.0 KiB")
    })?;
    s.send(b"\x1b", "close tree", |s| s.manager_document_ready())?;
    assert_eq!(fs::metadata(root.join("large/data"))?.len(), 8192);
    assert_eq!(fs::metadata(root.join(".hidden/data"))?.len(), 256);
    s.finish()
}

#[test]
fn remote_connection_dialog_help_resize_and_cancel_restore_terminal() -> io::Result<()> {
    use dn::localization::Language;
    for language in [Language::English, Language::Russian] {
        let preferences = format!("[regional]\nlanguage='{}'\n", language.key());
        let mut s = Session::with_preferences("remote-connections", &preferences)?;
        s.wait("manager ready", |s| {
            s.command_prompt_active() && s.text().contains("item-000.")
        })?;
        s.send(b"\x1bun", "remote connection dialog", |s| {
            s.text().contains(language.text("Remote connections"))
                && s.text().contains(language.text("Host"))
        })?;
        s.send(b"\x1bOP", "remote connection help", |s| {
            s.text()
                .contains(language.text(" Remote connection help ").trim())
        })?;
        s.send(b"\x1b", "return from connection help", |s| {
            s.text().contains(language.text("Profile name"))
                && !s
                    .text()
                    .contains(language.text(" Remote connection help ").trim())
        })?;
        s.resize(100, 32)?;
        s.wait("connection resize", |s| {
            s.text().contains(language.text("Save profile"))
        })?;
        s.resize(40, 12)?;
        s.resize(80, 25)?;
        s.wait("connection restored", |s| {
            s.text().contains(language.text("Remote connections"))
                && s.text().contains(language.text("SSH terminal"))
        })?;
        s.send(b"\x1b", "cancel connection", |s| {
            s.command_prompt_active() && !s.text().contains(language.text("Remote connections"))
        })?;
        s.finish()?;
    }
    Ok(())
}

#[test]
fn ftp_pane_preview_download_disconnect_and_terminal_restoration() -> io::Result<()> {
    use std::{
        io::{BufRead, Write},
        net::{Ipv4Addr, TcpListener},
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let server = std::thread::spawn(move || -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut control, _) = loop {
            match listener.accept() {
                Ok(v) => break v,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => return Err(e),
            }
        };
        // macOS can inherit O_NONBLOCK from the accepting listener.
        control.set_nonblocking(false)?;
        control.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut input = std::io::BufReader::new(control.try_clone()?);
        control.write_all(b"220 DN private terminal fixture\r\n")?;
        let mut passive = None;
        loop {
            let mut line = String::new();
            if input.read_line(&mut line)? == 0 {
                break;
            }
            let line = line.trim_end();
            if line.starts_with("USER ") {
                control.write_all(b"331 Password\r\n")?;
            } else if line == "PASS" || line.starts_with("PASS ") {
                control.write_all(b"230 Accepted\r\n")?;
            } else if line == "TYPE I" {
                control.write_all(b"200 Binary\r\n")?;
            } else if line == "PASV" {
                let data = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
                let p = data.local_addr()?.port();
                write!(
                    control,
                    "227 Passive (127,0,0,1,{},{})\r\n",
                    p / 256,
                    p % 256
                )?;
                passive = Some(data);
            } else if line.starts_with("MLSD") || line.starts_with("RETR ") {
                control.write_all(b"150 Data\r\n")?;
                let (mut data, _) = passive
                    .take()
                    .ok_or_else(|| io::Error::other("PASV required"))?
                    .accept()?;
                if line.starts_with("MLSD") {
                    data.write_all(b"type=file;size=20; remote.txt\r\n")?;
                } else {
                    assert_eq!(line, "RETR /remote.txt");
                    data.write_all(b"remote text fixture\n")?;
                }
                drop(data);
                control.write_all(b"226 Done\r\n")?;
            } else if line == "QUIT" {
                control.write_all(b"221 Bye\r\n")?;
                break;
            } else {
                control.write_all(b"502 Unsupported\r\n")?;
            }
        }
        Ok(())
    });
    let mut s = Session::new("remote-ftp-pane")?;
    fs::write(s.root.join("settings/dn/remote-connections.conf"),format!("[[connections]]\nname='Private fixture'\nprotocol='FTP'\nhost='127.0.0.1'\nport={port}\nuser='tester'\ndirectory='/'\n"))?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x1bun", "remote FTP dialog", |s| {
        s.text().contains("Private fixture") && s.text().contains("Remote connections")
    })?;
    s.send(b"\r", "plain FTP confirmation", |s| {
        s.text().contains("without encryption")
    })?;
    s.send(b"\r", "FTP pane listing", |s| {
        s.command_prompt_active() && s.text().contains("ftp://") && s.text().contains("remote.txt")
    })?;
    s.send(b"\x1b[B\x1bOR", "remote F3 preview", |s| {
        s.text().contains("remote text fixture")
    })?;
    s.send(b"\x1b", "return to remote pane", |s| {
        s.command_prompt_active() && s.text().contains("ftp://")
    })?;
    s.send(b"\x1b[15~", "remote download dialog", |s| {
        s.text().contains("Copy") && s.text().contains("remote.txt")
    })?;
    s.send(b"\r", "remote download completed", |s| {
        s.text().contains("Completed")
            && fs::read(s.root.join("remote.txt")).is_ok_and(|b| b == b"remote text fixture\n")
    })?;
    s.send(b"\x1b", "return from Transfer Center", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1b[5;5~", "disconnect remote pane", |s| {
        s.command_prompt_active() && !s.text().contains("ftp://")
    })?;
    assert_eq!(
        fs::read(s.root.join("remote.txt"))?,
        b"remote text fixture\n"
    );
    s.finish()?;
    server.join().unwrap()?;
    Ok(())
}

#[test]
fn network_information_menu_refresh_resize_and_close_restore_terminal() -> io::Result<()> {
    use dn::localization::Language;
    for language in [Language::English, Language::Russian] {
        let preferences = format!("[regional]\nlanguage='{}'\n", language.key());
        let mut s = Session::with_preferences("network-information", &preferences)?;
        s.wait("manager ready", |s| {
            s.command_prompt_active() && s.text().contains("item-000.")
        })?;
        s.send(b"\x1bui", "network information menu", |s| {
            s.text().contains(language.text("Network Information"))
                && s.text().contains(language.text("Internet reachability"))
        })?;
        s.send(b"\x1b[Z\r", "open interface dropdown", |s| {
            s.text().contains(language.text("Network interface"))
                && !s.text().contains(language.text("Internet reachability"))
        })?;
        s.send(b"\x1b", "cancel interface dropdown", |s| {
            s.text().contains(language.text("Internet reachability"))
                && s.text().contains(language.text("Network Information"))
        })?;
        s.send(b"\r", "reopen interface dropdown", |s| {
            !s.text().contains(language.text("Internet reachability"))
        })?;
        s.send(b"\x1b[F\r", "select last interface", |s| {
            s.text().contains(language.text("Internet reachability"))
        })?;
        s.send(b"\x1b[F", "network details", |s| s.text().contains("MTU"))?;
        s.send(b"\x1b[15~", "network refresh", |s| {
            s.text().contains(language.text("Internet reachability"))
        })?;
        s.resize(100, 32)?;
        s.resize(40, 12)?;
        s.resize(80, 25)?;
        s.wait("network window restored", |s| {
            s.text().contains(language.text("Network Information"))
        })?;
        s.send(b"\x1b", "close network information", |s| {
            s.command_prompt_active() && !s.text().contains(language.text("Network Information"))
        })?;
        s.finish()?;
    }
    Ok(())
}

#[test]
fn saved_profiles_select_edit_rename_cancel_delete_and_restore_terminal() -> io::Result<()> {
    use dn::localization::Language;
    fn click_text(
        s: &mut Session,
        label: &str,
        stage: &str,
        ready: impl Fn(&Session) -> bool,
    ) -> io::Result<()> {
        let text = s.text();
        let (row, line) = text
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(label))
            .ok_or_else(|| io::Error::other(format!("Missing button {label}")))?;
        let byte = line.find(label).unwrap();
        let column = unicode_width::UnicodeWidthStr::width(&line[..byte]);
        let input = format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            column + 1,
            row + 1,
            column + 1,
            row + 1
        );
        s.send(input.as_bytes(), stage, ready)
    }
    for language in [Language::English, Language::Russian] {
        let preferences = format!("[regional]\nlanguage='{}'\n", language.key());
        let mut s = Session::with_preferences("saved-profiles", &preferences)?;
        let path = s.root.join("settings/dn/remote-connections.conf");
        let text=[("Alpha","alpha.invalid"),("Beta","beta.invalid"),("Гамма 世界","gamma.invalid")].into_iter().map(|(name,host)|format!("[[connections]]\nname='{name}'\nprotocol='SFTP'\nhost='{host}'\nport=22\nuser='fixture'\ndirectory='/'\n")).collect::<String>();
        fs::write(&path, text)?;
        s.wait("manager ready", |s| {
            s.command_prompt_active() && s.text().contains("item-000.")
        })?;
        s.send(b"\x1bun", "connection form", |s| {
            s.text().contains(language.text("Remote connections"))
                && s.text().contains("alpha.invalid")
        })?;
        click_text(
            &mut s,
            language.text("Profiles"),
            "saved profile list",
            |s| {
                s.text()
                    .contains(language.text("Saved connection profiles"))
                    && s.text().contains("Beta")
            },
        )?;
        click_text(&mut s, "Beta", "select Beta row", |s| {
            s.text().contains("beta.invalid")
        })?;
        click_text(
            &mut s,
            language.text("Edit"),
            "edit selected profile",
            |s| {
                s.text().contains(language.text("Profile name"))
                    && s.text().contains("beta.invalid")
                    && !s
                        .text()
                        .contains(language.text("Saved connection profiles"))
            },
        )?;
        s.send(
            b"\x1b[H\x1b[3~\x1b[3~\x1b[3~\x1b[3~Renamed",
            "rename draft",
            |s| s.text().contains("Renamed"),
        )?;
        click_text(
            &mut s,
            language.text("Save profile"),
            "save renamed profile",
            |s| {
                s.text()
                    .contains(language.text("Connection profile saved without passwords"))
            },
        )?;
        let records: toml::Table = fs::read_to_string(&path)?
            .parse()
            .map_err(io::Error::other)?;
        assert_eq!(records["connections"].as_array().unwrap().len(), 3);
        assert!(records["connections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"].as_str() == Some("Renamed")));
        assert!(!records["connections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"].as_str() == Some("Beta")));
        click_text(
            &mut s,
            language.text("Profiles"),
            "profile list after save",
            |s| {
                s.text()
                    .contains(language.text("Saved connection profiles"))
                    && s.text().contains("Renamed")
            },
        )?;
        s.resize(100, 32)?;
        s.resize(40, 12)?;
        s.resize(80, 25)?;
        s.wait("profile list resized", |s| {
            s.text()
                .contains(language.text("Saved connection profiles"))
                && s.text().contains("Renamed")
        })?;
        click_text(
            &mut s,
            language.text("Delete"),
            "confirm profile removal",
            |s| s.text().contains(language.text("Delete saved profile")),
        )?;
        s.send(b"\r", "Cancel is the safe default", |s| {
            s.text()
                .contains(language.text("Saved connection profiles"))
                && !s.text().contains(language.text("Delete saved profile"))
        })?;
        let records: toml::Table = fs::read_to_string(&path)?.parse().unwrap();
        assert_eq!(records["connections"].as_array().unwrap().len(), 3);
        click_text(
            &mut s,
            language.text("Delete"),
            "reopen deletion confirmation",
            |s| s.text().contains(language.text("Delete saved profile")),
        )?;
        // Choose the button row, rather than the word in the confirmation title.
        s.send(b"\t\r", "delete saved profile", |s| {
            s.text()
                .contains(language.text("Saved profile deleted. Open connections are unchanged."))
        })?;
        let records: toml::Table = fs::read_to_string(&path)?.parse().unwrap();
        let records = records["connections"].as_array().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|v| v["name"].as_str() != Some("Renamed")));
        s.send(b"\x1b", "return from profiles", |s| {
            s.text().contains(language.text("Profile name"))
                && !s
                    .text()
                    .contains(language.text("Saved connection profiles"))
        })?;
        s.send(b"\x1b", "close connection form", |s| {
            s.command_prompt_active()
        })?;
        s.finish()?;
    }
    Ok(())
}

#[test]
fn network_live_loopback_rates_and_switching_restore_terminal() -> io::Result<()> {
    use dn::localization::Language;
    use std::net::UdpSocket;
    let snapshot = dn::network::information::snapshot();
    let loopback_name = if cfg!(target_os = "macos") {
        "lo0"
    } else {
        "lo"
    };
    let index = snapshot
        .adapters
        .iter()
        .position(|adapter| adapter.key.name == loopback_name)
        .ok_or_else(|| io::Error::other("Loopback interface missing from local snapshot"))?;
    let receiver = UdpSocket::bind("127.0.0.1:0")?;
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    receiver.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    for language in [Language::English, Language::Russian] {
        let preferences = format!("[regional]\nlanguage='{}'\n", language.key());
        let mut s = Session::with_preferences("network-live-rates", &preferences)?;
        s.wait("manager ready", |s| {
            s.command_prompt_active() && s.text().contains("item-000.")
        })?;
        s.send(b"\x1bui", "network information", |s| {
            s.text().contains(language.text("Internet reachability"))
        })?;
        s.send(b"\x1b[Z\r", "interface dropdown", |s| {
            !s.text().contains(language.text("Internet reachability"))
        })?;
        let mut selection = b"\x1b[H".to_vec();
        for _ in 0..index {
            selection.extend_from_slice(b"\x1b[B");
        }
        selection.push(b'\r');
        s.send(&selection, "measure loopback baseline", |s| {
            s.text().contains(language.text("Download"))
                && s.text().contains("B/s")
                && s.text().contains(loopback_name)
        })?;
        let mut received = [0u8; 4096];
        for _ in 0..256 {
            sender.send_to(&[0x55u8; 4096], receiver.local_addr()?)?;
            let (len, _) = receiver.recv_from(&mut received)?;
            assert_eq!(len, 4096);
        }
        s.wait("live received and sent traffic", |s| {
            let text = s.text();
            let Some(line) = text
                .lines()
                .find(|line| line.contains(language.text("Download")))
            else {
                return false;
            };
            let Some((download, upload)) = line.split_once(language.text("Upload")) else {
                return false;
            };
            let scaled = |value: &str| {
                ["KiB/s", "MiB/s", "GiB/s"]
                    .iter()
                    .any(|unit| value.contains(unit))
            };
            scaled(download) && scaled(upload)
        })?;
        s.send(b"\x1b[15~", "refresh restarts rates", |s| {
            s.text().contains(language.text("Internet reachability"))
                && s.text().contains(loopback_name)
        })?;
        s.send(b"\x1b", "close live rates", |s| s.command_prompt_active())?;
        s.finish()?;
    }
    Ok(())
}
