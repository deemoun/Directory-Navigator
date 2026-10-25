#![cfg(feature = "turbo-vision")]
use dn::turbo::TurboBackend;
use ratatui::{
    backend::Backend,
    buffer::Cell,
    layout::{Position, Size},
    style::{Color, Modifier},
};
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};
#[derive(Clone, Default)]
struct Output(Arc<Mutex<Vec<u8>>>);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Output {
    fn take(&self) -> String {
        String::from_utf8(std::mem::take(&mut *self.0.lock().unwrap())).unwrap()
    }
}
fn cell(text: &str) -> Cell {
    let mut c = Cell::default();
    c.set_symbol(text)
        .set_fg(Color::Indexed(87))
        .set_bg(Color::Indexed(240));
    c
}
#[test]
fn native_view_transport_preserves_graphemes_wide_cells_private_use_and_indexed_palette(
) -> io::Result<()> {
    let output = Output::default();
    let mut backend = TurboBackend::with_size(output.clone(), Size::new(40, 4))?;
    let cells = [
        (0, 0, cell("e\u{301}")),
        (2, 0, cell("界")),
        (5, 0, cell("👩‍💻")),
        (9, 0, cell("\u{f0000}")),
        (12, 0, cell("\u{100000}")),
    ];
    backend.draw(cells.iter().map(|(x, y, c)| (*x, *y, c)))?;
    backend.set_cursor_position(Position::new(17, 2))?;
    backend.flush()?;
    let text = output.take();
    for symbol in ["e\u{301}", "界", "👩‍💻", "\u{f0000}", "\u{100000}"] {
        assert!(text.contains(symbol), "{symbol:?}: {text:?}");
    }
    assert!(text.contains("38;5;87;48;5;240"));
    assert!(!text.contains("38;2;254;"), "color tokens leaked");
    assert!(
        text.ends_with("\x1b[3;18H\x1b[?25h"),
        "cursor moved by drawing: {text:?}"
    );
    Ok(())
}
#[test]
fn changed_combining_sequences_never_reuse_live_tokens_or_leave_previous_width() -> io::Result<()> {
    let output = Output::default();
    let mut backend = TurboBackend::with_size(output.clone(), Size::new(8, 2))?;
    for i in 0..1000 {
        let symbol = if i % 2 == 0 { "e\u{301}" } else { "e\u{300}" };
        let c = cell(symbol);
        backend.draw([(0, 0, &c)].into_iter())?;
        backend.flush()?;
        let text = output.take();
        assert!(
            text.contains(symbol),
            "change suppressed on frame {i}: {text:?}"
        );
    }
    let wide = cell("界");
    backend.draw([(0, 0, &wide)].into_iter())?;
    backend.flush()?;
    output.take();
    let narrow = cell("x");
    let blank = cell(" ");
    backend.draw([(0, 0, &narrow), (1, 0, &blank)].into_iter())?;
    backend.flush()?;
    let text = output.take();
    assert!(text.contains("x "));
    backend.resize(Size::new(80, 25))?;
    backend.clear()?;
    let c = cell("e\u{301}");
    backend.draw([(0, 0, &c)].into_iter())?;
    backend.flush()?;
    assert!(output.take().contains("e\u{301}"));
    Ok(())
}
#[test]
fn terminal_theme_indices_rgb_default_colors_and_styles_survive_transport() -> io::Result<()> {
    let output = Output::default();
    let mut backend = TurboBackend::with_size(output.clone(), Size::new(32, 2))?;
    let mut cells: Vec<_> = (0..16)
        .map(|n| {
            let mut c = cell("x");
            c.fg = Color::Indexed(n);
            c.bg = Color::Indexed(15 - n);
            c
        })
        .collect();
    let mut rgb = cell("R");
    rgb.fg = Color::Rgb(254, 0, 0);
    rgb.bg = Color::Reset;
    rgb.modifier = Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED | Modifier::REVERSED;
    cells.push(rgb);
    backend.draw(cells.iter().enumerate().map(|(x, c)| (x as u16, 0, c)))?;
    backend.flush()?;
    let text = output.take();
    for n in 0..16 {
        assert!(text.contains(&format!("38;5;{n};48;5;{}", 15 - n)));
    }
    assert!(text.contains("38;2;254;0;0;49;1;3;4;7m"), "{text:?}");
    assert!(backend.resize(Size::new(u16::MAX, 25)).is_err());
    Ok(())
}

/// Small independent ANSI screen reader for the transport contract. It consumes
/// only the absolute cursor/SGR/clear operations emitted by TV, and groups whole
/// graphemes rather than duplicating TV's scalar-cell representation.
fn screen(bytes: &str, size: Size) -> Vec<String> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut cells = vec![" ".to_owned(); usize::from(size.width) * usize::from(size.height)];
    let (mut x, mut y) = (0usize, 0usize);
    let mut rest = bytes;
    while !rest.is_empty() {
        if let Some(sequence) = rest.strip_prefix("\x1b[") {
            let end = sequence
                .find(|c: char| c.is_ascii_alphabetic())
                .expect("complete CSI");
            let parameters = &sequence[..end];
            let kind = sequence.as_bytes()[end];
            if kind == b'H' {
                let mut p = parameters
                    .split(';')
                    .map(|v| v.parse::<usize>().unwrap_or(1));
                y = p.next().unwrap_or(1).saturating_sub(1);
                x = p.next().unwrap_or(1).saturating_sub(1);
            } else if kind == b'J' && parameters == "2" {
                cells.fill(" ".into());
            }
            rest = &sequence[end + 1..];
            continue;
        }
        let end = rest.find('\x1b').unwrap_or(rest.len());
        for grapheme in rest[..end].graphemes(true) {
            let width = ratatui::text::Span::raw(grapheme).width();
            if x < usize::from(size.width) && y < usize::from(size.height) {
                cells[y * usize::from(size.width) + x] = grapheme.into();
                for offset in 1..width {
                    if x + offset < usize::from(size.width) {
                        cells[y * usize::from(size.width) + x + offset].clear();
                    }
                }
            }
            x += width;
        }
        rest = &rest[end..];
    }
    cells
}
#[test]
fn all_existing_screen_kinds_reach_turbo_vision_without_cell_changes() -> io::Result<()> {
    use dn::{app::App, palette::PaletteProfile, utilities::Kind};
    use ratatui::{backend::TestBackend, Terminal};
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };
    let directory = std::env::temp_dir().join(format!("dn-tv-render-{}", std::process::id()));
    fs::create_dir(&directory)?;
    let result = (|| -> io::Result<()> {
        for name in ["e\u{301}.txt", "界.txt", "👩‍💻.txt", "\u{f0000}.txt"] {
            fs::write(directory.join(name), "hello e\u{301} 界 👩‍💻\n")?;
        }
        for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            for size in [Size::new(80, 25), Size::new(132, 40)] {
                for scene in 0..9 {
                    let mut app = App::new(directory.clone())?;
                    app.config.palette = palette;
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while app.panels.iter().any(|p| p.loading) {
                        app.poll();
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(1));
                    }
                    match scene {
                        1 => app.prompt_operation(dn::operations::Kind::Mkdir, false),
                        2 => dn::palette::open(&mut app),
                        3 => dn::utilities::open(&mut app, Kind::Calculator),
                        4 => {
                            dn::editor::open(&mut app, true);
                            dn::editor::paste(&mut app, "e\u{301} 界 👩‍💻");
                        }
                        5 => app.show_viewer(dn::viewer::Viewer::from_contents(
                            directory.join("e\u{301}.txt"),
                            dn::viewer::from_bytes(
                                "e\u{301} 界 👩‍💻".as_bytes().to_vec(),
                                dn::viewer::ViewMode::Text,
                            ),
                        )),
                        6 => app.show_game(dn::tetris::Game::default()),
                        7 => {
                            app.show_game(dn::tetris::Game::default());
                            dn::tetris::action(&mut app, dn::tetris::GameAction::Setup);
                        }
                        8 => dn::windowing::open(&mut app),
                        _ => {}
                    }
                    let mut oracle = Terminal::new(TestBackend::new(size.width, size.height))?;
                    oracle.draw(|frame| dn::ui::draw(frame, &mut app))?;
                    let buffer = oracle.backend().buffer();
                    let mut painted = Vec::new();
                    let mut expected = vec![" ".to_owned(); buffer.content.len()];
                    for y in 0..size.height {
                        let mut x = 0;
                        while x < size.width {
                            let c = &buffer[(x, y)];
                            let width = ratatui::text::Span::raw(c.symbol())
                                .width()
                                .max(1)
                                .min(usize::from(size.width - x));
                            painted.push((x, y, c));
                            expected[usize::from(y) * usize::from(size.width) + usize::from(x)] =
                                c.symbol().into();
                            for offset in 1..width {
                                expected[usize::from(y) * usize::from(size.width)
                                    + usize::from(x)
                                    + offset]
                                    .clear();
                            }
                            x += width as u16;
                        }
                    }
                    let output = Output::default();
                    let mut native = TurboBackend::with_size(output.clone(), size)?;
                    native.bind_desktop(app.desktop.native());
                    app.desktop
                        .native()
                        .set_scope(dn::turbo::desktop::Scope::for_app(&app));
                    native.draw(painted.into_iter())?;
                    native.flush()?;
                    assert_eq!(
                        screen(&output.take(), size),
                        expected,
                        "palette={palette:?},size={size:?},scene={scene}"
                    );
                }
            }
        }
        Ok(())
    })();
    fs::remove_dir_all(directory)?;
    result
}

// Track the final colored screen across multiple differential frames. This
// catches native cells left behind when pieces move or a legacy page takes over.
fn colored_screen(bytes: &str, size: Size) -> Vec<(String, String, String)> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut cells = vec![
        (" ".into(), "default".into(), "default".into());
        usize::from(size.width) * usize::from(size.height)
    ];
    let (mut x, mut y) = (0usize, 0usize);
    let (mut fg, mut bg) = ("default".to_owned(), "default".to_owned());
    let mut rest = bytes;
    while !rest.is_empty() {
        if let Some(sequence) = rest.strip_prefix("\x1b[") {
            let end = sequence.find(|c: char| c.is_ascii_alphabetic()).unwrap();
            let p = &sequence[..end];
            match sequence.as_bytes()[end] {
                b'H' => {
                    let mut params = p.split(';').map(|n| n.parse::<usize>().unwrap_or(1));
                    y = params.next().unwrap_or(1).saturating_sub(1);
                    x = params.next().unwrap_or(1).saturating_sub(1);
                }
                b'J' if p == "2" => cells.fill((" ".into(), "default".into(), "default".into())),
                b'm' => {
                    let params: Vec<_> = p
                        .split(';')
                        .map(|n| n.parse::<u16>().unwrap_or(0))
                        .collect();
                    let mut i = 0;
                    while i < params.len() {
                        match params[i] {
                            0 => {
                                fg = "default".into();
                                bg = "default".into();
                            }
                            39 => fg = "default".into(),
                            49 => bg = "default".into(),
                            38 | 48 => {
                                let count = if params[i + 1] == 5 { 3 } else { 5 };
                                let color = format!("{:?}", &params[i + 1..i + count]);
                                if params[i] == 38 {
                                    fg = color;
                                } else {
                                    bg = color;
                                }
                                i += count;
                                continue;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            rest = &sequence[end + 1..];
            continue;
        }
        let end = rest.find('\x1b').unwrap_or(rest.len());
        for glyph in rest[..end].graphemes(true) {
            let width = ratatui::text::Span::raw(glyph).width();
            if x < usize::from(size.width) && y < usize::from(size.height) {
                cells[y * usize::from(size.width) + x] = (glyph.into(), fg.clone(), bg.clone());
                for offset in 1..width.min(usize::from(size.width) - x) {
                    cells[y * usize::from(size.width) + x + offset] =
                        (String::new(), fg.clone(), bg.clone());
                }
            }
            x += width;
        }
        rest = &rest[end..];
    }
    cells
}

#[test]
fn native_tetris_matches_legacy_colors_text_targets_and_transitions() -> io::Result<()> {
    use dn::{
        app::App,
        localization::Language,
        palette::PaletteProfile,
        tetris::{Game, Record, Screen},
        ui,
    };
    use ratatui::Terminal;
    for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
        for language in [Language::English, Language::Russian] {
            for size in [Size::new(80, 25), Size::new(132, 40), Size::new(83, 27)] {
                let mut app = App::new(std::env::current_dir()?)?;
                app.config.palette = palette;
                app.config.language = language;
                app.show_game(Game::with_seed(7, std::time::Instant::now()));
                let game = app.desktop.game_mut().unwrap();
                game.records.push(Record {
                    name: "界 e\u{301} 👩‍💻 \u{f0000}".into(),
                    start_level: 1,
                    level: 4,
                    lines: Some(12),
                    score: 1234,
                });
                game.board[18] = [9; dn::tetris::WIDTH];
                let legacy_output = Output::default();
                let native_output = Output::default();
                let mut legacy =
                    Terminal::new(TurboBackend::with_size(legacy_output.clone(), size)?)?;
                let mut native =
                    Terminal::new(TurboBackend::with_size(native_output.clone(), size)?)?;
                legacy.backend_mut().bind_desktop(app.desktop.native());
                native.backend_mut().bind_desktop(app.desktop.native());
                let (mut expected, mut actual) = (String::new(), String::new());
                for step in 0..35 {
                    if let Some(game) = app.desktop.game_mut() {
                        match step {
                            1 => {
                                game.translate(-1, 0);
                            }
                            2 => game.paused = true,
                            3 => {
                                game.paused = false;
                                game.preview = false;
                            }
                            4 => {
                                game.screen = Screen::Setup {
                                    level: 3,
                                    preview: false,
                                }
                            }
                            5 => game.screen = Screen::Scores,
                            6 => game.screen = Screen::Help,
                            7 => game.screen = Screen::Name,
                            8 => game.screen = Screen::Play,
                            9 => {
                                game.rotate();
                                game.preview = true;
                            }
                            18 => {
                                game.screen = Screen::GameOver;
                            }
                            19 => {
                                game.screen = Screen::Name;
                                game.name = "界".repeat(29);
                                game.name_cursor = game.name.len();
                                game.replace_name = false;
                                game.notice = "Cannot save scores: permission denied while opening the score configuration file in this directory. Retry save.".into();
                            }
                            20 => {
                                game.screen = Screen::Scores;
                                game.highlighted = Some(0);
                            }
                            21 => {
                                game.screen = Screen::Setup {
                                    level: 10,
                                    preview: true,
                                };
                                game.setup_focus = 1;
                            }
                            22 => {
                                game.setup_focus = 3;
                            }
                            23 => {
                                game.screen = Screen::Scores;
                                game.records.clear();
                            }
                            24 => {
                                game.notice.clear();
                            }
                            25 => {
                                game.screen = Screen::Name;
                                game.name = "e\u{301}界👩‍💻".repeat(7);
                                game.name_cursor = game.name.len();
                            }
                            26 => {
                                game.name_cursor = 0;
                                game.replace_name = true;
                            }
                            27 => {
                                game.screen = Screen::Play;
                            }
                            _ => {}
                        }
                    }
                    if step >= 28 {
                        let game = app.desktop.game_mut().unwrap();
                        game.screen = if step % 2 == 0 {
                            Screen::Name
                        } else {
                            Screen::Scores
                        };
                        game.notice = match step {
                            28 => "Cannot save: /tmp/a-very-long-directory-name/another-directory/tetris.conf: permission denied".into(),
                            29 => "  Cannot save scores:  multiple   spaces and a missing destination directory".into(),
                            30 => "保存先がありません 界 e\u{301} 👩‍💻 permission denied for this configuration directory".into(),
                            31 => "x".repeat(150),
                            32 => "First line\n\nThird line".into(),
                            33 => "one ".repeat(30),
                            _ => String::new(),
                        };
                    }
                    if step == 10 {
                        app.desktop.close_game();
                    }
                    if step == 11 {
                        app.show_game(Game::with_seed(99, std::time::Instant::now()));
                    }
                    if step == 12 {
                        // Both native raster and legacy canvas must survive resize.
                        legacy.backend_mut().resize(Size::new(100, 30))?;
                        native.backend_mut().resize(Size::new(100, 30))?;
                    }
                    if step == 13 {
                        legacy.backend_mut().resize(Size::new(50, 12))?;
                        native.backend_mut().resize(Size::new(50, 12))?;
                    }
                    if step == 14 {
                        legacy.backend_mut().resize(size)?;
                        native.backend_mut().resize(size)?;
                    }
                    if step == 15 {
                        app.desktop.game_mut().unwrap().over = true;
                    }
                    if step == 16 {
                        app.screensaver.start(std::time::Instant::now());
                    }
                    if step == 17 {
                        app.screensaver.activity(std::time::Instant::now());
                    }
                    if step == 20 {
                        let record = Record {
                            name: "界 e\u{301}".into(),
                            start_level: 1,
                            level: 4,
                            lines: Some(12),
                            score: 1234,
                        };
                        app.game_scores.add(record.clone())?;
                        app.desktop.game_mut().unwrap().records = vec![record];
                    }
                    legacy.draw(|frame| ui::draw(frame, &mut app))?;
                    let targets = format!("{:?}", app.mouse.regions);
                    expected.push_str(&legacy_output.take());
                    let painted = native.draw(|frame| ui::draw_native(frame, &mut app))?;
                    if step == 0 || (18..=26).contains(&step) {
                        let legacy_canvas: String = painted
                            .buffer
                            .content
                            .iter()
                            .map(|cell| cell.symbol())
                            .collect();
                        assert!(
                            !legacy_canvas.contains(language.text("Navigator's game")),
                            "native game must not be rendered into the Ratatui canvas"
                        );
                        for title in [
                            "Setup game",
                            "Tetris Top Ten",
                            "Tetris Help",
                            "New high score",
                            "Game over",
                        ] {
                            assert!(
                                !legacy_canvas.contains(language.text(title)),
                                "native page {title} must not be rendered into the Ratatui canvas"
                            );
                        }
                    }
                    if app.desktop.game().is_some_and(|g| g.screen == Screen::Name) {
                        assert_eq!(
                            legacy.backend_mut().get_cursor_position()?,
                            native.backend_mut().get_cursor_position()?,
                            "name cursor step={step}"
                        );
                    }
                    assert_eq!(
                        format!("{:?}", app.mouse.regions),
                        targets,
                        "mouse targets step={step}"
                    );
                    let native_frame = native_output.take();
                    assert!(
                        !native_frame.contains('\u{1}'),
                        "overlay marker leaked to terminal"
                    );
                    actual.push_str(&native_frame);
                    let dimensions = match step {
                        12 => Size::new(100, 30),
                        13 => Size::new(50, 12),
                        _ => size,
                    };
                    let expected_cells = colored_screen(&expected, dimensions);
                    let actual_cells = colored_screen(&actual, dimensions);
                    if let Some(index) = expected_cells
                        .iter()
                        .zip(&actual_cells)
                        .position(|(a, b)| a != b)
                    {
                        let row = index / usize::from(dimensions.width);
                        eprintln!(
                            "expected row {:?}",
                            expected_cells[row * usize::from(dimensions.width)
                                ..(row + 1) * usize::from(dimensions.width)]
                                .iter()
                                .map(|c| c.0.as_str())
                                .collect::<String>()
                        );
                        eprintln!(
                            "actual row {:?}",
                            actual_cells[row * usize::from(dimensions.width)
                                ..(row + 1) * usize::from(dimensions.width)]
                                .iter()
                                .map(|c| c.0.as_str())
                                .collect::<String>()
                        );
                        panic!("palette={palette:?},language={language:?},size={size:?},step={step},x={},y={}: expected {:?}, actual {:?}",index%usize::from(dimensions.width),index/usize::from(dimensions.width),expected_cells[index],actual_cells[index]);
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn native_tetris_clips_a_scene_prepared_before_terminal_shrink() -> io::Result<()> {
    use dn::{app::App, tetris::Game, ui};
    use ratatui::{backend::TestBackend, Terminal};
    let mut app = App::new(std::env::current_dir()?)?;
    app.show_game(Game::default());
    let mut canvas = Terminal::new(TestBackend::new(132, 40))?;
    canvas.draw(|frame| ui::draw_native(frame, &mut app))?;
    let output = Output::default();
    let mut native = TurboBackend::with_size(output.clone(), Size::new(40, 12))?;
    native.bind_desktop(app.desktop.native());
    native.draw(std::iter::empty())?;
    native.flush()?;
    // Repainting at the new dimensions drops the stale game scene entirely.
    let mut canvas = Terminal::new(TestBackend::new(40, 12))?;
    canvas.draw(|frame| ui::draw_native(frame, &mut app))?;
    native.draw(std::iter::empty())?;
    native.flush()?;
    Ok(())
}

#[test]
fn native_file_manager_matches_panes_shell_overlays_and_mouse_targets() -> io::Result<()> {
    use dn::{
        app::App, columns::ColumnPreset, localization::Language, palette::PaletteProfile,
        panel_layout::PanelLayout, tetris::Game, ui, volume_space::Space,
    };
    use ratatui::Terminal;
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let fixture =
        Temp(std::env::temp_dir().join(format!("dn-native-panes-{}", std::process::id())));
    fs::create_dir(&fixture.0)?;
    for n in 0..85 {
        fs::write(
            fixture.0.join(format!("item-{n:03}.txt")),
            format!("line {n}\n"),
        )?;
    }
    for name in [
        "e\u{301}.txt",
        "界.txt",
        "👩‍💻.rs",
        "\u{f0000}.zip",
        "long filename with spaces and multiple words.md",
    ] {
        fs::write(fixture.0.join(name), "hello")?;
    }
    fs::create_dir(fixture.0.join("folder"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("item-000.txt", fixture.0.join("link"))?;
    for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
        for language in [Language::English, Language::Russian] {
            for size in [Size::new(80, 25), Size::new(132, 40), Size::new(83, 27)] {
                for layout in [PanelLayout::Brief, PanelLayout::Details] {
                    let mut app = App::new(fixture.0.clone())?;
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while app.panels.iter().any(|p| p.loading) {
                        app.poll();
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(1));
                    }
                    app.config.palette = palette;
                    app.config.language = language;
                    app.config.layouts = [layout; 2];
                    app.config.columns = [ColumnPreset::NameSizeModified; 2];
                    app.config.show_clock = true;
                    app.clock.text = " 12:34:56 ".into();
                    for panel in &mut app.panels {
                        panel.space.result = Some(Ok(Space {
                            available: 123456789,
                            total: 987654321,
                        }));
                    }
                    let legacy_output = Output::default();
                    let native_output = Output::default();
                    let mut legacy =
                        Terminal::new(TurboBackend::with_size(legacy_output.clone(), size)?)?;
                    let mut native =
                        Terminal::new(TurboBackend::with_size(native_output.clone(), size)?)?;
                    legacy.backend_mut().bind_desktop(app.desktop.native());
                    native.backend_mut().bind_desktop(app.desktop.native());
                    let (mut expected, mut actual) = (String::new(), String::new());
                    for step in 0..42 {
                        match step {
                            1=>app.active=1,
                            2=> {app.panels[1].selected.insert("item-000.txt".into()); app.panels[1].selected.insert("folder".into());}
                            3=>app.panels[1].cursor=app.panels[1].entries.len()-1,
                            4=>app.config.columns=[ColumnPreset::Name;2],
                            5=>app.config.columns=[ColumnPreset::NameSize;2],
                            6=>app.config.columns=[ColumnPreset::NameSizeModified;2],
                            7=>app.config.layouts=[if layout==PanelLayout::Brief {PanelLayout::Details} else {PanelLayout::Brief};2],
                            8=>{app.config.show_tags=false;app.config.highlight_files=false;}
                            9=>{dn::quick_search::open(&mut app); let q=app.quick_search.as_mut().unwrap();q.query="missing界".into();q.no_match=true;}
                            10=>{app.quick_search.as_mut().unwrap().query="item".into();app.quick_search.as_mut().unwrap().no_match=false;}
                            11=>{app.quick_search=None;app.panels[1].set_filter("*.txt".into());}
                            12=>app.panels[1].set_filter(String::new()),
                            13=>app.panels[0].loading=true,
                            14=>{app.panels[0].loading=false;app.panels[0].entries.clear();}
                            15=>{app.panels[0].entries=dn::filesystem::scan(&fixture.0,false)?;app.panels[0].entries[1].size=None;app.panels[0].space.result=Some(Err("unavailable".into()));}
                            16=>app.command_line.insert("echo 界 e\u{301} 👩‍💻 and a rather long command to exercise scrolling")?,
                            17=>app.menu=Some((0,0)),
                            18=>{dn::menu::close(&mut app);app.prompt_operation(dn::operations::Kind::Mkdir,false);}
                            19=>{app.prompt=None;dn::welcome::open(&mut app);}
                            20=>{app.welcome.dialog=None;dn::dialog::keyboard_help(&mut app);}
                            21=>{app.dialogs.clear();dn::quick_view::toggle(&mut app);}
                            22=>{dn::quick_view::toggle(&mut app);dn::palette::open(&mut app);}
                            23=>{app.color_dialog=None;dn::windowing::open(&mut app);}
                            24=>{app.window_list=None;app.config.shortcuts=dn::shortcuts::Profile::Mac;dn::input::handle(&mut app,crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('o'),crossterm::event::KeyModifiers::CONTROL));}
                            25=>{app.shortcuts.clear();app.screensaver.start(Instant::now());}
                            26=>{app.screensaver.activity(Instant::now());}
                            27=>{legacy.backend_mut().resize(Size::new(50,12))?;native.backend_mut().resize(Size::new(50,12))?;}
                            28=>{legacy.backend_mut().resize(size)?;native.backend_mut().resize(size)?;}
                            29=>app.show_game(Game::with_seed(7,Instant::now())),
                            30=>app.desktop.close_game(),
                            31=>app.open_settings(dn::settings::SettingsKind::Keyboard),
                            32 => {
                                app.settings = None;
                                dn::menu::open(&mut app, 1);
                                let row = dn::menu::level_items(&app,0).iter().position(|item| matches!(item.action,dn::menu::Action::Submenu(_))).unwrap();
                                dn::menu::activate(&mut app,0,row);
                            }
                            33 => {
                                if let Some(entry) = app.panels[0].entries.get_mut(1) { entry.name = "changed e\u{301} 界 👩‍💻.txt".into(); }
                                app.clock.text = " 23:59:59 ".into();
                            }
                            34 => dn::menu::close(&mut app),
                            35 => { legacy.backend_mut().resize(Size::new(140,42))?; native.backend_mut().resize(Size::new(140,42))?; }
                            36 => { legacy.backend_mut().resize(size)?; native.backend_mut().resize(size)?; }
                            37 => dn::quick_view::toggle(&mut app),
                            38 => app.show_game(Game::with_seed(12,Instant::now())),
                            39 => app.desktop.game_mut().unwrap().screen = dn::tetris::Screen::Scores,
                            40 => app.desktop.close_game(),
                            41 => dn::quick_view::toggle(&mut app),
                            _=>{}
                        }
                        app.desktop
                            .native()
                            .set_scope(dn::turbo::desktop::Scope::for_app(&app));
                        legacy.draw(|frame| ui::draw(frame, &mut app))?;
                        let targets = format!("{:?}", app.mouse.regions);
                        expected.push_str(&legacy_output.take());
                        let painted = native.draw(|frame| ui::draw_native(frame, &mut app))?;
                        if step == 0 {
                            let canvas: String =
                                painted.buffer.content.iter().map(|c| c.symbol()).collect();
                            assert!(
                                !canvas.contains("item-"),
                                "native panes must not enter the legacy canvas"
                            );
                            assert!(
                                !canvas.contains("F1"),
                                "native function bar must not enter the legacy canvas"
                            );
                        }
                        assert_eq!(
                            format!("{:?}", app.mouse.regions),
                            targets,
                            "targets: step={step}"
                        );
                        if dn::focus::current(&app) == dn::focus::Focus::Manager && step != 27 {
                            assert_eq!(
                                legacy.backend_mut().get_cursor_position()?,
                                native.backend_mut().get_cursor_position()?,
                                "command cursor step={step}"
                            );
                        }
                        let native_frame = native_output.take();
                        assert!(
                            !native_frame.contains('\u{1}'),
                            "overlay marker leaked to terminal"
                        );
                        actual.push_str(&native_frame);
                        let dimensions = match step {
                            27 => Size::new(50, 12),
                            35 => Size::new(140, 42),
                            _ => size,
                        };
                        let expected_cells = colored_screen(&expected, dimensions);
                        let actual_cells = colored_screen(&actual, dimensions);
                        if let Some(index) = expected_cells
                            .iter()
                            .zip(&actual_cells)
                            .position(|(a, b)| a != b)
                        {
                            let row = index / usize::from(dimensions.width);
                            eprintln!(
                                "expected {:?}",
                                expected_cells[row * usize::from(dimensions.width)
                                    ..(row + 1) * usize::from(dimensions.width)]
                                    .iter()
                                    .map(|c| c.0.as_str())
                                    .collect::<String>()
                            );
                            eprintln!(
                                "actual {:?}",
                                actual_cells[row * usize::from(dimensions.width)
                                    ..(row + 1) * usize::from(dimensions.width)]
                                    .iter()
                                    .map(|c| c.0.as_str())
                                    .collect::<String>()
                            );
                            panic!("palette={palette:?},language={language:?},size={size:?},layout={layout:?},step={step},x={},y={row}: expected {:?}, actual {:?}",index%usize::from(dimensions.width),expected_cells[index],actual_cells[index]);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
