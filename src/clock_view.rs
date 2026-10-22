use chrono::{DateTime, Local, TimeZone};
pub struct Clock {
    pub text: String,
}
impl Default for Clock {
    fn default() -> Self {
        Self {
            text: " --:--:-- ".into(),
        }
    }
}
impl Clock {
    pub fn update<Tz: TimeZone>(&mut self, now: DateTime<Tz>) -> bool {
        self.update_with(now, crate::regional::DateTimeFormat::default())
    }
    pub fn update_with<Tz: TimeZone>(
        &mut self,
        now: DateTime<Tz>,
        format: crate::regional::DateTimeFormat,
    ) -> bool {
        let text = format.clock(&now);
        let changed = text != self.text;
        self.text = text;
        changed
    }
    pub fn tick(&mut self) -> bool {
        self.update(Local::now())
    }
    pub fn tick_with(&mut self, format: crate::regional::DateTimeFormat) -> bool {
        self.update_with(Local::now(), format)
    }
}
