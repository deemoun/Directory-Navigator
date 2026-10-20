#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Integer(i64),
    Real(f64),
}
impl Value {
    pub fn representations(self) -> [Result<String, String>; 4] {
        let decimal = match self {
            Self::Integer(n) => n.to_string(),
            Self::Real(n) => {
                let text = n.to_string();
                if text.len() > 64 {
                    format!("{n:e}")
                } else {
                    text
                }
            }
        };
        let integer = match self {
            Self::Integer(n) => Ok(n),
            Self::Real(n)
                if n.is_finite()
                    && n.fract() == 0.0
                    && n >= i64::MIN as f64
                    && n < -(i64::MIN as f64)
                    && (n as i64) as f64 == n =>
            {
                Ok(n as i64)
            }
            _ => Err("Unavailable: result must be an exact signed 64-bit integer".to_string()),
        };
        let base = |radix| {
            integer
                .as_ref()
                .map(|n| {
                    let sign = if *n < 0 { "-" } else { "" };
                    let magnitude = n.unsigned_abs();
                    match radix {
                        16 => format!("{sign}0x{magnitude:X}"),
                        2 => format!("{sign}0b{magnitude:b}"),
                        _ => format!("{sign}0o{magnitude:o}"),
                    }
                })
                .map_err(Clone::clone)
        };
        [Ok(decimal), base(16), base(2), base(8)]
    }
}
#[derive(Clone, Copy)]
enum Number {
    Int(i128),
    Float(f64),
}
impl Number {
    fn checked(self) -> Result<Self, String> {
        match self {
            Self::Int(n) if i64::try_from(n).is_err() => {
                Err("Signed 64-bit integer overflow".into())
            }
            Self::Float(n) if !n.is_finite() => {
                Err("Non-finite result or invalid numerical domain".into())
            }
            _ => Ok(self),
        }
    }
    fn real(self) -> f64 {
        match self {
            Self::Int(n) => n as f64,
            Self::Float(n) => n,
        }
    }
    fn binary(self, rhs: Self, op: u8) -> Result<Self, String> {
        self.checked()?;
        rhs.checked()?;
        if matches!(op, b'/' | b'%') && rhs.real() == 0.0 {
            return Err("Division by zero".into());
        }
        let result = match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => match op {
                b'+' => Self::Int(a + b),
                b'-' => Self::Int(a - b),
                b'*' => Self::Int(a * b),
                b'/' if a % b == 0 => Self::Int(a / b),
                b'%' => Self::Int(a % b),
                b'^' if b >= 0 => Self::Int(if a == 0 {
                    if b == 0 {
                        1
                    } else {
                        0
                    }
                } else if a == 1 {
                    1
                } else if a == -1 {
                    if b % 2 == 0 {
                        1
                    } else {
                        -1
                    }
                } else {
                    a.checked_pow(u32::try_from(b).map_err(|_| "Integer power overflow")?)
                        .ok_or("Integer power overflow")?
                }),
                _ => Self::Float(float_op(a as f64, b as f64, op)),
            },
            (a, b) => Self::Float(float_op(a.real(), b.real(), op)),
        };
        result.checked()
    }
}
fn float_op(a: f64, b: f64, op: u8) -> f64 {
    match op {
        b'+' => a + b,
        b'-' => a - b,
        b'*' => a * b,
        b'/' => a / b,
        b'%' => a % b,
        _ => a.powf(b),
    }
}
struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
    depth: usize,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self
            .bytes
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }
    fn take(&mut self, byte: u8) -> bool {
        self.space();
        if self.bytes.get(self.position) == Some(&byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }
    fn sum(&mut self) -> Result<Number, String> {
        let mut n = self.product()?;
        loop {
            let op = if self.take(b'+') {
                b'+'
            } else if self.take(b'-') {
                b'-'
            } else {
                break;
            };
            n = n.binary(self.product()?, op)?;
        }
        Ok(n)
    }
    fn product(&mut self) -> Result<Number, String> {
        let mut n = self.unary()?;
        loop {
            let op = if self.take(b'*') {
                b'*'
            } else if self.take(b'/') {
                b'/'
            } else if self.take(b'%') {
                b'%'
            } else {
                break;
            };
            n = n.binary(self.unary()?, op)?;
        }
        Ok(n)
    }
    fn unary(&mut self) -> Result<Number, String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("Expression nesting limit is 64".into());
        }
        let value = if self.take(b'-') {
            match self.unary()? {
                Number::Int(n) => Number::Int(-n).checked()?,
                Number::Float(n) => Number::Float(-n).checked()?,
            }
        } else if self.take(b'+') {
            self.unary()?.checked()?
        } else {
            self.power()?
        };
        self.depth -= 1;
        Ok(value)
    }
    fn power(&mut self) -> Result<Number, String> {
        let n = self.primary()?;
        if self.take(b'^') {
            n.binary(self.unary()?, b'^')
        } else {
            Ok(n)
        }
    }
    fn primary(&mut self) -> Result<Number, String> {
        if self.take(b'(') {
            let n = self.sum()?;
            if !self.take(b')') {
                return Err("Expected closing parenthesis".into());
            }
            return Ok(n);
        }
        self.space();
        let start = self.position;
        if self.bytes.get(start).is_some_and(u8::is_ascii_alphabetic) {
            while self
                .bytes
                .get(self.position)
                .is_some_and(u8::is_ascii_alphabetic)
            {
                self.position += 1;
            }
            let function = &self.bytes[start..self.position];
            if !self.take(b'(') {
                return Err("Expected function argument in parentheses".into());
            }
            let n = self.sum()?.checked()?;
            if !self.take(b')') {
                return Err("Expected closing parenthesis".into());
            }
            return match function {
                b"abs" => match n {
                    Number::Int(n) => Number::Int(n.abs()).checked(),
                    Number::Float(n) => Number::Float(n.abs()).checked(),
                },
                b"sqrt" if n.real() >= 0.0 => Number::Float(n.real().sqrt()).checked(),
                b"sqrt" => Err("sqrt requires a non-negative argument".into()),
                _ => Err("Unknown function; supported: sqrt, abs".into()),
            };
        }
        let radix = if self.bytes.get(start) == Some(&b'0') {
            match self.bytes.get(start + 1) {
                Some(b'x') | Some(b'X') => 16,
                Some(b'b') | Some(b'B') => 2,
                Some(b'o') | Some(b'O') => 8,
                _ => 10,
            }
        } else {
            10
        };
        if radix != 10 {
            self.position += 2;
            let digits = self.position;
            while self
                .bytes
                .get(self.position)
                .is_some_and(u8::is_ascii_alphanumeric)
            {
                self.position += 1;
            }
            let text = std::str::from_utf8(&self.bytes[digits..self.position])
                .map_err(|_| "Invalid integer")?;
            let n = i128::from_str_radix(text, radix)
                .map_err(|_| "Invalid or overflowing integer literal")?;
            if n > i64::MAX as i128 + 1 {
                return Err("Signed 64-bit integer overflow".into());
            }
            return Ok(Number::Int(n));
        }
        while self
            .bytes
            .get(self.position)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
        {
            self.position += 1;
        }
        if matches!(self.bytes.get(self.position), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.bytes.get(self.position), Some(b'+' | b'-')) {
                self.position += 1;
            }
            while self
                .bytes
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
        }
        let text =
            std::str::from_utf8(&self.bytes[start..self.position]).map_err(|_| "Invalid number")?;
        if text.is_empty() {
            return Err("Expected a number or expression".into());
        }
        if text.contains(['.', 'e', 'E']) {
            Number::Float(text.parse().map_err(|_| "Invalid real number")?).checked()
        } else {
            let n: i128 = text.parse().map_err(|_| "Invalid or overflowing integer")?;
            if n > i64::MAX as i128 + 1 {
                Err("Signed 64-bit integer overflow".into())
            } else {
                Ok(Number::Int(n))
            }
        }
    }
}
pub fn evaluate(expression: &str) -> Result<Value, String> {
    if expression.len() > 2048 {
        return Err("Expression limit is 2048 bytes".into());
    }
    let mut p = Parser {
        bytes: expression.as_bytes(),
        position: 0,
        depth: 0,
    };
    let n = p.sum()?.checked()?;
    p.space();
    if p.position != p.bytes.len() {
        return Err(format!("Unexpected input at byte {}", p.position + 1));
    }
    Ok(match n {
        Number::Int(n) => Value::Integer(n as i64),
        Number::Float(n) => Value::Real(n),
    })
}
#[derive(Default)]
pub struct Calculator {
    pub expression: String,
    pub cursor: usize,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub base: usize,
    pub history: Vec<String>,
    pub history_cursor: Option<usize>,
    pub draft: String,
    pub focus: usize,
}
impl Calculator {
    pub fn evaluate(&mut self) {
        match evaluate(&self.expression) {
            Ok(value) => {
                self.result = Some(value);
                self.error = None;
                self.history.retain(|s| s != &self.expression);
                self.history.insert(0, self.expression.clone());
                self.history.truncate(100);
                self.history_cursor = None;
            }
            Err(error) => {
                self.error = Some(error);
                self.result = None;
            }
        }
    }
    pub fn recall(&mut self, older: bool) {
        if self.history.is_empty() || (!older && self.history_cursor.is_none()) {
            return;
        }
        let next = if older {
            if self.history_cursor.is_none() {
                self.draft = self.expression.clone();
            }
            Some(
                self.history_cursor
                    .map_or(0, |n| (n + 1).min(self.history.len() - 1)),
            )
        } else {
            self.history_cursor.and_then(|n| n.checked_sub(1))
        };
        self.expression = next.map_or_else(|| self.draft.clone(), |n| self.history[n].clone());
        self.cursor = self.expression.len();
        self.history_cursor = next;
    }
    pub fn copy_text(&self) -> Result<String, String> {
        if self.error.is_some() {
            return Err("Evaluate a valid expression first".into());
        }
        self.result
            .ok_or("Evaluate an expression first")?
            .representations()[self.base]
            .clone()
    }
}
