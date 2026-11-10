//! Text-only PDF presentation. Never execute actions, scripts or embedded files.
pub mod password;
use super::{Contents, ViewMode};
use crate::task::Cancellation;
use std::{
    io::{self, Write},
    path::Path,
    sync::Arc,
};
pub const MAX_PAGES: usize = 2_000;
const MAX_TEXT: usize = 64 * 1024 * 1024;
pub const EMPTY_NOTICE: &str =
    "PDF has no extractable text. Scanned pages require OCR; use an external viewer.";
pub const PASSWORD_NOTICE: &str = "This PDF requires a password.";
pub const LIMIT_NOTICE: &str = "PDF viewing limit reached. Use an external PDF viewer.";
#[derive(Clone, Debug)]
pub struct Info {
    pub page_lines: Vec<usize>,
    pub empty: bool,
    pub locked: bool,
}
impl Info {
    pub fn page(&self, line: usize) -> usize {
        self.page_lines
            .partition_point(|start| *start <= line)
            .max(1)
    }
}
pub fn is_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}
pub fn signature(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-")
}
pub(crate) fn document_bytes(
    bytes: Vec<u8>,
    mode: ViewMode,
    hint: bool,
    cancel: &Cancellation,
) -> io::Result<Contents> {
    if mode == ViewMode::Auto && (hint || signature(&bytes)) {
        let bytes: Arc<[u8]> = bytes.into();
        match extract_shared(bytes.clone(), None, cancel) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Ok(Contents {
                stream: None,
                pdf: Some(Arc::new(Info {
                    page_lines: Vec::new(),
                    empty: false,
                    locked: true,
                })),
                source: None,
                raw: Some(bytes.clone()),
                format: None,
                encoding_needed: false,
                raw_truncated: false,
                loaded_bytes: bytes.len(),
                lines: vec![PASSWORD_NOTICE.into()],
                binary: false,
                truncated: false,
            }),
            result => result,
        }
    } else {
        super::from_bytes_cancelled(bytes, mode, cancel)
    }
}
struct Bounded<'a> {
    text: String,
    lines: usize,
    cancel: &'a Cancellation,
    limited: bool,
}
impl Write for Bounded<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.cancel.check()?;
        let lines = bytes.iter().filter(|b| **b == b'\n').count();
        if self.text.len().saturating_add(bytes.len()) > MAX_TEXT
            || self.lines.saturating_add(lines) >= crate::limits::VIEWER_LINES
        {
            self.limited = true;
            return Err(io::Error::other(LIMIT_NOTICE));
        }
        let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
        self.text.push_str(text);
        self.lines += lines;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub fn extract(bytes: Vec<u8>, cancel: &Cancellation) -> io::Result<Contents> {
    extract_shared(bytes.into(), None, cancel)
}
pub fn extract_with_password(
    bytes: Vec<u8>,
    password: &crate::archive::Password,
    cancel: &Cancellation,
) -> io::Result<Contents> {
    extract_shared(bytes.into(), Some(password), cancel)
}
pub(crate) fn extract_shared(
    bytes: Arc<[u8]>,
    password: Option<&crate::archive::Password>,
    cancel: &Cancellation,
) -> io::Result<Contents> {
    cancel.check()?;
    if bytes.len() > super::MAX_BYTES {
        return Err(io::Error::other(LIMIT_NOTICE));
    }
    // Some malformed PDFs make the extraction library panic. Convert unwinding
    // failures to a document error without changing foreground ownership.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        extract_inner(bytes, password, cancel)
    }))
    .unwrap_or_else(|_| {
        Err(io::Error::other(
            "Unable to extract PDF text. Use As Hex or an external viewer.",
        ))
    })
}
fn extract_inner(
    bytes: Arc<[u8]>,
    password: Option<&crate::archive::Password>,
    cancel: &Cancellation,
) -> io::Result<Contents> {
    if !signature(&bytes) {
        return Err(io::Error::other(
            "Invalid PDF header. Use As Text or As Hex to inspect the original bytes.",
        ));
    }
    let options = pdf_extract::LoadOptions {
        password: password
            .map(|p| std::str::from_utf8(p.bytes()).map(str::to_owned))
            .transpose()
            .map_err(|_| io::Error::other("Invalid PDF password input"))?,
        ..Default::default()
    };
    let doc = pdf_extract::Document::load_mem_with_options(&bytes, options).map_err(|e| {
        use pdf_extract::{encryption::DecryptionError, Error};
        match e {
            Error::InvalidPassword | Error::Decryption(DecryptionError::IncorrectPassword) => {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Incorrect PDF password. Try again.",
                )
            }
            Error::Decryption(DecryptionError::StringPrep(_)) => io::Error::new(
                io::ErrorKind::InvalidInput,
                "PDF password contains unsupported characters.",
            ),
            Error::Decryption(
                DecryptionError::UnsupportedEncryption
                | DecryptionError::UnsupportedVersion
                | DecryptionError::UnsupportedRevision,
            ) => io::Error::new(
                io::ErrorKind::Unsupported,
                "Unsupported PDF encryption. Use an external PDF viewer.",
            ),
            error => io::Error::other(format!("Unable to read PDF: {error}")),
        }
    })?;
    cancel.check()?;
    if doc.is_encrypted() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            PASSWORD_NOTICE,
        ));
    }
    let pages = doc.get_pages();
    if pages.len() > MAX_PAGES {
        return Err(io::Error::other(LIMIT_NOTICE));
    }
    if pages.is_empty() {
        return Err(io::Error::other("PDF contains no readable pages."));
    }
    let mut output = Bounded {
        text: String::new(),
        lines: 0,
        cancel,
        limited: false,
    };
    let mut starts = Vec::with_capacity(pages.len());
    let mut any_text = false;
    for number in pages.keys() {
        cancel.check()?;
        starts.push(output.lines);
        writeln!(output, "── {number} / {} ──", pages.len())?;
        let before = output.text.len();
        let result = {
            let writer: &mut dyn Write = &mut output;
            let mut device = pdf_extract::PlainTextOutput::new(writer);
            pdf_extract::output_doc_page(&doc, &mut device, *number)
        };
        cancel.check()?;
        if output.limited {
            return Err(io::Error::other(LIMIT_NOTICE));
        }
        result.map_err(|e| io::Error::other(format!("Unable to extract PDF text: {e}")))?;
        any_text |= !output.text[before..].trim().is_empty();
        if !output.text.ends_with('\n') {
            writeln!(output)?;
        }
        writeln!(output)?;
    }
    let mut contents = super::text_contents_cancelled(&output.text, cancel)?;
    contents.pdf = Some(Arc::new(Info {
        page_lines: starts,
        empty: !any_text,
        locked: false,
    }));
    contents.loaded_bytes = bytes.len();
    contents.raw = Some(bytes);
    Ok(contents)
}
