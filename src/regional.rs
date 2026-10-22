//! Date/time preferences are independent of UI language; machine formats stay unchanged.
use chrono::{DateTime, Datelike, TimeZone, Timelike};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DateOrder {
    #[default]
    Ymd,
    Dmy,
    Mdy,
}
impl DateOrder {
    pub fn key(self) -> &'static str {
        match self {
            Self::Ymd => "ymd",
            Self::Dmy => "dmy",
            Self::Mdy => "mdy",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "dmy" => Self::Dmy,
            "mdy" => Self::Mdy,
            _ => Self::Ymd,
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Ymd => Self::Dmy,
            Self::Dmy => Self::Mdy,
            Self::Mdy => Self::Ymd,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTimeFormat {
    pub date_order: DateOrder,
    pub date_separator: char,
    pub twelve_hour: bool,
    pub time_separator: char,
}
impl Default for DateTimeFormat {
    fn default() -> Self {
        Self {
            date_order: DateOrder::Ymd,
            date_separator: '-',
            twelve_hour: false,
            time_separator: ':',
        }
    }
}
impl DateTimeFormat {
    pub fn date<T: Datelike>(self, t: &T, short: bool) -> String {
        let y = if short {
            format!("{:02}", t.year().rem_euclid(100))
        } else {
            format!("{:04}", t.year())
        };
        let m = format!("{:02}", t.month());
        let d = format!("{:02}", t.day());
        let s = self.date_separator;
        match self.date_order {
            DateOrder::Ymd => format!("{y}{s}{m}{s}{d}"),
            DateOrder::Dmy => format!("{d}{s}{m}{s}{y}"),
            DateOrder::Mdy => format!("{m}{s}{d}{s}{y}"),
        }
    }
    pub fn time<T: Timelike>(self, t: &T, seconds: bool) -> String {
        let hour = if self.twelve_hour {
            let h = t.hour() % 12;
            if h == 0 {
                12
            } else {
                h
            }
        } else {
            t.hour()
        };
        let s = self.time_separator;
        let mut text = format!("{hour:02}{s}{:02}", t.minute());
        if seconds {
            text.push_str(&format!("{s}{:02}", t.second()));
        }
        if self.twelve_hour {
            text.push_str(if t.hour() < 12 { " AM" } else { " PM" });
        }
        text
    }
    pub fn stamp<T: Datelike + Timelike>(self, t: &T, short: bool, seconds: bool) -> String {
        format!("{} {}", self.date(t, short), self.time(t, seconds))
    }
    pub fn clock<Tz: TimeZone>(self, t: &DateTime<Tz>) -> String {
        format!(" {} ", self.time(t, true))
    }
    pub fn clock_width(self) -> u16 {
        if self.twelve_hour {
            13
        } else {
            10
        }
    }
    pub fn detail_width(self) -> u16 {
        if self.twelve_hour {
            19
        } else {
            16
        }
    }
    pub fn footer_width(self) -> u16 {
        if self.twelve_hour {
            17
        } else {
            14
        }
    }
    pub fn information(self, source: &str) -> String {
        DateTime::parse_from_str(source, "%Y-%m-%d %H:%M:%S %:z")
            .map(|t| format!("{} {}", self.stamp(&t, false, true), t.format("%:z")))
            .unwrap_or_else(|_| source.into())
    }
    pub fn archive(self, source: &str) -> String {
        use chrono::NaiveDateTime;
        for pattern in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
            if let Ok(t) = NaiveDateTime::parse_from_str(
                source.strip_suffix(" UTC").unwrap_or(source),
                pattern,
            ) {
                return format!(
                    "{}{}",
                    self.stamp(&t, false, true),
                    if source.ends_with(" UTC") { " UTC" } else { "" }
                );
            }
        }
        source.into()
    }
}
