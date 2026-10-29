//! Frozen before the shared-logic refactor: cells/styles, cursor and mouse hits.
mod support;
use dn::{
    editor::{self, EditorAction as A},
    layout,
    localization::Language,
    palette::PaletteProfile,
    presentation::layout::Rect,
    settings::SettingsKind,
    shortcuts::Profile,
    testing::Buffer,
    ui,
    viewer::{self, ViewMode, Viewer},
};
use std::io;
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
#[test]
fn shared_logic_preserves_native_frames() -> io::Result<()> {
    let fixture = support::Fixture::new("logic-parity")?;
    let mut captures = String::new();
    for language in [Language::English, Language::Russian] {
        for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            for profile in [Profile::ClassicDn, Profile::Mac] {
                for (w, h) in [(80, 25), (132, 40)] {
                    for case in 0..20 {
                        // Automatic's display name follows the host OS. These two
                        // dropdown goldens are macOS-specific; behavior is tested
                        // separately against the host's own Profile names.
                        if !cfg!(target_os = "macos") && matches!(case, 17 | 18) {
                            continue;
                        }
                        let mut a = fixture.app()?;
                        a.config.show_clock = false;
                        a.config.language = language;
                        a.config.palette = palette;
                        a.config.shortcuts = profile;
                        a.show_viewer(Viewer::from_contents(
                            "fixture.txt".into(),
                            viewer::from_bytes(
                                "alpha 界 e\u{301}\nbeta\n".as_bytes().to_vec(),
                                ViewMode::Text,
                            ),
                        ));
                        match case {
                            0..=5 => a.open_settings(
                                [
                                    SettingsKind::System,
                                    SettingsKind::Interface,
                                    SettingsKind::FileManager,
                                    SettingsKind::Highlight,
                                    SettingsKind::Plugins,
                                    SettingsKind::Keyboard,
                                ][case],
                            ),
                            6 => viewer::action(&mut a, viewer::Action::Find),
                            7 => dn::utilities::open(&mut a, dn::utilities::Kind::Calculator),
                            8..=11 => {
                                editor::open(&mut a, true);
                                support::wait(&mut a);
                                editor::paste(&mut a, "alpha 界 e\u{301}\nbeta\n");
                                if case == 9 {
                                    editor::action(&mut a, A::Menu);
                                }
                                if case == 10 {
                                    editor::action(&mut a, A::Find);
                                }
                                if case == 11 {
                                    editor::action(&mut a, A::Help);
                                }
                            }
                            12 => dn::windowing::open(&mut a),
                            13 => dn::command_line::history::open(&mut a),
                            14 => {
                                for i in 0..25 {
                                    a.file_tools.histories.record(
                                        dn::file_tools::history::HistoryKind::Edit,
                                        format!("fixture-{i:02}.txt").into(),
                                    );
                                }
                                dn::file_tools::open_history(
                                    &mut a,
                                    dn::file_tools::history::HistoryKind::Edit,
                                );
                            }
                            15 => {
                                a.config.bookmarks[0] = Some("fixture".into());
                                dn::navigation_tools::open_bookmarks(&mut a);
                            }
                            16 => dn::palette::open(&mut a),
                            17 | 18 => {
                                a.open_settings(SettingsKind::Keyboard);
                                if case == 18 {
                                    a.modals.settings_mut().unwrap().keyboard_choice(false);
                                }
                                dn::settings::open_choice(&mut a, 0);
                            }
                            19 => {
                                a.open_settings(SettingsKind::Regional);
                                dn::settings::open_choice(&mut a, 1);
                            }
                            _ => unreachable!(),
                        }
                        let snapshot = layout::prepare(&mut a, Rect::new(0, 0, w, h));
                        let result = ui::render(&a, &snapshot);
                        let normalize = |result: dn::ui::RenderedFrame| {
                            let cursor = result.cursor();
                            let hits = result.hit_regions().to_vec();
                            let mut buffer = Buffer::rendered(result);
                            if case == 19 {
                                // The regional preview samples wall-clock time. Freeze only its
                                // digits; punctuation, geometry, colors and other text stay exact.
                                let y = (h - 17) / 2 + 9;
                                for x in 0..w {
                                    let cell = &mut buffer.content[usize::from(y * w + x)];
                                    if cell.symbol().chars().all(|c| c.is_ascii_digit()) {
                                        cell.set_symbol("#");
                                    }
                                }
                            }
                            format!("{cursor:?}{hits:?}{buffer:?}")
                        };
                        let signature = normalize(result);
                        assert_eq!(signature, normalize(ui::render(&a, &snapshot)));
                        captures.push_str(&format!(
                            "{language:?}/{palette:?}/{profile:?}/{w}x{h}/{case}: {:016x}\n",
                            hash(&signature)
                        ));
                    }
                }
            }
        }
    }
    let expected = include_str!("fixtures/shared_logic_frames.txt")
        .lines()
        .filter(|line| {
            cfg!(target_os = "macos")
                || !matches!(
                    line.split(": ").next().unwrap().rsplit('/').next().unwrap(),
                    "17" | "18"
                )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert_eq!(captures, expected);
    Ok(())
}
