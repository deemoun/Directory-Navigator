#![cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::{fs, io};

#[cfg(target_os = "linux")]
#[test]
fn linux_pty_unicode_diagnostics_do_not_treat_linux_term_as_a_console() -> io::Result<()> {
    let mut s = Session::with_terminal_preferences("unicode-pty-diagnostics", "", "linux")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x1buy", "System Information", |s| {
        s.text().contains("System Information") && s.text().contains("Architecture")
    })?;
    s.send(b"\x1b[F", "Unicode and console diagnostics", |s| {
        s.text().contains("No virtual console detected")
            && s.text().contains("Extended colors (indexed / RGB)")
            && s.text()
                .contains("Not applicable outside a virtual console")
            && s.text().contains("Not verified")
    })?;
    s.send(b"\x1b[15~", "refresh read-only diagnostics", |s| {
        s.text().contains("Architecture") && !s.text().contains("Loading native information")
    })?;
    s.send(b"\x1b[F", "repeat PTY font status", |s| {
        s.text()
            .contains("Not applicable outside a virtual console")
    })?;
    s.send(b"\x1b", "close diagnostics", |s| s.command_prompt_active())?;
    s.finish()
}

#[test]
fn cyrillic_paths_search_view_edit_and_pty_with_linux_term_preserve_utf8() -> io::Result<()> {
    let mut s = Session::with_terminal_preferences("cyrillic-path", "", "linux")?;
    let name = "Ёжик-е\u{308}.txt";
    let original = "Привет, Ёжик! Mix е\u{308}\n";
    fs::write(s.root.join(name), original)?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x12", "refresh Unicode file", |s| {
        s.command_prompt_active()
    })?;
    s.select_file(name)?;
    s.send(b"\x1b[13~", "Cyrillic viewer", |s| {
        s.text().contains("Привет, Ёжик! Mix е\u{308}")
    })?;
    s.send(b"\x1b[18~", "viewer search", |s| s.text().contains("Find"))?;
    s.send("Ёжик\r".as_bytes(), "Cyrillic search", |s| {
        !s.text().contains("Viewer search") && s.text().contains("Привет, Ёжик!")
    })?;
    s.send(b"\x1b", "return to Unicode file", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1b[14~", "Cyrillic editor", |s| {
        s.text().contains("F2 Save") && s.text().contains("Привет, Ёжик! Mix е\u{308}")
    })?;
    s.paste("Ёё ", "Unicode editing", |s| {
        s.text().contains("Ёё Привет, Ёжик!")
    })?;
    s.send(b"\x1bOQ", "save Unicode source", |s| {
        fs::read_to_string(s.root.join(name)).is_ok_and(|text| text == format!("Ёё {original}"))
    })?;
    s.send(b"\x1b", "close Unicode editor", |s| {
        s.command_prompt_active() && !s.text().contains("F2 Save")
    })?;
    assert_eq!(
        fs::read(s.root.join(name))?,
        format!("Ёё {original}").as_bytes()
    );
    assert!(
        s.root.join(name).is_file(),
        "decomposed native path must remain unchanged"
    );
    s.finish()
}

#[cfg(feature = "audio")]
fn id3(utf16: bool, title: &str, artist: &str) -> Vec<u8> {
    let version = if utf16 { 3 } else { 4 };
    let syncsafe = |n: usize| {
        [
            ((n >> 21) & 127) as u8,
            ((n >> 14) & 127) as u8,
            ((n >> 7) & 127) as u8,
            (n & 127) as u8,
        ]
    };
    let mut body = Vec::new();
    for (key, text) in [(b"TIT2", title), (b"TPE1", artist)] {
        let data = if utf16 {
            [
                vec![1, 255, 254],
                text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            ]
            .concat()
        } else {
            [vec![3], text.as_bytes().to_vec()].concat()
        };
        body.extend(key);
        body.extend(if utf16 {
            (data.len() as u32).to_be_bytes()
        } else {
            syncsafe(data.len())
        });
        body.extend([0, 0]);
        body.extend(data);
    }
    let mut bytes = vec![b'I', b'D', b'3', version, 0, 0];
    bytes.extend(syncsafe(body.len()));
    bytes.extend(body);
    bytes
}

#[cfg(feature = "audio")]
fn click(
    s: &mut Session,
    label: &str,
    stage: &str,
    ready: impl Fn(&Session) -> bool,
) -> io::Result<()> {
    let (x, y) = s
        .text()
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            let x = line.find(label)?;
            Some((unicode_width::UnicodeWidthStr::width(&line[..x]) + 1, y + 1))
        })
        .expect("player control");
    s.send(
        format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes(),
        stage,
        ready,
    )
}

#[cfg(feature = "audio")]
#[test]
fn cyrillic_music_metadata_filename_fallback_and_player_switching_without_audio() -> io::Result<()>
{
    let mut s = Session::with_preferences("cyrillic-music", "[plugins]\nenabled=['mp3_player']\n")?;
    fs::create_dir(s.root.join("music"))?;
    for (name, data) in [
        (
            "01-Ёжик.mp3",
            id3(false, "Ёжик Mix е\u{308}", "Группа Ёлка"),
        ),
        ("02-Песня.mp3", id3(true, "Песня Ёё", "Исполнитель")),
        ("03-Без-тегов-Ё.mp3", Vec::new()),
    ] {
        fs::write(s.root.join("music").join(name), data)?;
    }
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x12", "refresh music folder", |s| {
        s.text().contains("music")
    })?;
    s.select_file("music")?;
    s.send(b"\x1bua", "import Cyrillic music without playback", |s| {
        s.text().contains("Music Player")
            && s.plugin_document_ready()
            && !s.text().contains("Adding music files")
    })?;
    s.send(b"\x1bz", "expanded Cyrillic metadata", |s| {
        s.text().contains("Группа Ёлка") && s.text().contains("Ёжик Mix е\u{308}")
    })?;
    click(&mut s, "Info…", "Cyrillic track Info", |s| {
        s.text().contains("Music Track Info") && s.text().contains("01-Ёжик.mp3")
    })?;
    s.send(b"\x1b", "close track Info", |s| {
        !s.text().contains("Music Track Info") && s.plugin_document_ready()
    })?;
    click(&mut s, "Playlist…", "Cyrillic playlist", |s| {
        s.text().contains("Music queue · current session")
            && s.text().contains("Песня Ёё")
            && s.text().contains("03-Без-тегов-Ё.mp3")
    })?;
    s.send(b"\x1b", "close playlist", |s| {
        !s.text().contains("Music queue · current session") && s.plugin_document_ready()
    })?;
    s.send(b"\x1bz", "compact preserves Unicode queue", |s| {
        s.text().contains("Music Player") && !s.text().contains("Settings…")
    })?;
    s.send(b"\x1bz", "expanded preserves Unicode metadata", |s| {
        s.text().contains("Группа Ёлка") && s.text().contains("Ёжик Mix е\u{308}")
    })?;
    s.resize(100, 32)?;
    s.wait("Unicode player after resize", |s| {
        s.text().contains("Ёжик Mix е\u{308}")
    })?;
    s.send(b"\x1b", "close player without audio", |s| {
        s.command_prompt_active()
    })?;
    assert_eq!(
        fs::read(s.root.join("music/01-Ёжик.mp3"))?,
        id3(false, "Ёжик Mix е\u{308}", "Группа Ёлка")
    );
    s.finish()
}
