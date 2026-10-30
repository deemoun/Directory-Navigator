#![cfg(target_os = "macos")]
#[path = "support/pty.rs"]
mod pty;
use pty::Session;
use std::{fs, io};

#[test]
fn settings_and_unicode_prompt_workflow_restores_terminal_and_preserves_cancelled_drafts(
) -> io::Result<()> {
    let mut s = Session::new("controllers")?;
    s.wait("manager ready", |s| {
        s.text().contains("item-000.") && s.text().contains("F5 Copy")
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
        s.text().contains("item-000.") && s.text().contains("F5 Copy")
    })?;
    s.send(b"\x1b[B\x1b[B\x1b[B\x1b[14~", "editor opened", |s| {
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
        s.text().contains("F5 Copy")
    })?;
    s.send(b"\x1b[B\x1b[13~", "viewer opened", |s| {
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
    // Activate exposed manager background explicitly. An Escape followed by
    // an Alt sequence can be combined by the terminal key decoder under load.
    s.send(
        b"\x1b[<0;79;22M\x1b[<0;79;22m\x1b[20;3~",
        "game opened",
        |s| s.text().contains("Navigator's game"),
    )?;
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
    s.send(b"\x1b", "close game", |s| s.text().contains("F5 Copy"))?;
    s.send(b"\x1bOP", "native help", |s| s.text().contains("Contents"))?;
    s.send(b"\x1b[15;5~", "resize help", |s| {
        s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b[1;2C\r", "accept help size", |s| {
        !s.text().contains("Arrows move")
    })?;
    s.send(b"\x1b", "close help", |s| !s.text().contains("Contents"))?;
    let exits = s.occurrences(b"\x1b[?1049l");
    s.send(b"\x0f", "output handoff", |s| {
        s.occurrences(b"\x1b[?1049l") > exits
    })?;
    let enters = s.occurrences(b"\x1b[?1049h");
    s.send(b"\r", "return to desktop", |s| {
        s.occurrences(b"\x1b[?1049h") > enters && s.text().contains("F5 Copy")
    })?;
    s.resize(100, 32)?;
    s.wait("resize redraw", |s| s.text().contains("F5 Copy"))?;
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
        s.text().contains("item-000.") && s.text().contains("F5 Copy")
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
        s.text().contains("Default colors set")
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
        s.text().contains("item-000.") && s.text().contains("F5 Copy")
    })?;
    s.send(b"\x1b[B\x1b[B\x1b[B\x1b[14~", "editor opened", |s| {
        s.text().contains("F2 Save")
    })?;
    s.send(b"\x1bz", "editor restored", |s| {
        s.text().contains("F5 Copy")
    })?;
    s.send(
        b"\x1b[<0;1;2M\x1b[<0;1;2m\x1b[B\x1b[13~",
        "viewer opened",
        |s| s.text().contains("Viewer"),
    )?;
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
    s.send(b"\x1b[B\x1b[B\x1b[B\x1b[14~", "editor opened", |s| {
        s.text().contains("F2 Save")
    })?;
    s.send(b"\x1bz", "editor restored", |s| {
        s.text().contains("F5 Copy")
    })?;
    s.send(b"\x1b[<0;1;2M\x1b[<0;1;2m\x1b[20;3~", "game opened", |s| {
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
