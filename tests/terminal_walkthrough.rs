#![cfg(target_os = "macos")]
#[path = "support/apk.rs"]
mod apk_fixture;
#[path = "support/pdf.rs"]
mod pdf_fixture;
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::{fs, io};

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
#[cfg(feature = "audio")]
fn music_repeat(s: &Session, value: &str) -> bool {
    s.text()
        .lines()
        .any(|line| line.contains("Repeat:") && line.contains(value))
}

#[test]
#[cfg(feature = "audio")]
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
#[cfg(feature = "audio")]
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
        s.text().contains("Music Player")
            && !s.text().contains("Adding music files")
            && s.plugin_document_ready()
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
        !s.text().contains("Music queue · current session") && s.plugin_document_ready()
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
    // SIGWINCH can flush a key sent before the resized desktop is committed.
    s.wait("player resize committed", |s| {
        s.text()
            .lines()
            .last()
            .is_some_and(|line| line.contains("F10 Quit"))
            && s.text().contains("Volume")
    })?;
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
#[ignore = "requires a working macOS audio output device; plays a quiet synthetic tone"]
#[cfg(feature = "audio")]
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
#[cfg(feature = "audio")]
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
#[cfg(feature = "audio")]
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

#[cfg(feature = "audio")]
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
#[cfg(feature = "audio")]
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
#[cfg(feature = "audio")]
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

#[test]
fn apk_workshop_inspect_project_patch_export_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::with_preferences("apk-workshop", "[plugins]\nenabled=['apk_workshop']\n")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    apk_fixture::write(&s.root.join("fixture.apk"))?;
    s.send(b"\x12", "refresh APK fixture", |s| {
        s.text().contains("fixture.apk") && s.command_prompt_active()
    })?;
    s.select_file("fixture.apk")?;
    s.send(b"\r", "Enter opens APK Workshop", |s| {
        s.text().contains("org.dn.fixture") && s.text().contains("Overview")
    })?;
    music_click(&mut s, "Files", "APK files", |s| {
        s.text().contains("Extract…") && s.text().contains("resources.arsc")
    })?;
    music_click_row(&mut s, "assets", None, "select assets", |s| {
        s.text().contains("assets")
    })?;
    s.send(b"\r", "APK folder", |s| s.text().contains("config.txt"))?;
    music_click(&mut s, "config.txt", "select APK member", |s| {
        s.text().contains("config.txt")
    })?;
    s.send(b"\x1b[13~", "APK member viewer", |s| {
        s.text().contains("original config")
    })?;
    s.send(b"\x1b", "back to APK", |s| {
        s.text().contains("APK Workshop") && s.text().contains("config.txt")
    })?;
    music_click(&mut s, "Project…", "new APK project picker", |s| {
        s.text().contains("Save As")
    })?;
    // Keep long absolute Unicode paths covered even with a short TMPDIR.
    let destinations = fs::canonicalize(&s.root)?
        .join("paths")
        .join(format!("{}界", "long-".repeat(18)));
    fs::create_dir_all(&destinations)?;
    let project = destinations.join("project");
    s.paste(project.to_str().unwrap(), "project name", |s| {
        s.text().contains("project")
    })?;
    s.send(b"\r", "APK project created", |s| {
        project.join("apk-project.toml").exists() && s.text().contains("Work dir")
    })?;
    music_click(&mut s, "Edit", "manifest edit", |s| {
        s.text().contains("Debuggable (true/false)")
    })?;
    s.send(
        b"\x1b[H\x1b[3~\x1b[3~\x1b[3~\x1b[3~\x1b[3~true",
        "enable debuggable",
        |s| s.text().contains("true"),
    )?;
    music_click(&mut s, "OK", "apply manifest", |s| {
        s.text().contains("Debuggable: true")
    })?;
    music_click(&mut s, "Reset", "confirm manifest reset", |s| {
        s.text().contains("Discard changes to this file?")
    })?;
    s.send(b"\r", "default Cancel keeps manifest edit", |s| {
        !s.text().contains("Discard changes to this file?") && s.text().contains("Debuggable: true")
    })?;
    music_click(&mut s, "Export…", "export APK picker", |s| {
        s.text().contains("Save As")
    })?;
    let output = destinations.join("patched.apk");
    s.paste(output.to_str().unwrap(), "APK output path", |s| {
        s.text().contains("patched.apk")
    })?;
    s.send(b"\r", "APK exported", |s| {
        output.exists() && s.text().contains("APK exported successfully")
    })?;
    assert!(
        dn::apk::Package::open(output, &Default::default())?
            .manifest
            .debuggable
    );
    s.resize(132, 40)?;
    s.wait("APK resize", |s| {
        s.text().contains("APK Workshop") && s.text().contains("Export…")
    })?;
    s.resize(80, 25)?;
    s.wait("APK resize restored", |s| s.text().contains("Work dir"))?;
    s.send(b"\x1b", "close APK Workshop", |s| s.command_prompt_active())?;
    assert!(
        !dn::apk::Package::open(s.root.join("fixture.apk"), &Default::default())?
            .manifest
            .debuggable
    );
    s.finish()
}

#[test]
fn encoding_choices_conversion_and_cancel_restore_terminal() -> io::Result<()> {
    use dn::text_encoding::TextEncoding as C;
    let mut s = Session::new("text-encodings")?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    let path = s.root.join("item-000.txt");
    let bytes = C::Windows1251
        .default_format()
        .encode("Привет\r\nстрока\n")
        .unwrap();
    fs::write(&path, &bytes)?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1bOR", "viewer encoding picker", |s| {
        s.text().contains("Open with encoding")
    })?;
    s.send(b"\x1b[B", "Windows-1251 preview", |s| {
        s.text().contains("Привет")
    })?;
    s.send(b"\r", "decoded viewer", |s| {
        s.text().contains("Windows-1251") && !s.text().contains("Open with encoding")
    })?;
    s.send(b"\x1b[19~", "viewer F8", |s| {
        s.text().contains("Open with encoding")
    })?;
    s.send(b"\x1b", "cancel reinterpretation", |s| {
        !s.text().contains("Open with encoding")
    })?;
    s.send(b"\x1b", "close viewer", |s| s.command_prompt_active())?;
    s.send(b"\x1bOS", "editor encoding picker", |s| {
        s.text().contains("Open with encoding")
    })?;
    s.send(b"\x1b[B", "editor Windows-1251 preview", |s| {
        s.text().contains("Привет")
    })?;
    s.send(b"\r", "decoded editor", |s| {
        s.text().contains("F2 Save")
            && s.text().contains("Windows-1251")
            && !s.text().contains("Open with encoding")
    })?;
    s.send(b"\x1b[21~", "editor File menu", |s| {
        s.text().contains("Save with encodin")
    })?;
    s.send(b"g", "save encoding picker", |s| {
        s.text().contains("Save with encoding") && s.text().contains("Write BOM")
    })?;
    s.send(b"\x1b[A\x1b[A\x1b[A", "choose UTF-16 LE", |s| {
        s.text().contains("Write BOM")
    })?;
    s.send(b"\r", "converted editor", |s| {
        s.text().contains("UTF-16 LE + BOM") && !s.text().contains("Save with encoding")
    })?;
    assert_eq!(
        fs::read(&path)?,
        C::Utf16Le
            .default_format()
            .encode("Привет\r\nстрока\n")
            .unwrap()
    );
    s.resize(100, 32)?;
    s.wait("converted editor resize committed", |s| {
        fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
            .is_ok_and(|state| state.contains("viewport=(100, 32)"))
            && s.text().contains("UTF-16 LE + BOM")
    })?;
    s.send(b"\x1b", "close converted editor", |s| {
        s.command_prompt_active()
    })?;
    s.finish()
}

#[test]
fn pdf_text_pages_search_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::new("pdf-text")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    fs::write(
        s.root.join("readme.pdf"),
        pdf_fixture::bytes(&[b"first PDF page", b"second PDF page unique-search"])?,
    )?;
    s.send(b"\x12", "refresh PDF fixture", |s| {
        s.command_prompt_active()
    })?;
    s.select_file("readme.pdf")?;
    s.send(b"\x1b[13~", "PDF text viewer", |s| {
        s.text().contains("PDF")
            && s.text().contains("first PDF page")
            && s.text().contains("Page 1/2")
    })?;
    s.send(b"\x07", "PDF page navigation", |s| {
        s.text().contains("Page number (from 1)")
    })?;
    s.send(b"2\r", "PDF second page", |s| {
        !s.text().contains("Viewer Go to") && s.text().contains("Page 2/2")
    })?;
    s.send(b"\x1b[18~", "PDF text search", |s| {
        s.text().contains("Find")
    })?;
    s.send(b"unique-search\r", "found PDF text", |s| {
        !s.text().contains("Viewer search") && s.text().contains("unique-search")
    })?;
    s.resize(100, 32)?;
    s.wait("PDF resize", |s| {
        s.text().contains("PDF") && s.text().contains("unique-search")
    })?;
    s.send(b"\x1b", "close PDF", |s| {
        s.command_prompt_active() && !s.text().contains("F3 Viewer")
    })?;
    s.finish()
}

#[test]
fn pdf_password_retry_help_cancel_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::new("pdf-password")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    let password = "unique-PDF-secret";
    let bytes = pdf_fixture::encrypted_modern(&[b"authenticated PDF body"], password)?;
    fs::write(s.root.join("private.pdf"), &bytes)?;
    s.send(b"\x12", "refresh encrypted PDF", |s| {
        s.command_prompt_active()
    })?;
    s.select_file("private.pdf")?;
    s.send(b"\x1b[13~", "PDF password prompt", |s| {
        s.text().contains("PDF password") && s.text().contains("Password")
    })?;
    s.send(b"bad-guess", "masked wrong password", |s| {
        s.text().contains("*********")
    })?;
    assert!(!s.text().contains("bad-guess"));
    s.send(b"\r", "incorrect password retry", |s| {
        s.text().contains("Incorrect PDF password. Try again.")
    })?;
    s.resize(100, 32)?;
    s.wait("resized PDF password prompt", |s| {
        s.text().contains("PDF password")
    })?;
    s.send(b"\x1bOP", "PDF password Help", |s| {
        s.text().contains("PDF password help")
    })?;
    s.send(b"\x1b", "return to password field", |s| {
        s.text().contains("PDF password") && !s.text().contains("PDF password help")
    })?;
    s.paste(password, "masked correct password", |s| {
        s.text().contains("*****************")
    })?;
    assert!(!s.text().contains(password));
    let diagnostics = fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))?;
    assert!(!diagnostics.contains(password));
    assert!(!diagnostics.contains("bad-guess"));
    s.send(b"\r", "authenticated PDF text", |s| {
        s.text().contains("authenticated PDF body") && !s.text().contains("PDF password")
    })?;
    s.send(b"\x1b", "close unlocked PDF", |s| {
        s.command_prompt_active() && !s.text().contains("F3 Viewer")
    })?;
    s.send(b"\x1b[13~", "reopened PDF asks again", |s| {
        s.text().contains("PDF password")
    })?;
    s.send(b"\x1b", "cancel PDF password", |s| {
        s.command_prompt_active()
            && !s.text().contains("PDF password")
            && !s.text().contains("F3 Viewer")
    })?;
    assert_eq!(fs::read(s.root.join("private.pdf"))?, bytes);
    s.finish()
}
