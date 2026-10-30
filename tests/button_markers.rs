//! Compact rows must reserve both DN focus arrows without clipping captions.
mod support;
use crossterm::event::KeyCode as K;
use dn::{
    app::App, copy_dialog, localization::Language, mouse::Target, operations::Kind,
    palette::PaletteProfile, testing::Buffer,
};
use std::{fs, io};
fn focus(app: &mut App, wanted: usize) {
    for _ in 0..20 {
        if app.modals.prompt().unwrap().focus == wanted {
            return;
        }
        support::key(app, K::Tab);
    }
    panic!("Tab never reached button {wanted}");
}
fn assert_button(app: &App, b: &Buffer, target: Target) {
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(r, t)| r.width > 3 && *t == target)
        .unwrap()
        .0;
    let row = (r.x..r.right())
        .map(|x| b[(x, r.y)].symbol())
        .collect::<String>();
    assert_eq!(
        b[(r.x + 1, r.y)].symbol(),
        "►",
        "left marker missing: {target:?} {row:?}"
    );
    assert_eq!(
        b[(r.right() - 2, r.y)].symbol(),
        "◄",
        "right marker missing: {target:?} {row:?}"
    );
    let prompt = app.modals.prompt().unwrap();
    let source = match target {
        Target::SubmitPrompt if prompt.copy.is_some() => "O~K~",
        Target::SubmitPrompt => {
            if matches!(
                prompt.kind,
                dn::dialog::PromptKind::Operation(Kind::Delete | Kind::Trash)
            ) {
                "[ Delete ]"
            } else {
                "[ OK ]"
            }
        }
        Target::CancelPrompt => "[ Cancel ]",
        Target::PromptHelp if prompt.copy.is_some() => "~H~elp",
        Target::PromptHelp => "[ Help ]",
        Target::Copy(copy_dialog::Action::Tree) => "~T~ree",
        _ => unreachable!(),
    };
    let caption = app.config.language.text(source).replace('~', "");
    assert!(
        row.contains(
            caption
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim()
        ),
        "clipped caption: {row:?}"
    );
    for x in [r.x + 1, r.right() - 2] {
        assert_eq!(b[(x, r.y)].fg, app.config.palette.color(11));
        assert_eq!(b[(x, r.y)].bg, app.config.palette.color(8));
    }
}
#[test]
fn compact_prompt_and_copy_move_buttons_keep_both_arrows_through_tab_and_cancel() -> io::Result<()>
{
    let fixture = support::Fixture::new("button-markers")?;
    fs::write(fixture.0.join("source.txt"), "keep this fixture")?;
    for language in [Language::English, Language::Russian] {
        for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            for (w, h) in [(80, 25), (132, 40)] {
                for case in 0..8 {
                    let mut app = fixture.app()?;
                    app.config.language = language;
                    app.config.palette = palette;
                    app.panels[0].cursor = app.panels[0]
                        .entries
                        .iter()
                        .position(|e| e.name == "source.txt")
                        .unwrap();
                    match case {
                        0 => dn::config_import::open(&mut app),
                        1 => app.prompt_goto(),
                        2 => app.prompt_operation(Kind::Mkdir, false),
                        3 | 4 => {
                            app.config.use_trash = case == 4;
                            app.prompt_operation(Kind::Delete, false);
                        }
                        5 => app.prompt_operation(Kind::Copy, false),
                        6 | 7 => app.prompt_operation(Kind::Move, case == 7),
                        _ => unreachable!(),
                    }
                    support::draw(&mut app, w, h)?;
                    let controls = if case >= 5 {
                        vec![
                            (6, Target::SubmitPrompt),
                            (7, Target::CancelPrompt),
                            (8, Target::Copy(copy_dialog::Action::Tree)),
                            (9, Target::PromptHelp),
                        ]
                    } else {
                        vec![
                            (1, Target::SubmitPrompt),
                            (2, Target::CancelPrompt),
                            (3, Target::PromptHelp),
                        ]
                    };
                    for (id, target) in &controls {
                        focus(&mut app, *id);
                        let b = support::draw(&mut app, w, h)?;
                        assert_button(&app, &b, *target);
                        let mut row = controls
                            .iter()
                            .map(|(_, t)| {
                                app.mouse
                                    .regions
                                    .iter()
                                    .find(|(r, target)| r.width > 3 && target == t)
                                    .unwrap()
                                    .0
                            })
                            .collect::<Vec<_>>();
                        row.sort_by_key(|r| r.x);
                        assert!(row.windows(2).all(|r| r[0].right() <= r[1].x));
                        assert!(row.iter().all(|r| r.right() <= w && r.bottom() <= h));
                    }
                    focus(&mut app, if case >= 5 { 7 } else { 2 });
                    support::key(&mut app, K::BackTab);
                    support::key(&mut app, K::Tab);
                    let b = support::draw(&mut app, w, h)?;
                    assert_button(&app, &b, Target::CancelPrompt);
                    support::key(&mut app, K::Enter);
                    assert!(app.modals.prompt().is_none() && app.job.is_none());
                    assert_eq!(
                        fs::read(fixture.0.join("source.txt"))?,
                        b"keep this fixture"
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn translated_archive_and_game_buttons_keep_both_arrows() -> io::Result<()> {
    use std::{io::Write, time::Duration};
    let fixture = support::Fixture::new("translated-button-markers")?;
    let path = fixture.0.join("fixture.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
    zip.start_file("file.txt", zip::write::SimpleFileOptions::default())?;
    zip.write_all(b"fixture")?;
    zip.finish()?;
    for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
        for (w, h) in [(80, 25), (132, 40)] {
            let mut app = fixture.app()?;
            app.config.language = Language::Russian;
            app.config.palette = palette;
            dn::archive::open(&mut app, path.clone());
            support::wait_until(&mut app, Duration::from_secs(5), |a| {
                a.archive().is_some_and(|b| !b.loading())
            });
            support::draw(&mut app, w, h)?;
            for _ in 0..10 {
                if app.archive().unwrap().focus == 4 {
                    break;
                }
                support::key(&mut app, K::Tab);
            }
            check_markers(
                &mut app,
                w,
                h,
                Target::Archive(dn::archive::Action::Refresh),
                "Обновить",
            )?;
            support::key(&mut app, K::Esc);
            dn::tetris::open(&mut app);
            support::draw(&mut app, w, h)?;
            support::key(&mut app, K::Tab);
            support::key(&mut app, K::Tab);
            check_markers(
                &mut app,
                w,
                h,
                dn::tetris::target(dn::tetris::GameAction::Setup),
                "Настройка",
            )?;
            support::key(&mut app, K::F(1));
            check_markers(
                &mut app,
                w,
                h,
                dn::tetris::target(dn::tetris::GameAction::Back),
                "",
            )?;
        }
    }
    Ok(())
}
fn check_markers(app: &mut App, w: u16, h: u16, target: Target, caption: &str) -> io::Result<()> {
    let b = support::draw(app, w, h)?;
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(r, t)| r.width > 3 && *t == target)
        .unwrap()
        .0;
    let row: String = (r.x..r.right()).map(|x| b[(x, r.y)].symbol()).collect();
    assert_eq!(b[(r.x + 1, r.y)].symbol(), "►", "{target:?} {row:?}");
    assert_eq!(b[(r.right() - 2, r.y)].symbol(), "◄", "{target:?} {row:?}");
    assert!(row.contains(caption), "clipped caption: {row:?}");
    Ok(())
}
