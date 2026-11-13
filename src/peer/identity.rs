//! Explicit, session-only labels. Never derive names from host/user information.
use std::io;
use unicode_segmentation::UnicodeSegmentation;
pub struct Identity {
    pub key: iroh::SecretKey,
    pub name: String,
}
impl Default for Identity {
    fn default() -> Self {
        let key = iroh::SecretKey::generate();
        let name = anonymous(key.public());
        Self { key, name }
    }
}
pub fn anonymous(id: iroh::EndpointId) -> String {
    format!("DN {}", &id.to_string()[..8])
}
pub fn validate(value: &str) -> io::Result<String> {
    if value.chars().any(char::is_control) || value.len() > 96 || value.graphemes(true).count() > 24
    {
        return Err(io::Error::other(
            "Use a nickname of at most 24 characters, without control characters.",
        ));
    }
    Ok(value.trim().to_owned())
}
pub fn advertised(value: Option<&iroh::address_lookup::UserData>, id: iroh::EndpointId) -> String {
    value
        .and_then(|v| v.as_ref().strip_prefix("dn-name:1:"))
        .and_then(|v| validate(v).ok())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| anonymous(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_names_are_bounded_and_anonymous_by_default() {
        let identity = Identity::default();
        assert_eq!(identity.name, anonymous(identity.key.public()));
        assert_eq!(validate("  Ёжик 💻  ").unwrap(), "Ёжик 💻");
        assert_eq!(validate("  ").unwrap(), "");
        assert!(validate("name\nother").is_err());
        assert!(validate(&"a".repeat(25)).is_err());
        assert!(validate(&"a\u{301}".repeat(24)).is_ok());
        assert!(validate(&"💻".repeat(25)).is_err());
        let data = "dn-name:1:Лаптоп".parse().unwrap();
        assert_eq!(advertised(Some(&data), identity.key.public()), "Лаптоп");
        for value in ["old name", "dn-name:1:", "dn-name:1:bad\nname"] {
            let data = value.parse().unwrap();
            assert_eq!(
                advertised(Some(&data), identity.key.public()),
                identity.name
            );
        }
        assert_eq!(advertised(None, identity.key.public()), identity.name);
    }
}
