use chrono::{DateTime, Local, TimeZone, Timelike};
pub struct Clock {
    pub text: String,
    second: Option<i64>,
}
impl Default for Clock {
    fn default() -> Self {
        Self {
            text: " --:--:-- ".into(),
            second: None,
        }
    }
}
impl Clock {
    pub fn update<Tz: TimeZone>(&mut self, now: DateTime<Tz>) -> bool {
        if self.second == Some(now.timestamp()) {
            return false;
        }
        self.second = Some(now.timestamp());
        let text = format!(
            " {:02}:{:02}:{:02} ",
            now.hour(),
            now.minute(),
            now.second()
        );
        let changed = text != self.text;
        self.text = text;
        changed
    }
    pub fn tick(&mut self) -> bool {
        self.update(Local::now())
    }
}
