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
