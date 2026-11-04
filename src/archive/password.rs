//! Shared ephemeral credentials. Never implement Debug or serialization here.
use std::sync::Arc;
use zeroize::Zeroize;
struct Secret(Vec<u8>);
impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
#[derive(Clone)]
pub struct Password(Arc<Secret>);
impl Password {
    pub fn new(value: &str) -> Self {
        Self(Arc::new(Secret(value.as_bytes().to_vec())))
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0 .0
    }
}
