//! Strict document codecs. Encoded bytes and editable Unicode are distinct.
use std::fmt;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf16Le,
    Utf16Be,
    Cp866,
    Windows1251,
    Koi8R,
    Windows1252,
}
impl TextEncoding {
    pub const ALL: [Self; 7] = [
        Self::Utf8,
        Self::Utf16Le,
        Self::Utf16Be,
        Self::Cp866,
        Self::Windows1251,
        Self::Koi8R,
        Self::Windows1252,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Cp866 => "CP866",
            Self::Windows1251 => "Windows-1251",
            Self::Koi8R => "KOI8-R",
            Self::Windows1252 => "Windows-1252",
        }
    }
    pub fn unicode(self) -> bool {
        matches!(self, Self::Utf8 | Self::Utf16Le | Self::Utf16Be)
    }
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.name() == name)
    }
    fn legacy(self) -> &'static encoding_rs::Encoding {
        match self {
            Self::Cp866 => encoding_rs::IBM866,
            Self::Windows1251 => encoding_rs::WINDOWS_1251,
            Self::Koi8R => encoding_rs::KOI8_R,
            Self::Windows1252 => encoding_rs::WINDOWS_1252,
            _ => unreachable!("Unicode uses explicit codecs"),
        }
    }
    pub fn default_format(self) -> TextFormat {
        TextFormat {
            encoding: self,
            bom: matches!(self, Self::Utf16Le | Self::Utf16Be),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextFormat {
    pub encoding: TextEncoding,
    pub bom: bool,
}
impl From<bool> for TextFormat {
    fn from(bom: bool) -> Self {
        Self {
            encoding: TextEncoding::Utf8,
            bom,
        }
    }
}
impl fmt::Display for TextFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            self.encoding.name(),
            if self.bom { " + BOM" } else { "" }
        )
    }
}
impl TextFormat {
    fn marker(self) -> &'static [u8] {
        if !self.bom {
            return &[];
        }
        match self.encoding {
            TextEncoding::Utf8 => b"\xef\xbb\xbf",
            TextEncoding::Utf16Le => b"\xff\xfe",
            TextEncoding::Utf16Be => b"\xfe\xff",
            _ => &[],
        }
    }
    pub fn decode(self, bytes: &[u8], truncated: bool) -> Result<String, String> {
        if self.bom && !self.encoding.unicode() {
            return Err("BOM is only available for Unicode encodings.".into());
        }
        let bytes = if self.bom {
            bytes
                .strip_prefix(self.marker())
                .ok_or("Missing or mismatched BOM")?
        } else {
            bytes
        };
        let invalid = || {
            format!(
                "Invalid {} text; choose another encoding or Hex.",
                self.encoding.name()
            )
        };
        let text = match self.encoding {
            TextEncoding::Utf8 => match std::str::from_utf8(bytes) {
                Ok(s) => s.to_owned(),
                Err(e) if truncated && e.error_len().is_none() => {
                    std::str::from_utf8(&bytes[..e.valid_up_to()])
                        .map_err(|_| invalid())?
                        .to_owned()
                }
                Err(_) => return Err(invalid()),
            },
            TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
                if !truncated && bytes.len() % 2 != 0 {
                    return Err(invalid());
                }
                let mut units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| {
                        if self.encoding == TextEncoding::Utf16Le {
                            u16::from_le_bytes([b[0], b[1]])
                        } else {
                            u16::from_be_bytes([b[0], b[1]])
                        }
                    })
                    .collect();
                if truncated && units.last().is_some_and(|u| (0xd800..=0xdbff).contains(u)) {
                    units.pop();
                }
                String::from_utf16(&units).map_err(|_| invalid())?
            }
            _ => self
                .encoding
                .legacy()
                .decode_without_bom_handling_and_without_replacement(bytes)
                .ok_or_else(invalid)?
                .into_owned(),
        };
        if text.len() > 8 * 1024 * 1024 {
            return Err("Decoded text exceeds the 8 MiB viewer limit.".into());
        }
        Ok(text)
    }
    pub fn encode(self, text: &str) -> Result<Vec<u8>, String> {
        if self.bom && !self.encoding.unicode() {
            return Err("BOM is only available for Unicode encodings.".into());
        }
        let mut bytes = self.marker().to_vec();
        match self.encoding {
            TextEncoding::Utf8 => bytes.extend_from_slice(text.as_bytes()),
            TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
                for unit in text.encode_utf16() {
                    bytes.extend_from_slice(&if self.encoding == TextEncoding::Utf16Le {
                        unit.to_le_bytes()
                    } else {
                        unit.to_be_bytes()
                    });
                }
            }
            _ => {
                let (encoded, _, errors) = self.encoding.legacy().encode(text);
                if errors {
                    return Err(format!("Some characters cannot be saved as {}. Choose UTF-8; the file was not changed.", self.encoding.name()));
                }
                bytes.extend_from_slice(&encoded);
                // Never accept mappings that do not round-trip to the original text.
                if self.decode(&bytes, false)? != text {
                    return Err(
                        "Encoding would change characters. Choose UTF-8; the file was not changed."
                            .into(),
                    );
                }
            }
        }
        Ok(bytes)
    }
}
/// No statistical guesses. UTF-32 must be checked before the UTF-16 LE prefix.
pub fn detect(bytes: &[u8], truncated: bool) -> Result<Option<TextFormat>, String> {
    if bytes.starts_with(b"\xff\xfe\0\0") || bytes.starts_with(b"\0\0\xfe\xff") {
        return Err("UTF-32 is not supported. Use Hex.".into());
    }
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Utf16Le,
        TextEncoding::Utf16Be,
    ] {
        let format = TextFormat {
            encoding,
            bom: true,
        };
        if bytes.starts_with(format.marker()) {
            format.decode(bytes, truncated)?;
            return Ok(Some(format));
        }
    }
    let format = TextFormat::default();
    Ok(format
        .decode(bytes, truncated)
        .ok()
        .filter(|s| !s.contains('\0'))
        .map(|_| format))
}
pub fn plausible_text(bytes: &[u8]) -> bool {
    !bytes
        .iter()
        .any(|b| *b < 32 && !matches!(*b, b'\t' | b'\r' | b'\n'))
}

/// An owned draft; preview is prepared when the choice changes, never in paint.
pub struct EncodingDialog {
    pub format: TextFormat,
    pub focus: usize,
    pub preview: String,
    pub error: Option<String>,
    pub saving: bool,
    pub hex: bool,
    pub(crate) bytes: std::sync::Arc<[u8]>,
    pub(crate) truncated: bool,
    pub(crate) requests: crate::requests::RequestScope,
    pending: Option<crate::task::Task<Result<String, String>>>,
}
impl EncodingDialog {
    pub fn new(
        format: TextFormat,
        bytes: std::sync::Arc<[u8]>,
        truncated: bool,
        saving: bool,
        hex: bool,
    ) -> Self {
        let mut d = Self {
            format,
            bytes,
            truncated,
            saving,
            hex,
            focus: 0,
            requests: Default::default(),
            pending: None,
            preview: String::new(),
            error: None,
        };
        d.refresh();
        d
    }
    pub fn choose(&mut self, index: usize) {
        if let Some(encoding) = TextEncoding::ALL.get(index).copied() {
            if encoding != self.format.encoding {
                self.format = encoding.default_format();
                if !self.saving {
                    self.format.bom = self.bytes.starts_with(self.format.marker())
                        && !self.format.marker().is_empty();
                }
            }
        }
        self.refresh();
    }
    pub fn refresh(&mut self) {
        self.pending = None;
        self.preview.clear();
        if self.saving {
            self.error = None;
            return;
        }
        self.error = Some("Preparing preview…".into());
        let bytes = self.bytes.clone();
        let format = self.format;
        let truncated = self.truncated;
        match self.requests.spawn("dn-encoding-preview", move |cancel| {
            cancel.check().map_err(|e| e.to_string())?;
            let text = format.decode(&bytes, truncated)?;
            cancel.check().map_err(|e| e.to_string())?;
            Ok(text
                .lines()
                .take(6)
                .map(|line| {
                    line.chars()
                        .take(64)
                        .map(|c| if c.is_control() { ' ' } else { c })
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }) {
            Ok(task) => self.pending = Some(task),
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn poll(&mut self) -> bool {
        let Some(reply) = crate::task::take_reply(&mut self.pending) else {
            return false;
        };
        match reply {
            Ok(Ok(text)) => {
                self.preview = text;
                self.error = None;
            }
            Ok(Err(error)) => self.error = Some(error),
            Err(_) => self.error = Some("Preview worker stopped".into()),
        }
        true
    }
}

pub(crate) fn spec<'a>(
    app: &'a crate::app::App,
    area: crate::presentation::layout::Rect,
    dialog: &EncodingDialog,
    editor: bool,
) -> Option<crate::turbo::dialog_spec::DialogSpec<'a>> {
    use crate::{
        editor::EditorAction as E,
        mouse::Target,
        painting::Paragraph,
        presentation::layout::Rect,
        turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec},
        viewer::Action as V,
    };
    let target = |index: usize| {
        if editor {
            Target::Editor(E::EncodingChoice(index))
        } else {
            Target::Viewer(V::EncodingChoice(index))
        }
    };
    let accept = if editor {
        Target::Editor(E::EncodingAccept)
    } else {
        Target::Viewer(V::EncodingAccept)
    };
    let cancel = if editor {
        Target::Editor(E::Cancel)
    } else {
        Target::Viewer(V::Cancel)
    };
    let rect = centered(area, 74, 21);
    let mut spec = DialogSpec::new(
        rect,
        scope(
            app,
            if editor {
                crate::focus::Focus::EditorModal
            } else {
                crate::focus::Focus::ViewerSearch
            },
        )?,
        dialog.focus,
        app.config.language.text(if dialog.saving {
            " Save with encoding "
        } else {
            " Open with encoding "
        }),
        accept,
        cancel,
    );
    for (i, encoding) in TextEncoding::ALL.iter().enumerate() {
        spec.control(Control::new(
            i,
            Rect::new(rect.x + 3, rect.y + 2 + i as u16, 25, 1),
            ControlKind::Radio {
                caption: encoding.name().into(),
                checked: *encoding == dialog.format.encoding,
            },
            target(i),
        ));
    }
    if dialog.saving {
        let mut control = Control::new(
            7,
            Rect::new(rect.x + 34, rect.y + 2, 31, 1),
            ControlKind::Checkbox {
                caption: app.config.language.text("Write BOM").into(),
                checked: dialog.format.bom,
            },
            target(7),
        );
        control.enabled = dialog.format.encoding.unicode();
        spec.control(control);
        spec.memo(app.config.language.text("F2 keeps the current encoding.\nUnrepresentable characters stop saving.\nLine endings remain unchanged."), Rect::new(rect.x + 3, rect.y + 11, 66, 3));
    } else {
        spec.caption(
            Paragraph::new(app.config.language.text("Preview:")),
            Rect::new(rect.x + 3, rect.y + 10, 66, 1),
        );
        spec.memo(&dialog.preview, Rect::new(rect.x + 3, rect.y + 11, 66, 6));
    }
    if let Some(error) = &dialog.error {
        spec.memo(
            app.config.language.text(error),
            Rect::new(rect.x + 31, rect.y + 4, 38, 5),
        );
    }
    let mut control = Control::new(
        8,
        Rect::new(rect.x + 5, rect.bottom() - 3, 18, 1),
        ControlKind::Button(
            app.config
                .language
                .text(if dialog.saving {
                    " [ Save ] "
                } else {
                    " [ Open ] "
                })
                .into(),
        ),
        accept,
    );
    control.enabled = dialog.error.is_none();
    spec.control(control);
    if dialog.hex {
        spec.control(Control::new(
            9,
            Rect::new(rect.x + 28, rect.bottom() - 3, 16, 1),
            ControlKind::Button(" Hex ".into()),
            Target::Viewer(V::EncodingHex),
        ));
    }
    spec.control(Control::new(
        10,
        Rect::new(rect.x + 50, rect.bottom() - 3, 18, 1),
        ControlKind::Button(app.config.language.text(" [ Cancel ] ").into()),
        cancel,
    ));
    Some(spec)
}
