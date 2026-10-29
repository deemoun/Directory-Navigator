//! Explicit ordered choice values; drafts, localization and child ownership stay
//! in Settings. Unknown separators retain their legacy cycling fallback.
use crate::{config::Config, localization::Language, regional::DateOrder, shortcuts::Profile};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceValue {
    Profile(Profile),
    Language(Language),
    DateOrder(DateOrder),
    Separator(char),
    TwelveHour(bool),
}
impl ChoiceValue {
    fn label(self, language: Language) -> String {
        match self {
            Self::Profile(p) => language.text(p.name()).into(),
            Self::Language(l) => l.name().into(),
            Self::DateOrder(order) => language
                .text(match order {
                    DateOrder::Ymd => "Year–month–day",
                    DateOrder::Dmy => "Day–month–year",
                    DateOrder::Mdy => "Month–day–year",
                })
                .into(),
            Self::Separator(c) => c.to_string(),
            Self::TwelveHour(b) => language.text(if b { "12 hour" } else { "24 hour" }).into(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceId {
    Profile,
    Language,
    DateOrder,
    DateSeparator,
    TimeFormat,
    TimeSeparator,
}
pub const KEYBOARD: &[ChoiceId] = &[ChoiceId::Profile];
pub const REGIONAL: &[ChoiceId] = &[
    ChoiceId::Language,
    ChoiceId::DateOrder,
    ChoiceId::DateSeparator,
    ChoiceId::TimeFormat,
    ChoiceId::TimeSeparator,
];
pub struct ChoiceSpec {
    pub caption: &'static str,
    pub values: &'static [ChoiceValue],
    read: fn(&Config) -> ChoiceValue,
    write: fn(&mut Config, ChoiceValue),
    fallback: usize,
}
impl ChoiceId {
    pub fn definition(self) -> ChoiceSpec {
        use ChoiceValue as V;
        match self {
            Self::Profile => ChoiceSpec {
                caption: "Profile",
                values: &[
                    V::Profile(Profile::Auto),
                    V::Profile(Profile::ClassicDn),
                    V::Profile(Profile::Mac),
                ],
                read: |c| V::Profile(c.shortcuts),
                write: |c, v| {
                    if let V::Profile(v) = v {
                        c.shortcuts = v
                    }
                },
                fallback: 0,
            },
            Self::Language => ChoiceSpec {
                caption: "Language",
                values: &[
                    V::Language(Language::English),
                    V::Language(Language::Russian),
                ],
                read: |c| V::Language(c.language),
                write: |c, v| {
                    if let V::Language(v) = v {
                        c.language = v
                    }
                },
                fallback: 0,
            },
            Self::DateOrder => ChoiceSpec {
                caption: "Date order",
                values: &[
                    V::DateOrder(DateOrder::Ymd),
                    V::DateOrder(DateOrder::Dmy),
                    V::DateOrder(DateOrder::Mdy),
                ],
                read: |c| V::DateOrder(c.datetime.date_order),
                write: |c, v| {
                    if let V::DateOrder(v) = v {
                        c.datetime.date_order = v
                    }
                },
                fallback: 0,
            },
            Self::DateSeparator => ChoiceSpec {
                caption: "Date separator",
                values: &[V::Separator('-'), V::Separator('.'), V::Separator('/')],
                read: |c| V::Separator(c.datetime.date_separator),
                write: |c, v| {
                    if let V::Separator(v) = v {
                        c.datetime.date_separator = v
                    }
                },
                fallback: 0,
            },
            Self::TimeFormat => ChoiceSpec {
                caption: "Time format",
                values: &[V::TwelveHour(false), V::TwelveHour(true)],
                read: |c| V::TwelveHour(c.datetime.twelve_hour),
                write: |c, v| {
                    if let V::TwelveHour(v) = v {
                        c.datetime.twelve_hour = v
                    }
                },
                fallback: 0,
            },
            Self::TimeSeparator => ChoiceSpec {
                caption: "Time separator",
                values: &[V::Separator(':'), V::Separator('.')],
                read: |c| V::Separator(c.datetime.time_separator),
                write: |c, v| {
                    if let V::Separator(v) = v {
                        c.datetime.time_separator = v
                    }
                },
                fallback: 1,
            },
        }
    }
}
impl ChoiceSpec {
    fn next(&self, current: ChoiceValue, reverse: bool) -> ChoiceValue {
        let index = self
            .values
            .iter()
            .position(|v| *v == current)
            .unwrap_or(self.fallback);
        self.values[(index + if reverse { self.values.len() - 1 } else { 1 }) % self.values.len()]
    }
    pub fn cycle(&self, draft: &mut Config, reverse: bool) {
        (self.write)(draft, self.next((self.read)(draft), reverse));
    }
    pub fn items(&self, draft: &Config, language: Language) -> Vec<String> {
        let mut value = (self.read)(draft);
        let mut items = Vec::new();
        // At most one unknown initial value plus the explicit variant list.
        for _ in 0..=self.values.len() {
            let label = value.label(language);
            if items.contains(&label) {
                break;
            }
            items.push(label);
            value = self.next(value, false);
        }
        items
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_variants_cycle_both_ways_and_start_lists_at_the_current_value() {
        for id in KEYBOARD.iter().chain(REGIONAL) {
            let spec = id.definition();
            for value in spec.values {
                let mut draft = Config::default();
                (spec.write)(&mut draft, *value);
                let first = spec.items(&draft, Language::English);
                assert_eq!(first[0], value.label(Language::English));
                assert_eq!(first.len(), spec.values.len());
                spec.cycle(&mut draft, false);
                spec.cycle(&mut draft, true);
                assert_eq!((spec.read)(&draft), *value);
            }
        }
    }
    #[test]
    fn unknown_separator_fallbacks_match_previous_behavior() {
        let mut draft = Config::default();
        draft.datetime.date_separator = '|';
        assert_eq!(
            ChoiceId::DateSeparator
                .definition()
                .items(&draft, Language::English),
            ["|", ".", "/", "-"]
        );
        ChoiceId::DateSeparator
            .definition()
            .cycle(&mut draft, false);
        assert_eq!(draft.datetime.date_separator, '.');
        draft.datetime.time_separator = '/';
        ChoiceId::TimeSeparator
            .definition()
            .cycle(&mut draft, false);
        assert_eq!(draft.datetime.time_separator, ':');
    }
}
