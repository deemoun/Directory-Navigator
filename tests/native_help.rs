use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    localization::Language, palette::PaletteProfile, presentation::layout::Size,
    shortcuts::Profile, turbo::help::Browser,
};
fn prepare(help: &mut Browser, language: Language) {
    help.prepare(
        Size::new(100, 35),
        PaletteProfile::ClassicDn,
        language,
        Profile::default(),
    );
}
fn key(help: &mut Browser, key: K, modifiers: M) {
    assert!(!help.key(KeyEvent::new(key, modifiers)));
}
#[test]
fn native_help_contents_topics_history_and_resize_preserve_context() {
    let mut help = Browser::new("Help", &"context line\n".repeat(80));
    prepare(&mut help, Language::English);
    key(&mut help, K::PageDown, M::NONE);
    let offset = help.offset();
    assert!(offset > 0);
    key(&mut help, K::F(1), M::NONE);
    assert_eq!(help.topic().as_deref(), Some("contents"));
    // The first link is selected initially; Tab selects the panels topic.
    key(&mut help, K::Tab, M::NONE);
    key(&mut help, K::Enter, M::NONE);
    assert_eq!(help.topic().as_deref(), Some("panels"));
    key(&mut help, K::F(1), M::ALT);
    assert_eq!(help.topic().as_deref(), Some("contents"));
    key(&mut help, K::F(1), M::ALT);
    assert_eq!(help.topic().as_deref(), Some("context"));
    assert_eq!(help.offset(), offset);
    help.prepare(
        Size::new(79, 24),
        PaletteProfile::ClassicDn,
        Language::English,
        Profile::default(),
    );
    assert!(help.scene().is_none());
    prepare(&mut help, Language::English);
    assert_eq!(help.offset(), offset);
    assert!(help.scene().is_some());
    assert!(help.key(KeyEvent::new(K::Esc, M::NONE)));
}
#[test]
fn native_help_handles_unicode_and_literal_markup_in_both_languages() {
    for language in [Language::English, Language::Russian] {
        let mut help = Browser::new("Help", "2 * 3; `literal`; Кириллица; e\u{301}; 界; 🦀");
        prepare(&mut help, language);
        assert!(help.scene().is_some());
        key(&mut help, K::F(1), M::NONE);
        prepare(&mut help, language);
        assert_eq!(help.topic().as_deref(), Some("contents"));
    }
}
