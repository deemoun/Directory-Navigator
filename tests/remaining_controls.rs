//! Remaining shared controls use only private temporary fixtures.
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    app::App,
    input,
    mouse::Target,
    testing::{layout::Rect, Buffer},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, Instant},
};
mod support;
use support::{click_at, draw, key, Fixture};
fn fixture() -> io::Result<(Fixture, App)> {
    let t = Fixture::new("remaining-controls")?;
    fs::write(t.0.join("fixture.txt"), "never replace this fixture")?;
    let mut a = t.app()?;
    a.config = Default::default();
    a.config.save_on_exit = false;
    a.config_store = dn::config::Store::new(Some(t.0.join("settings.conf")));
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "fixture.txt")
        .unwrap();
    Ok((t, a))
}
fn wait(a: &mut App, ready: impl Fn(&App) -> bool) {
    support::wait_until(a, Duration::from_secs(5), ready);
}

fn hit(a: &App, target: Target) -> Rect {
    a.mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
        .unwrap()
        .0
}
#[test]
fn copy_move_disabled_focus_recovers_without_submitting_or_changing_options() -> io::Result<()> {
    for kind in [dn::operations::Kind::Copy, dn::operations::Kind::Move] {
        let (t, mut a) = fixture()?;
        a.prompt_operation(kind, false);
        if kind == dn::operations::Kind::Copy {
            dn::copy_dialog::action(&mut a, dn::copy_dialog::Action::RemoveSources);
        }
        let (disabled, target) = if kind == dn::operations::Kind::Copy {
            (4, dn::copy_dialog::Action::Verify)
        } else {
            (5, dn::copy_dialog::Action::RemoveSources)
        };
        let p = a.modals.prompt_mut().unwrap();
        p.focus = disabled; // Forced verification / locked Move source removal.
        let options = p.copy.as_ref().unwrap().options;
        draw(&mut a, 80, 25)?;
        assert!(!a
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Copy(target)));
        key(&mut a, K::Enter);
        key(&mut a, K::Char(' '));
        assert!(a.job.is_none());
        assert_eq!(
            a.modals.prompt().unwrap().copy.as_ref().unwrap().options,
            options
        );
        key(&mut a, K::Tab);
        assert_eq!(a.modals.prompt().unwrap().focus, 0);
        input::handle(&mut a, KeyEvent::new(K::Tab, M::SHIFT));
        assert_eq!(a.modals.prompt().unwrap().focus, 9);
        draw(&mut a, 80, 25)?;
        let r = hit(&a, Target::CancelPrompt);
        click_at(&mut a, r);
        assert!(a.modals.prompt().is_none());
        assert_eq!(
            fs::read_to_string(t.0.join("fixture.txt"))?,
            "never replace this fixture"
        );
    }
    Ok(())
}
#[test]
fn completed_discovery_disables_stop_and_retires_its_published_mouse_action() -> io::Result<()> {
    let (_t, mut a) = fixture()?;
    dn::discovery::open_find(&mut a);
    dn::discovery::action(&mut a, dn::discovery::Action::Start);
    a.discovery_mut().unwrap().focus = 4;
    draw(&mut a, 80, 25)?;
    let stale = hit(&a, Target::Discovery(dn::discovery::Action::Stop));
    let revision = a.layout_revision;
    wait(&mut a, |a| a.discovery().unwrap().job.is_none());
    dn::layout::update(&mut a);
    assert!(a.layout_revision > revision);
    click_at(&mut a, stale);
    assert!(a.discovery().is_some());
    key(&mut a, K::Enter);
    assert_eq!(a.discovery().unwrap().focus, 4);
    key(&mut a, K::BackTab);
    assert_eq!(a.discovery().unwrap().focus, 6);
    key(&mut a, K::Enter);
    assert!(a.discovery().is_none());
    Ok(())
}
#[test]
fn calculator_history_is_a_blocking_shared_list_and_preserves_parent_focus() -> io::Result<()> {
    let (_t, mut a) = fixture()?;
    dn::utilities::open(&mut a, dn::utilities::Kind::Calculator);
    a.desktop.plugins.calculator_mut().expression = "41 + 1".into();
    a.desktop.plugins.calculator_mut().cursor = 6;
    a.desktop.plugins.calculator_mut().evaluate();
    a.desktop.plugins.calculator_mut().focus = 7;
    dn::utilities::action(&mut a, dn::utilities::Action::CalculatorHistory);
    draw(&mut a, 80, 25)?;
    assert!(a.mouse.regions.iter().all(|(_, t)| matches!(
        t,
        Target::Plugin(dn::plugins::CALCULATOR, dn::plugins::ActionId(5 | 1000..))
            | Target::Surface(..)
    )));
    for k in [K::Tab, K::BackTab] {
        key(&mut a, k);
        assert_eq!(a.desktop.plugins.calculator_mut().focus, 7);
        assert!(a.desktop.plugins.calculator_mut().history_open);
    }
    key(&mut a, K::Esc);
    assert!(!a.desktop.plugins.calculator_mut().history_open);
    assert_eq!(a.desktop.plugins.calculator_mut().focus, 7);
    dn::utilities::action(&mut a, dn::utilities::Action::CalculatorHistory);
    key(&mut a, K::Enter);
    assert!(!a.desktop.plugins.calculator_mut().history_open);
    assert_eq!(a.desktop.plugins.calculator_mut().focus, 0);
    assert_eq!(a.desktop.plugins.calculator_mut().expression, "41 + 1");
    assert!(a.desktop.plugins.calculator_mut().result.is_none());
    Ok(())
}
fn setup(a: &mut App, case: usize) -> (u16, u16) {
    use dn::{file_tools, tetris, utilities};
    match case {
        0 | 1 => {
            a.prompt_operation(
                if case == 0 {
                    dn::operations::Kind::Copy
                } else {
                    dn::operations::Kind::Move
                },
                false,
            );
            let p = a.modals.prompt_mut().unwrap();
            p.field = "界 e\u{301} 👩‍💻".into();
            p.cursor = p.field.len();
            (59, 16)
        }
        2 => {
            file_tools::open_compare(a);
            (74, 15)
        }
        3 => {
            file_tools::open_export(a);
            if let Some(file_tools::ToolDialog::Export {
                field,
                cursor,
                base,
                ..
            }) = a.file_tools_mut()
            {
                *base = PathBuf::from("/fixture");
                *field = "list 界.txt".into();
                *cursor = field.len();
            }
            (74, 13)
        }
        4 => {
            a.open_pattern(true);
            (58, 8)
        }
        5 => {
            a.open_columns();
            (74, 13)
        }
        6 | 7 => {
            utilities::open(a, utilities::Kind::Calculator);
            a.desktop.plugins.calculator_mut().expression = "12345 * (67 + 89)".into();
            a.desktop.plugins.calculator_mut().cursor =
                a.desktop.plugins.calculator_mut().expression.len();
            a.desktop.plugins.calculator_mut().evaluate();
            if case == 7 {
                a.desktop.plugins.calculator_mut().history_open = true;
            }
            (72, 18)
        }
        8 => {
            utilities::open(a, utilities::Kind::Characters);
            (76, 24)
        }
        9 => {
            dn::palette::open(a);
            (72, 18)
        }
        10 => {
            a.screensaver
                .modals
                .set_screensaver(Some(Default::default()));
            (58, 20)
        }
        11 => {
            dn::navigation_tools::open_bookmarks(a);
            (74, 18)
        }
        12 => {
            dn::navigation_tools::open_filter(a);
            (74, 11)
        }
        13 => {
            dn::command_line::history::open(a);
            (74, 21)
        }
        14 | 15 => {
            tetris::open(a);
            if case == 14 {
                tetris::action(a, tetris::GameAction::Setup);
                (
                    if a.config.language == dn::localization::Language::Russian {
                        68
                    } else {
                        53
                    },
                    15,
                )
            } else {
                a.desktop.game_mut().unwrap().screen = tetris::Screen::GameOver;
                (44, 7)
            }
        }
        _ => unreachable!(),
    }
}
#[test]
fn remaining_dialog_matrix_is_immutable_clipped_and_translated() -> io::Result<()> {
    use dn::{localization::Language, palette::PaletteProfile};
    let mut captures = String::new();
    for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
        for language in [Language::English, Language::Russian] {
            for (w, h) in [(80, 25), (132, 40)] {
                for case in 0..16 {
                    let (_t, mut a) = fixture()?;
                    a.config.palette = palette;
                    a.config.language = language;
                    let (dw, dh) = setup(&mut a, case);
                    // Preserve each surface's variable full-width sizing for the comparison.
                    let dw = if [2, 3, 8, 11, 12, 13].contains(&case) {
                        if case == 13 {
                            (w - 6).min(100)
                        } else {
                            if case == 8 {
                                (w - 4).min(100)
                            } else {
                                (w - 6).min(94)
                            }
                        }
                    } else {
                        dw
                    };
                    let dh = if case == 13 { (h - 4).min(26) } else { dh };
                    let snap = dn::layout::prepare_window(
                        &mut a,
                        Rect::new(7, 3, w, h),
                        Rect::new(0, 0, w + 20, h + 12),
                        Instant::now(),
                    );
                    let focus = dn::focus::current(&a);
                    let revision = a.layout_revision;
                    let first = ui::render(&a, &snap);
                    let hits = first.hit_regions().to_vec();
                    let cursor = first.cursor();
                    let second = ui::render(&a, &snap);
                    assert_eq!(cursor, second.cursor());
                    assert_eq!(hits, second.hit_regions());
                    let b = Buffer::rendered(first);
                    assert_eq!(b, Buffer::rendered(second));
                    assert_eq!(dn::focus::current(&a), focus);
                    assert_eq!(a.layout_revision, revision);
                    assert!(
                        hits.iter().all(|(r, _)| r.x >= 7
                            && r.y >= 3
                            && r.right() <= w + 7
                            && r.bottom() <= h + 3),
                        "case {case}"
                    );
                    captures.push_str(&format!("{palette:?} {language:?} {w} {h} {case}\n"));
                    let x = 7 + (w - dw) / 2;
                    let y = 3 + (h - dh) / 2;
                    for yy in y..y + dh {
                        for xx in x..x + dw {
                            captures.push_str(&format!("{:?}", b[(xx, yy)]));
                        }
                    }
                    captures.push('\n');
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("DN_REMAINING_CAPTURE") {
        fs::write(path, captures)?;
    }
    Ok(())
}
