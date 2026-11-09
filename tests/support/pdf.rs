//! PDF fixtures generated in memory; no generated documents enter the checkout.
#![allow(dead_code)]
use pdf_extract::{
    content::{Content, Operation},
    dictionary, Document, Object, Stream,
};
use std::io;
pub fn document(pages: &[&[u8]]) -> Document {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let cmap = doc.add_object(Stream::new(dictionary! {}, b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /DNUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfrange\n<00> <7F> <0000>\nendbfrange\n2 beginbfchar\n<80> <0416>\n<81> <044F>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend".to_vec()));
    let font = doc.add_object(dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "ToUnicode" => cmap});
    let resources = doc.add_object(dictionary! {"Font" => dictionary! {"F1" => font}});
    let mut kids = Vec::new();
    for text in pages {
        let ops = if text.is_empty() {
            vec![]
        } else {
            vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![40.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(*text)]),
                Operation::new("ET", vec![]),
            ]
        };
        let content = Content { operations: ops }.encode().unwrap();
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages_id, "Contents" => content_id, "Resources" => resources, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()]});
        kids.push(page.into());
    }
    doc.objects.insert(
        pages_id,
        dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => pages.len() as i64}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
    doc.trailer.set("Root", catalog);
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal("DN-PDF-fixture01"),
            Object::string_literal("DN-PDF-fixture01"),
        ]),
    );
    doc
}
pub fn bytes(pages: &[&[u8]]) -> io::Result<Vec<u8>> {
    let mut doc = document(pages);
    doc.compress();
    let mut out = Vec::new();
    doc.save_to(&mut out)?;
    Ok(out)
}

pub fn encrypted(pages: &[&[u8]], password: &str, aes: bool) -> io::Result<Vec<u8>> {
    use pdf_extract::{EncryptionState, EncryptionVersion, Permissions};
    let mut doc = document(pages);
    let version = if aes {
        use pdf_extract::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
        let filter: std::sync::Arc<dyn CryptFilter> = std::sync::Arc::new(Aes128CryptFilter);
        EncryptionVersion::V4 {
            document: &doc,
            encrypt_metadata: true,
            crypt_filters: std::collections::BTreeMap::from([(b"StdCF".to_vec(), filter)]),
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: "DN fixture owner",
            user_password: password,
            permissions: Permissions::all(),
        }
    } else {
        EncryptionVersion::V1 {
            document: &doc,
            owner_password: "DN fixture owner",
            user_password: password,
            permissions: Permissions::all(),
        }
    };
    let state = EncryptionState::try_from(version).map_err(io::Error::other)?;
    doc.encrypt(&state).map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)?;
    Ok(bytes)
}

pub fn encrypted_modern(pages: &[&[u8]], password: &str) -> io::Result<Vec<u8>> {
    use pdf_extract::{
        encryption::crypt_filters::{Aes256CryptFilter, CryptFilter},
        EncryptionState, EncryptionVersion, Permissions,
    };
    let mut doc = document(pages);
    doc.version = "2.0".into();
    let filter: std::sync::Arc<dyn CryptFilter> = std::sync::Arc::new(Aes256CryptFilter);
    let key = [7u8; 32];
    let state = EncryptionState::try_from(EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: std::collections::BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: "DN fixture owner",
        user_password: password,
        permissions: Permissions::all(),
    })
    .map_err(io::Error::other)?;
    doc.encrypt(&state).map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)?;
    Ok(bytes)
}
