use chrono::{DateTime, Local};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn grouped_bytes(bytes: u128) -> String {
    let digits = bytes.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    result
}

/// Fit display text from the left, preserving the end of long paths/names.
pub fn tail(text: &str, width: u16) -> String {
    use ratatui::text::Span;
    let width = usize::from(width);
    if Span::raw(text).width() <= width {
        return text.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 1;
    let mut suffix = Vec::new();
    for character in text.chars().rev() {
        let character_width = Span::raw(character.to_string()).width();
        if used + character_width > width {
            break;
        }
        suffix.push(character);
        used += character_width;
    }
    std::iter::once('…')
        .chain(suffix.into_iter().rev())
        .collect()
}

/// Convert fallibly rather than using chrono's infallible SystemTime conversion.
pub fn local_timestamp(time: SystemTime) -> Option<DateTime<Local>> {
    let (seconds, nanos) = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => (
            i64::try_from(duration.as_secs()).ok()?,
            duration.subsec_nanos(),
        ),
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs()).ok()?.checked_neg()?;
            if duration.subsec_nanos() == 0 {
                (seconds, 0)
            } else {
                (
                    seconds.checked_sub(1)?,
                    1_000_000_000 - duration.subsec_nanos(),
                )
            }
        }
    };
    Some(DateTime::from_timestamp(seconds, nanos)?.with_timezone(&Local))
}

pub fn modified(time: Option<SystemTime>, with_time: bool) -> String {
    time.and_then(local_timestamp)
        .map(|time| {
            time.format(if with_time {
                "%Y-%m-%d %H:%M"
            } else {
                "%Y-%m-%d"
            })
            .to_string()
        })
        .unwrap_or_else(|| "—".into())
}

pub fn size(bytes: u64) -> String {
    if bytes <= 999_999_999 {
        return bytes.to_string();
    }
    for (power, unit) in [
        (60, "EiB"),
        (50, "PiB"),
        (40, "TiB"),
        (30, "GiB"),
        (20, "MiB"),
    ] {
        let scale = 1_u64 << power;
        if bytes >= scale {
            let tenths = (u128::from(bytes) * 10 + u128::from(scale) / 2) / u128::from(scale);
            return format!("{}.{}{unit}", tenths / 10, tenths % 10);
        }
    }
    format!("{:.1} MiB", bytes as f64 / (1_u64 << 20) as f64)
}
