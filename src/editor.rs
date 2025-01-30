use log::{debug, warn};

use crate::content::Content;
use crate::Stream;
use crate::{encodings::Encoding, parser_aux::try_to_replace_encoded_text, Error, Result};
use std::collections::BTreeMap;
use std::io::Write;

impl Stream {
    pub fn replace_text(&self, encodings: &BTreeMap<Vec<u8>, Encoding>, text: &str, other_text: &str) -> Result<Self> {
        let mut raw_content = Vec::<u8>::new();
        let mut is_compressed = false;
        match self.decompressed_content() {
            Ok(data) => {
                raw_content.write_all(&data)?;
                is_compressed = true;
            }
            Err(_) => {
                raw_content.write_all(&self.content)?;
            }
        };
        let mut content = Content::decode(&raw_content)?;
        let mut current_encoding = None;
        for operation in &mut content.operations {
            match operation.operator.as_ref() {
                "Tf" => {
                    let current_font = operation
                        .operands
                        .first()
                        .ok_or_else(|| Error::Syntax("missing font operand".to_string()))?
                        .as_name()?;
                    current_encoding = encodings.get(current_font);
                }
                "Tj" => match current_encoding {
                    Some(encoding) => try_to_replace_encoded_text(operation, encoding, text, &other_text)?,
                    None => {
                        warn!("Could not decode extracted text, some of the occurances might not be properly replaced")
                    }
                },
                _ => {}
            }
        }

        let mut new_stream = Self::new(self.dict.clone(), content.encode()?);
        new_stream.set_plain_content(content.encode()?);
        if is_compressed {
            new_stream.compress()?;
        }

        Ok(new_stream)
    }
}
