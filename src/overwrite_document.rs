use std::path::Path;

use crate::{Dictionary, Document, Object, ObjectId, Result};

#[derive(Debug, Clone)]
pub struct OverwriteDocument {
    /// The raw data for the files read from input.
    pub bytes_documents: Vec<u8>,

    pub document: Document,

    pub is_encrypted: bool,
    pub password: Option<String>,
}

impl OverwriteDocument {
    pub fn new(bytes: Vec<u8>) -> Self {
        Document::load_from(bytes.as_slice())
            .map(|document| {
                let is_encrypted = document.is_encrypted();

                Self {
                    bytes_documents: bytes,
                    document,
                    is_encrypted,
                    password: None,
                }
            })
            .unwrap()
    }

    pub fn set_password(&mut self, password: String) {
        if !self.is_encrypted {
            panic!("Document is not encrypted");
        }
        self.password = Some(password);
    }

    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Ok(Self::new(bytes))
    }
}
