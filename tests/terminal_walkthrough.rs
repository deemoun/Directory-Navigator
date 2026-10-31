#![cfg(target_os = "macos")]
#[path = "support/pty.rs"]
mod pty;
use pty::Session;
use std::{fs, io};

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
fn mp3_prototype_compact_expanded_help_movement_and_close_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("mp3-player")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("F5 Copy")
    })?;
    // The prototype is opt-in; enable it through the actual settings draft.
    s.send(b"\x1bocp", "plugin settings", |s| {
        s.text().contains("Plugins") && s.text().contains("mp3_player")
    })?;
    s.send(b"\t \r", "enable MP3 player", |s| {
        !s.text().contains("mp3_player") && s.command_prompt_active()
    })?;
    s.send(b"\x1bu3", "compact MP3 player", |s| {
        s.text()
            .lines()
            .nth(8)
            .is_some_and(|r| r.contains("MP3 Player") && r.chars().nth(22) == Some('╔'))
    })?;
    let compact = s.text();
    assert!(!compact.contains("UI prototype"));
    s.send(b"\x1bz", "expanded MP3 player", |s| {
        let t = s.text();
        t.contains("UI prototype") && t.contains("Pause") && t.contains("Volume")
    })?;
    // Buttons only demonstrate navigation; Open must not create a file picker.
    s.send(b"\t\t\t\t\t\r", "inert Open", |s| {
        s.text().contains("UI prototype")
    })?;
    assert!(!s.text().contains("Open file"));
    s.send(b"\x1bOP", "player help", |s| {
        s.text().contains("MP3 Player Help")
    })?;
    s.send(b"\x1b", "close player help", |s| {
        !s.text().contains("MP3 Player Help")
    })?;
    s.send(b"\x1b[15;5~", "move player", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[C\r", "accept player movement", |s| {
        !s.text().contains("Arrows move")
    })?;
    s.send(b"\x1bz", "compact again", |s| {
        !s.text().contains("UI prototype") && s.text().contains("MP3 Player")
    })?;
    s.send(b"\x1b", "close player", |s| {
        !s.text().contains("MP3 Player") && s.command_prompt_active()
    })?;
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
        s.command_prompt_active()
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
fn block_commands_and_viewer_addresses_preserve_files_and_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("block-addresses")?;
    s.wait("manager ready", |s| {
        s.text().contains("item-000.")
            && !s.text().contains("Reading directory")
            && s.command_prompt_active()
    })?;
    let original = "zebra\nalpha\n([界])\n";
    fs::write(s.root.join("item-000.txt"), original)?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1b[14~", "editor loaded", |s| {
        s.text().contains("zebra") && s.text().contains("F2 Save")
    })?;
    s.send(b"\x01\x1bbs", "sort selected block", |s| {
        let t = s.text();
        t.find("alpha")
            .zip(t.find("zebra"))
            .is_some_and(|(a, z)| a < z)
    })?;
    s.send(b"\x1bbr", "uppercase block", |s| {
        s.text().contains("ZEBRA") && s.text().contains("ALPHA")
    })?;
    s.send(b"\x1a", "undo case conversion", |s| {
        s.text().contains("zebra") && !s.text().contains("ZEBRA")
    })?;
    s.send(b"\t\x1b[Z", "indent and unindent", |s| {
        s.text().contains("zebra")
    })?;
    s.send(b"\x1b", "dirty close guard", |s| {
        s.text().contains("Discard")
    })?;
    s.send(b"d", "discard unsaved block edits", |s| {
        s.command_prompt_active() && !s.text().contains("F2 Save")
    })?;
    assert_eq!(fs::read_to_string(s.root.join("item-000.txt"))?, original);
    s.send(b"\x1b[13~", "viewer opened", |s| {
        s.text().contains("F3 Viewer") && s.text().contains("zebra")
    })?;
    s.send(b"\x07", "viewer go to", |s| {
        s.text().contains("Viewer Go to")
    })?;
    s.send(b"3\r", "line navigation", |s| {
        !s.text().contains("Viewer Go to") && s.text().contains("3/3 lines")
    })?;
    s.send(b"\x1b", "close viewer", |s| {
        s.command_prompt_active() && !s.text().contains("F3 Viewer")
    })?;
    assert_eq!(fs::read_to_string(s.root.join("item-000.txt"))?, original);
    s.finish()
}

#[test]
fn recovery_inspect_later_restore_and_discard_leave_source_and_terminal_intact() -> io::Result<()> {
    let mut s = Session::new_recovery("recovery")?;
    s.wait("recovery offered", |s| {
        s.text().contains("Unsaved work from a previous session")
    })?;
    s.send(b"\t\r", "inspect checkpoint", |s| {
        s.text().contains("recovery 界")
    })?;
    s.send(b"\x1b", "return to recovery", |s| {
        s.text().contains("Unsaved work from a previous session")
    })?;
    s.send(b"\x1b", "defer recovery", |s| s.command_prompt_active())?;
    s.send(b"\x1bf\x1b[F\x1b[A\r", "reopen recovery", |s| {
        s.text().contains("Unsaved work from a previous session")
    })?;
    s.send(b"\r", "recover unsaved copy", |s| {
        s.text().contains("F2 Save") && s.text().contains("recovery 界")
    })?;
    assert_eq!(fs::read(s.root.join("source.txt"))?, b"original source\n");
    s.send(b"\x1b", "guard recovered document close", |s| {
        s.text().contains("Discard")
    })?;
    s.send(b"d", "discard recovered document", |s| {
        s.command_prompt_active()
    })?;
    s.finish()?;
    assert_eq!(fs::read(s.root.join("source.txt"))?, b"original source\n");
    for dir in fs::read_dir(s.root.join("settings/dn/editor-recovery"))? {
        for file in fs::read_dir(dir?.path())? {
            assert!(file?.path().extension().is_none_or(|s| s != "rec"));
        }
    }
    Ok(())
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
fn mp3_demo_playlist_track_picker_help_and_cancel_are_ui_only() -> io::Result<()> {
    let mut s = Session::new("mp3-playlist")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active()
            && s.text().contains("item-000.")
            && !s.text().contains("Reading directory")
    })?;
    s.send(b"\x1bocp", "plugins", |s| s.text().contains("mp3_player"))?;
    s.send(b"\t \r", "enable prototype", |s| {
        s.command_prompt_active() && !s.text().contains("mp3_player")
    })?;
    s.send(b"\x1bu3", "player", |s| s.text().contains("MP3 Player"))?;
    s.send(b"\x1bz", "expanded player", |s| {
        s.text().contains("UI prototype")
    })?;
    // Play is initially focused; ten tabs reach List in expanded row order.
    s.send(b"\t\t\t\t\t\t\t\t\t\t\r", "demo play order", |s| {
        s.text().contains("Play order") && s.text().contains("Demo track 01")
    })?;
    s.send(b"\t\t\r", "Add track chooser", |s| {
        s.text().contains("Select track")
    })?;
    s.send(b"\x1b[B\r", "append demo track", |s| {
        s.text().contains("Play order") && !s.text().contains("Select track")
    })?;
    s.send(b"\x1bOP", "owned player Help", |s| {
        s.text().contains("MP3 Player Help")
    })?;
    s.send(b"\x1b", "return to draft", |s| {
        s.text().contains("Play order") && !s.text().contains("MP3 Player Help")
    })?;
    s.send(b"\x1b", "cancel playlist", |s| {
        s.text().contains("MP3 Player") && !s.text().contains("Play order")
    })?;
    s.send(b"\x1b", "close prototype", |s| s.command_prompt_active())?;
    assert_eq!(fs::read(s.root.join("item-000.txt"))?, b"fixture\n");
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
    s.send(b"\x1b[15;5~", "move calculator", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[C\x1b[B\r", "calculator moved", |s| {
        !s.text().contains("Arrows move")
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
    s.send(b"\x1b[B\r", "fixture directory opened", |s| {
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
fn finished_game_buttons_remain_clickable_after_game_over() -> io::Result<()> {
    let mut s = Session::new("game-over-buttons")?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.send(b"\x1b[20;3~", "game opened", |s| {
        s.text().contains("Navigator's game")
    })?;
    s.send(b"+++++++++", "fast game", |s| {
        s.text().contains("Level: 10")
    })?;
    for _ in 0..80 {
        if s.text().contains("Game over") {
            break;
        }
        fn score(s: &Session) -> Option<u64> {
            s.text()
                .split_once("Score:")?
                .1
                .trim_start()
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        }
        let previous = score(&s).expect("Visible game score");
        s.send(b" ", "fixture piece locked or game ended", |s| {
            s.text().contains("Game over") || score(s).is_some_and(|score| score != previous)
        })?;
    }
    s.wait("game over dialog ready", |s| {
        let text = s.text();
        text.contains("Game over") && text.contains("Final score:") && text.contains("OK")
    })?;
    // Reproduce a final drop key arriving after the Game over page appeared.
    // Space must not start a pending OK press that would swallow the next Esc.
    // An ignored Space need not publish a new frame; acknowledge the resulting
    // Esc transition instead of waiting for an unchanged screen to redraw.
    s.send(b" \x1b", "finished board", |s| {
        s.text().contains("GAME OVER") && !s.text().contains("Game over")
    })?;
    fn new_position(s: &Session) -> (usize, usize) {
        let text = s.text();
        let (y, line) = text
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("New") && !line.contains("F2"))
            .expect("New button row");
        (line[..line.find("New").unwrap()].chars().count(), y)
    }
    let (x, y) = new_position(&s);
    let down = format!("\x1b[<0;{};{}M", x + 1, y + 1);
    s.send(
        down.as_bytes(),
        "New visibly pressed on finished board",
        |s| s.text().contains("GAME OVER") && new_position(s) == (x + 1, y),
    )?;
    let up = format!("\x1b[<0;{};{}m", x + 1, y + 1);
    s.send(up.as_bytes(), "New starts on release", |s| {
        !s.text().contains("GAME OVER") && s.text().contains("Navigator's game")
    })?;
    s.send(b"\x1b", "game closed", |s| s.command_prompt_active())?;
    s.finish()
}

#[test]
fn empty_recovery_menu_shows_one_message_and_returns_to_manager() -> io::Result<()> {
    let mut s = Session::new("empty-recovery")?;
    s.wait("empty startup stays quiet", |s| s.command_prompt_active())?;
    s.send(b"\x1bf", "File menu", |s| {
        s.text().contains("Recover unsaved documents")
    })?;
    s.send(b"\x1b[F\x1b[A\r", "empty recovery message", |s| {
        s.text().contains("No files to recover")
    })?;
    s.send(b"\r", "empty recovery acknowledged", |s| {
        !s.text().contains("No files to recover") && s.command_prompt_active()
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
