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

fn music_click(
    s: &mut Session,
    label: &str,
    purpose: &str,
    ready: impl Fn(&Session) -> bool,
) -> io::Result<()> {
    music_click_row(s, label, None, purpose, ready)
}
fn music_click_row(
    s: &mut Session,
    label: &str,
    row_label: Option<&str>,
    purpose: &str,
    ready: impl Fn(&Session) -> bool,
) -> io::Result<()> {
    let screen = s.text();
    let (y, line, at) = screen
        .lines()
        .enumerate()
        .filter(|(_, line)| row_label.is_none_or(|needle| line.contains(needle)))
        .find_map(|(y, line)| line.find(label).map(|at| (y, line, at)))
        .ok_or_else(|| io::Error::other(format!("Missing music control {label}:\n{screen}")))?;
    let x = unicode_width::UnicodeWidthStr::width(&line[..at]);
    let click = format!("\x1b[<0;{};{}M\x1b[<0;{};{}m", x + 2, y + 1, x + 2, y + 1);
    s.send(click.as_bytes(), purpose, ready)
}
fn music_repeat(s: &Session, value: &str) -> bool {
    s.text()
        .lines()
        .any(|line| line.contains("Repeat:") && line.contains(value))
}

#[test]
fn mp3_empty_player_compact_expanded_help_movement_and_close_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("mp3-player")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("F5 Copy")
    })?;
    // The player is opt-in; enable it through the actual settings draft.
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
            .is_some_and(|r| r.contains("Music Player") && r.chars().nth(22) == Some('╔'))
    })?;
    let compact = s.text();
    assert!(!compact.contains("Empty playlist"));
    s.send(b"\x1bz", "expanded MP3 player", |s| {
        let t = s.text();
        t.contains("Empty playlist") && t.contains("Open…") && t.contains("Volume")
    })?;
    music_click(&mut s, "Open…", "Open shows DN file picker", |s| {
        s.text().contains("Open file")
    })?;
    music_click(&mut s, "Cancel", "cancel Open by mouse", |s| {
        !s.text().contains("Open file") && s.text().contains("Music Player")
    })?;
    s.send(b"\x1bOP", "player help", |s| {
        s.text().contains("Music Player Help")
    })?;
    s.send(b"\x1b", "close player help", |s| {
        !s.text().contains("Music Player Help")
    })?;
    s.send(b"\x1b[15;5~", "move player", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[C\r", "accept player movement", |s| {
        !s.text().contains("Arrows move")
    })?;
    s.send(b"\x1bz", "compact again", |s| {
        !s.text().contains("Volume") && s.text().contains("Music Player")
    })?;
    s.send(b"\x1b", "close player", |s| {
        !s.text().contains("Music Player") && s.command_prompt_active()
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
fn mp3_real_queue_help_cancel_and_volume_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("mp3-queue")?;
    fs::write(
        s.root.join("queue-track.mp3"),
        b"not played in this UI walkthrough",
    )?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.send(b"\x1bocp", "plugins", |s| s.text().contains("mp3_player"))?;
    s.send(b"\t \r", "enable player", |s| s.command_prompt_active())?;
    s.select_file("queue-track.mp3")?;
    s.send(b"\x1bua", "import selection", |s| {
        s.text().contains("Music Player") && !s.text().contains("Adding music files")
    })?;
    s.send(b"\x1bz", "expand", |s| s.text().contains("Volume"))?;
    music_click(&mut s, "Playlist…", "live queue", |s| {
        s.text().contains("Music queue · current session") && s.text().contains("queue-track.mp3")
    })?;
    s.send(b"\x1bOP", "owned Help", |s| {
        s.text().contains("Music Player Help")
    })?;
    s.send(b"\x1b", "return to queue", |s| {
        s.text().contains("Music queue · current session")
            && !s.text().contains("Music Player Help")
    })?;
    s.send(b"\x1b", "close queue", |s| {
        !s.text().contains("Music queue · current session")
    })?;
    music_click_row(
        &mut s,
        "+",
        Some("Volume"),
        "immediate volume increase",
        |s| s.text().contains("60%"),
    )?;
    music_click_row(
        &mut s,
        "−",
        Some("Volume"),
        "immediate volume decrease",
        |s| s.text().contains("50%"),
    )?;
    s.resize(100, 32)?;
    s.send(b"\x1b[17;3~", "work in manager", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1bu3", "return to singleton", |s| {
        s.text().contains("Volume") && !s.command_prompt_active()
    })?;
    s.send(b"\x1b", "close", |s| s.command_prompt_active())?;
    assert_eq!(
        fs::read(s.root.join("queue-track.mp3"))?,
        b"not played in this UI walkthrough"
    );
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

#[test]
#[ignore = "requires a working macOS audio output device; plays a quiet synthetic tone"]
fn mp3_audio_play_pause_seek_background_stop_and_close_restore_terminal() -> io::Result<()> {
    let mut s = Session::with_preferences(
        "mp3-audio",
        "[plugins]\nenabled=['mp3_player']\n[plugins.mp3_player]\nplay_on_enter=true\n",
    )?;
    fs::write(
        s.root.join("tone.mp3"),
        include_bytes!("fixtures/mp3/tone.mp3"),
    )?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.select_file("tone.mp3")?;
    s.send(b"\r", "Enter plays through default audio output", |s| {
        s.text().contains("Playing") && s.text().contains("Elapsed")
    })?;
    // Play -> seek forward -> Pause (no next track).
    music_click(&mut s, "Pause", "pause", |s| s.text().contains("Paused"))?;
    s.send(b"\r", "resume", |s| s.text().contains("Playing"))?;
    music_click(&mut s, "+5s", "seek +5 seconds", |s| {
        s.text().contains("Playing") && (5..12).any(|n| s.text().contains(&format!("00:{n:02}")))
    })?;
    s.send(b"\x1b[17;3~", "manager while audio plays", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1bu3", "return to playing singleton", |s| {
        !s.command_prompt_active() && s.text().contains("Playing")
    })?;
    // Seek-forward focus -> Pause -> Stop.
    music_click(&mut s, "Stop", "stop", |s| {
        s.text().contains("Stopped") && s.text().contains("00:00")
    })?;
    s.send(b"\x1b", "close player", |s| s.command_prompt_active())?;
    s.send(b"\x1bu3", "reopen empty", |s| {
        s.text().contains("Music Player") && !s.text().contains("Volume")
    })?;
    s.send(b"\x1b", "close empty player", |s| s.command_prompt_active())?;
    s.finish()
}

#[test]
fn mp3_folder_tags_modes_info_and_resize_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("mp3-folder-modes")?;
    fs::create_dir_all(s.root.join("music/nested"))?;
    for (file, title) in [
        ("music/a.mp3", "First song"),
        ("music/nested/b.MP3", "Second song"),
    ] {
        let mut body = Vec::new();
        for (id, value) in [
            (b"TIT2", title),
            (b"TPE1", "Test artist"),
            (b"TALB", "Test album"),
        ] {
            let text = [vec![3], value.as_bytes().to_vec()].concat();
            body.extend(id);
            body.extend((text.len() as u32).to_be_bytes());
            body.extend([0, 0]);
            body.extend(text);
        }
        let size = body.len();
        let mut tag = vec![
            b'I',
            b'D',
            b'3',
            3,
            0,
            0,
            ((size >> 21) & 127) as u8,
            ((size >> 14) & 127) as u8,
            ((size >> 7) & 127) as u8,
            (size & 127) as u8,
        ];
        tag.extend(body);
        fs::write(s.root.join(file), tag)?;
    }
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.send(b"\x1bocp", "plugins", |s| s.text().contains("mp3_player"))?;
    s.send(b"\t \r", "enable player", |s| s.command_prompt_active())?;
    s.select_file("music")?;
    s.send(b"\x1bua", "import folder", |s| {
        s.text().contains("Music Player")
            && !s.text().contains("Adding music files")
            && s.plugin_document_ready()
    })?;
    s.send(b"\x1bz", "tagged player", |s| {
        s.text().contains("Test artist") && s.text().contains("First song")
    })?;
    music_click(&mut s, "Shuffle", "shuffle immediately", |s| {
        s.text()
            .lines()
            .any(|line| line.contains("Shuffle") && line.contains('X'))
    })?;
    music_click(&mut s, "Off", "repeat popup", |s| {
        s.text().contains("Cancel") && !s.plugin_document_ready()
    })?;
    s.send(b"\x1b[B\r", "choose repeat one", |s| {
        s.plugin_document_ready() && music_repeat(s, "One")
    })?;
    music_click(&mut s, "One", "repeat popup again", |s| {
        !s.plugin_document_ready() && s.text().contains("Cancel")
    })?;
    s.send(b"\x1b[C\x1b", "cancel repeat selection", |s| {
        s.plugin_document_ready() && music_repeat(s, "One")
    })?;
    s.send(b"\x1bOP", "player Help", |s| {
        s.text().contains("Music Player Help")
    })?;
    s.send(b"\x1b", "return from Help", |s| {
        music_repeat(s, "One") && !s.text().contains("Music Player Help")
    })?;
    music_click(&mut s, "Info…", "track Info", |s| {
        s.text().contains("Music Track Info") && s.text().contains("Test album")
    })?;
    s.send(b"\x1b", "close Info", |s| {
        !s.text().contains("Music Track Info")
    })?;
    music_click(&mut s, "Playlist…", "tagged queue", |s| {
        s.text().contains("Music queue · current session")
            && s.text().contains("First song")
            && s.text().contains("Second song")
    })?;
    s.send(b"\x1b", "close queue", |s| {
        !s.text().contains("Music queue · current session")
    })?;
    s.resize(100, 32)?;
    s.wait("mode retained after resize", |s| {
        s.text()
            .lines()
            .any(|line| line.contains("Shuffle") && line.contains('X'))
            && music_repeat(s, "One")
    })?;
    s.send(b"\x1b[17;3~", "manager", |s| s.command_prompt_active())?;
    s.send(b"\x1bu3", "return to singleton", |s| {
        !s.command_prompt_active() && music_repeat(s, "One")
    })?;
    s.send(b"\x1b", "close player", |s| s.command_prompt_active())?;
    s.finish()
}

#[test]
fn original_screen_savers_lists_all_effects_preview_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::new("original-screen-savers")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x1bocv", "original screen saver lists", |s| {
        s.text().contains("Selected savers") && s.text().contains("Available savers")
    })?;
    s.send(b"\t", "available list focus", |s| {
        s.text().contains("Available savers")
    })?;
    for (index, name) in ["Star flight", "Flash-light", "Clock", "Blackness"]
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            s.send(b"\x1b[B", "next available saver", |s| {
                s.text().contains(name)
            })?;
            s.send(b"\x1b[2~", "add saver to selected", |s| {
                s.text().matches(name).count() >= 2
            })?;
        }
        s.send(b"p", "preview starts", |s| {
            !s.text().contains("Selected savers") && !s.command_prompt_active()
        })?;
        if name == "Clock" {
            s.wait("moving clock visible", |s| {
                s.text().chars().filter(char::is_ascii_digit).count() >= 4
            })?;
        } else if name == "Blackness" {
            s.wait("black screen", |s| {
                s.text().chars().all(char::is_whitespace)
            })?;
        }
        if name == "Flash-light" {
            for (width, height) in [(120, 40), (80, 25)] {
                s.resize(width, height)?;
                let viewport = format!("viewport=({width}, {height})");
                s.wait("flashlight resize committed", |s| {
                    fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
                        .is_ok_and(|state| state.contains(&viewport))
                })?;
            }
        }
        // F10 only wakes the preview; the original draft must remain open.
        s.send(b"\x1b[21~", "preview wakes without quitting", |s| {
            s.text().contains("Selected savers") && s.text().contains("Available savers")
        })?;
    }
    s.send(b"\x1b", "cancel draft", |s| s.command_prompt_active())?;
    s.finish()
}

#[test]
fn smartpad_notes_toggle_save_and_dirty_close_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("smartpad-notes")?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.send(b"\x1bq", "SmartPad opened", |s| {
        s.text().contains("SmartPad") && !s.command_prompt_active()
    })?;
    s.send("Заметка 界".as_bytes(), "Unicode notes", |s| {
        s.text().contains("Заметка 界")
    })?;
    s.send(b"\x1bq", "back to manager", |s| s.command_prompt_active())?;
    s.send(b"\x1bq", "same notes restored", |s| {
        !s.command_prompt_active() && s.text().contains("Заметка 界")
    })?;
    s.send(b"\x1b[15;5~", "SmartPad move", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[C\x1b[B\r", "SmartPad moved", |s| {
        !s.text().contains("Arrows move") && s.text().contains("SmartPad")
    })?;
    s.send(b"\x1b[12~", "save notes", |s| s.text().contains("Save As"))?;
    s.send(b"smartpad.txt\r", "notes saved", |s| {
        !s.text().contains("Save As") && s.text().contains("Saved")
    })?;
    assert_eq!(
        fs::read_to_string(s.root.join("smartpad.txt"))?,
        "Заметка 界"
    );
    s.send(b"!", "unsaved extra note", |s| {
        s.text().contains("Заметка 界!")
    })?;
    s.send(b"\x1b", "dirty close guard", |s| {
        s.text().contains("unsaved")
    })?;
    s.send(b"\x1b", "cancel close", |s| {
        !s.text().contains("unsaved") && s.text().contains("Заметка 界!")
    })?;
    s.send(b"\x1b", "dirty close again", |s| {
        s.text().contains("unsaved")
    })?;
    s.send(b"d", "discard only final edit", |s| {
        s.command_prompt_active()
    })?;
    assert_eq!(
        fs::read_to_string(s.root.join("smartpad.txt"))?,
        "Заметка 界"
    );
    s.finish()
}

#[test]
fn floating_smartpad_clock_is_not_duplicated_after_zoom_restore_and_idle() -> io::Result<()> {
    fn clocks(s: &Session) -> Vec<String> {
        s.text()
            .as_bytes()
            .windows(8)
            .filter(|b| {
                b[2] == b':'
                    && b[5] == b':'
                    && [0, 1, 3, 4, 6, 7]
                        .into_iter()
                        .all(|i| b[i].is_ascii_digit())
            })
            .map(|b| String::from_utf8(b.to_vec()).unwrap())
            .collect()
    }
    let mut s = Session::new("floating-editor-clock")?;
    s.wait("manager clock ready", |s| {
        s.command_prompt_active() && clocks(s).len() == 1
    })?;
    s.send(b"\x1bq", "floating SmartPad has one desktop clock", |s| {
        s.text().contains("SmartPad") && clocks(s).len() == 1
    })?;
    let before = clocks(&s);
    s.wait("idle clock redraw keeps one clock", |s| {
        let current = clocks(s);
        current.len() == 1 && current != before
    })?;
    s.send(b"\x1bz", "maximized editor retains its clock", |s| {
        s.text().contains("[↕]") && clocks(s).len() == 1
    })?;
    s.send(b"\x1bz", "restored editor has no duplicate clock", |s| {
        s.text().contains("[↑]") && !s.text().contains("[↕]") && clocks(s).len() == 1
    })?;
    s.send(b"\x1bq", "focus manager", |s| {
        s.command_prompt_active() && clocks(s).len() == 1
    })?;
    s.send(b"\x1bq", "focus SmartPad", |s| {
        !s.command_prompt_active() && s.text().contains("SmartPad") && clocks(s).len() == 1
    })?;
    s.send(b"\x1b", "close empty SmartPad", |s| {
        s.command_prompt_active()
    })?;
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
fn mp3_setup_time_and_track_lengths_in_real_terminal() -> io::Result<()> {
    let mut s =
        Session::with_preferences("mp3-enhancements", "[plugins]\nenabled=['mp3_player']\n")?;
    fs::write(
        s.root.join("duration.mp3"),
        include_bytes!("fixtures/mp3/tone.mp3"),
    )?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.select_file("duration.mp3")?;
    s.send(b"\x1bua", "import duration", |s| {
        s.text().contains("Music Player")
            && !s.text().contains("Adding music files")
            && s.plugin_document_ready()
    })?;
    s.send(b"\x1bz", "expand", |s| s.text().contains("Settings…"))?;
    music_click(&mut s, "Settings…", "setup", |s| {
        s.text().contains("Seek step (seconds)")
    })?;
    s.send(b"\t\t\t \t\t\r", "volume draft", |s| {
        s.text().contains("60%")
    })?;
    s.send(b"\t\r", "apply setup", |s| {
        !s.text().contains("Seek step (seconds)") && s.text().contains("60%")
    })?;
    music_click(&mut s, "Elapsed", "remaining timing", |s| {
        s.text().contains("Remaining")
    })?;
    s.send(b"\r", "elapsed timing", |s| s.text().contains("Elapsed"))?;
    music_click(&mut s, "Playlist…", "playlist length", |s| {
        s.text().contains("Music queue · current session") && s.text().contains("duration.mp3")
    })?;
    assert!(!s.text().contains("--:--"), "{}", s.text());
    for (width, height) in [(132, 40), (80, 25)] {
        s.resize(width, height)?;
        s.wait("playlist resize acknowledged", |s| {
            s.text().contains("Music queue · current session")
                && fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
                    .is_ok_and(|d| d.contains(&format!("viewport=({width}, {height})")))
        })?;
    }
    s.send(b"\x1b", "cancel playlist", |s| {
        !s.text().contains("Music queue · current session")
    })?;
    s.send(b"\x1b", "close player", |s| s.command_prompt_active())?;
    s.finish()
}

fn music_fixture_queue(s: &mut Session) -> io::Result<()> {
    fs::create_dir(s.root.join("mixed"))?;
    for (name, bytes) in [
        (
            "a.flac",
            include_bytes!("fixtures/music/tone.flac").as_slice(),
        ),
        (
            "b.wav",
            include_bytes!("fixtures/music/tone.wav").as_slice(),
        ),
        (
            "c.ogg",
            include_bytes!("fixtures/music/tone.ogg").as_slice(),
        ),
        ("d.mp3", include_bytes!("fixtures/mp3/tone.mp3").as_slice()),
    ] {
        fs::write(s.root.join("mixed").join(name), bytes)?;
    }
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.select_file("mixed")?;
    s.send(b"\x1bua", "import mixed music", |s| {
        s.text().contains("Music Player")
            && !s.text().contains("Adding music files")
            && s.plugin_document_ready()
    })?;
    s.send(b"\x1bz", "expand music player", |s| {
        s.text().contains("FLAC") && s.text().contains("Volume")
    })
}
#[test]
fn music_mixed_queue_metadata_and_new_menu_mnemonic_restore_terminal() -> io::Result<()> {
    let mut s = Session::with_preferences("music-mixed", "[plugins]\nenabled=['mp3_player']\n")?;
    music_fixture_queue(&mut s)?;
    music_click(&mut s, "Playlist…", "mixed queue", |s| {
        s.text().contains("Music queue · current session") && s.text().contains("Vorbis title")
    })?;
    let text = s.text();
    assert!(text.contains("FLAC") && text.contains("WAV title") && text.contains("d.mp3"));
    assert_eq!(text.matches("00:12").count(), 4);
    s.send(b"\x1b", "close queue", |s| {
        !s.text().contains("Music queue · current session") && s.text().contains("Volume")
    })?;
    s.send(b"\x1b", "close player", |s| s.command_prompt_active())?;
    s.send(b"\x1bus", "new Music Player mnemonic", |s| {
        s.text().contains("Music Player") && !s.command_prompt_active()
    })?;
    s.send(b"\x1b", "close empty player", |s| s.command_prompt_active())?;
    s.finish()
}
#[test]
#[ignore = "requires a working macOS audio output device; plays quiet synthetic tones"]
fn music_mixed_audio_play_seek_next_and_channel_changes_restore_terminal() -> io::Result<()> {
    let mut s =
        Session::with_preferences("music-mixed-audio", "[plugins]\nenabled=['mp3_player']\n")?;
    music_fixture_queue(&mut s)?;
    s.send(b"\r", "play FLAC", |s| {
        s.text().contains("Playing") && s.text().contains("FLAC")
    })?;
    // All formats use the same output/transport. Next must replace the decoder
    // across stereo 24-bit/48k FLAC, mono 16-bit/44.1k WAV, stereo Vorbis and MP3.
    for title in ["WAV title", "Vorbis title", "d.mp3"] {
        let text = s.text();
        let (row, col) = text
            .lines()
            .enumerate()
            .find_map(|(row, line)| {
                line.find("►►")
                    .map(|byte| (row, line[..byte].chars().count()))
            })
            .unwrap();
        let mouse = format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            col + 1,
            row + 1,
            col + 1,
            row + 1
        );
        s.send(mouse.as_bytes(), "next music format", |s| {
            s.text().contains("Playing") && s.text().contains(title)
        })?;
        s.send(b"\x1b[Z\r", "seek current format", |s| {
            s.text().contains("Playing")
                && (5..=8).any(|n| s.text().contains(&format!("00:{n:02}")))
        })?;
    }
    s.send(b"\x1b", "close playing music", |s| {
        s.command_prompt_active()
    })?;
    s.finish()
}

#[test]
fn archive_creation_and_password_preview_extraction_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("archive-passwords")?;
    s.wait("initial manager", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1bfh", "plain ZIP creation form", |s| {
        s.text().contains("Create archive") && s.text().contains("Confirm password")
    })?;
    s.send(b"\r", "plain archive completes", |s| {
        s.text().contains("Transfer Center")
            && s.text().matches("· Completed ·").count() == 1
            && s.root.join("item-000.txt.zip").exists()
    })?;
    let catalog = dn::archive::reader::list(
        s.root.join("item-000.txt.zip"),
        &dn::task::Cancellation::default(),
    )?;
    assert!(!catalog.nodes[1].locked);
    s.send(b"\x1b", "return after plain packing", |s| {
        s.command_prompt_active()
    })?;
    s.select_file("item-001.txt")?;
    s.send(b"\x1bfh", "encrypted ZIP creation form", |s| {
        s.text().contains("Create archive") && s.text().contains("Confirm password")
    })?;
    s.send(
        b"\tfixture-password\tfixture-password",
        "masked creation password",
        |s| s.text().contains("****************") && !s.text().contains("fixture-password"),
    )?;
    s.send(b"\r", "encrypted archive completes", |s| {
        s.text().contains("Transfer Center")
            && s.text().matches("· Completed ·").count() == 2
            && s.root.join("item-001.txt.zip").exists()
    })?;
    s.send(b"\x1b", "return after encrypted packing", |s| {
        s.command_prompt_active()
    })?;
    // Packing refreshes both panes asynchronously. Explicitly finish a fresh
    // listing before opening a filename search for the newly published output.
    s.send(b"\x12", "refresh packed output", |s| {
        s.command_prompt_active()
            && fs::read_to_string(s.root.join("settings/dn/diagnostics.txt")).is_ok_and(|d| {
                d.lines()
                    .any(|line| line == "panels_loading=[false, false]")
            })
    })?;
    s.select_file("item-001.txt.zip")?;
    s.send(b"\r", "encrypted archive pane", |s| {
        s.command_prompt_active() && s.text().contains(" :: /") && s.text().contains("item-001.txt")
    })?;
    s.select_file("item-001.txt")?;
    s.send(b"\x1b[13~", "preview password prompt", |s| {
        s.text().contains("Archive password")
    })?;
    s.send(b"wrong\r", "wrong password remains retryable", |s| {
        s.text().contains("Incorrect archive password")
    })?;
    s.resize(100, 32)?;
    s.wait("password prompt after resize", |s| {
        s.text().contains("Archive password") && s.text().contains("Incorrect archive password")
    })?;
    s.send(b"fixture-password\r", "authenticated preview", |s| {
        s.text().contains("fixture") && s.text().contains("F2 Wrap")
    })?;
    s.send(b"\x1b", "return from encrypted preview", |s| {
        s.command_prompt_active() && s.text().contains(" :: /")
    })?;
    s.send(b"\x12", "refresh forgets archive password", |s| {
        s.command_prompt_active()
            && s.text().contains("item-001.txt")
            && !s.text().contains("Reading directory")
    })?;
    s.select_file("item-001.txt")?;
    s.send(b"\x1b[15~", "encrypted extraction copy form", |s| {
        s.text().contains("Extract 1 selected archive")
    })?;
    s.send(b"unpacked.txt\r", "extraction password prompt", |s| {
        s.text().contains("Archive password")
    })?;
    s.send(
        b"fixture-password\r",
        "encrypted extraction completes",
        |s| {
            s.text().contains("Transfer Center")
                && s.text().matches("· Completed ·").count() == 3
                && s.root.join("unpacked.txt").exists()
        },
    )?;
    assert_eq!(fs::read(s.root.join("unpacked.txt"))?, b"fixture\n");
    assert_eq!(fs::read(s.root.join("item-001.txt"))?, b"fixture\n");
    s.send(b"\x1b", "return after extraction", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1b[5;5~", "leave encrypted archive", |s| {
        s.command_prompt_active() && !s.text().contains(" :: /")
    })?;
    s.finish()
}

#[test]
fn markdown_viewer_and_unsaved_editor_preview_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("markdown-preview")?;
    let path = s.root.join("a.md");
    fs::write(&path,"# Markdown heading\n\n**formatted** paragraph\n\n| Name | Value |\n|---|---|\n| DN | 42 |\n\n```rust\nfn main() {}\n```\n")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x12", "refresh Markdown fixture", |s| {
        s.command_prompt_active() && s.text().contains("a.md")
    })?;
    s.select_file("a.md")?;
    s.send(b"\x1b[13~", "formatted Markdown viewer", |s| {
        s.text().contains("Markdown heading")
            && s.text().contains("F6 Source")
            && !s.text().contains("**formatted**")
    })?;
    s.send(b"\x1b[17~", "Markdown source mode", |s| {
        s.text().contains("**formatted**") && s.text().contains("F6 Markdown")
    })?;
    s.send(b"\x1b[17~", "return to formatted view", |s| {
        s.text().contains("F6 Source") && !s.text().contains("**formatted**")
    })?;
    s.resize(120, 35)?;
    s.wait("Markdown resize", |s| {
        s.text().contains("Markdown heading") && s.text().contains("F6 Source")
    })?;
    s.send(b"\x1b", "close Markdown viewer", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1b[14~", "Markdown source editor", |s| {
        s.text().contains("# Markdown heading") && s.text().contains("F2 Save")
    })?;
    s.send(
        b"\x1b[200~UNSAVED \x1b[201~",
        "unsaved Markdown edit",
        |s| s.text().contains("UNSAVED # Markdown"),
    )?;
    s.send(b"\x1b[15~", "unsaved-buffer Markdown preview", |s| {
        s.text().contains("UNSAVED # Markdown")
            && s.text().contains("F5 Editor")
            && s.text().contains("F6 Source")
    })?;
    assert!(!fs::read_to_string(&path)?.contains("UNSAVED"));
    s.send(b"\x1b[15~", "return to unsaved editor", |s| {
        s.text().contains("F2 Save") && s.text().contains("UNSAVED # Markdown")
    })?;
    s.send(b"\x1a", "undo remains available after preview", |s| {
        s.text().contains("F2 Save") && !s.text().contains("UNSAVED")
    })?;
    s.send(b"\x1b[15~", "preview refresh after undo", |s| {
        s.text().contains("F5 Editor")
            && s.text().contains("F6 Source")
            && !s.text().contains("UNSAVED")
    })?;
    s.send(b"\x1b", "Esc returns to the source editor", |s| {
        s.text().contains("F2 Save")
    })?;
    s.send(b"\x1b", "close edited source", |s| {
        s.command_prompt_active() || s.text().contains("Save changes")
    })?;
    if !s.command_prompt_active() {
        s.send(b"d", "discard temporary edit", |s| {
            s.command_prompt_active()
        })?;
    }
    assert!(!fs::read_to_string(&path)?.contains("UNSAVED"));
    s.finish()
}
