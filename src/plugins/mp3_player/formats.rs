//! One extension registry for native opening and recursive queue imports.
use std::path::Path;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Format {
    Mp3,
    Flac,
    Wav,
    Vorbis,
}
impl Format {
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?;
        [
            (Self::Mp3, "mp3"),
            (Self::Flac, "flac"),
            (Self::Wav, "wav"),
            (Self::Wav, "wave"),
            (Self::Vorbis, "ogg"),
            (Self::Vorbis, "oga"),
        ]
        .into_iter()
        .find(|(_, name)| extension.eq_ignore_ascii_case(name))
        .map(|(format, _)| format)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extension_registry_is_case_insensitive_and_excludes_unavailable_codecs() {
        for name in ["a.mp3", "a.FLAC", "a.Wav", "a.wave", "a.OGG", "a.oga"] {
            assert!(Format::from_path(Path::new(name)).is_some(), "{name}");
        }
        for name in ["a.mp4", "a.m4a", "a.aac", "a.opus", "a.txt", "flac"] {
            assert!(Format::from_path(Path::new(name)).is_none(), "{name}");
        }
    }
}
