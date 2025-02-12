use log::{debug, warn};

<<<<<<< HEAD
use crate::content::Content;
use crate::Stream;
use crate::{encodings::Encoding, parser_aux::try_to_replace_encoded_text, Error, Result};
use std::collections::BTreeMap;
use std::io::Write;

impl Stream {
    pub fn replace_text(&self, encodings: &BTreeMap<Vec<u8>, Encoding>, text: &str, other_text: &str) -> Result<Self> {
=======
use crate::content::{Content, Operation};
use crate::fonts::Fonts;
use crate::parser_aux::{highlight_encoded_text, redact_encoded_text};
use crate::{encodings::Encoding, Error, Result};
use crate::{Document, ObjectId, Stream};
use std::collections::HashMap;
use std::io::Write;

impl Stream {
    pub fn redact_text(&self, document: &Document, page_id: ObjectId, text: &str) -> Result<Self> {
        self.apply_text(&document, page_id, &text, redact_encoded_text)
    }

    pub fn highlight_text(&self, document: &Document, page_id: ObjectId, text: &str) -> Result<Self> {
        self.apply_text(&document, page_id, &text, highlight_encoded_text)
    }

    fn apply_text<F>(&self, document: &Document, page_id: ObjectId, text: &str, applier: F) -> Result<Self>
    where
        F: for<'a> Fn(&'a Vec<Operation>, usize, &'a Encoding, &'a Fonts, &str) -> Result<Vec<Vec<Operation>>>,
    {
        let encodings = document.get_encodings(page_id)?;
>>>>>>> 202afb8 (WIP)
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
<<<<<<< HEAD
        let mut content = Content::decode(&raw_content)?;
        let mut current_encoding = None;
        for operation in &mut content.operations {
            match operation.operator.as_ref() {
                "Tf" => {
                    let current_font = operation
=======
        let content = Content::decode(&raw_content)?;
        let mut current_encoding: Option<&Encoding> = None;
        let mut current_font: Option<&Fonts> = None;

        let mut font_cache = HashMap::<&[u8], Fonts>::new();

        let mut new_operations = Vec::<Operation>::new();
        let mut buffered_tj_operations = Vec::<Vec<Operation>>::new();

        for (oid, operation) in content.operations.iter().enumerate() {
            match operation.operator.as_ref() {
                "Tf" => {
                    // Get the font name operand and create an owned key
                    let font_name = operation
>>>>>>> 202afb8 (WIP)
                        .operands
                        .first()
                        .ok_or_else(|| Error::Syntax("missing font operand".to_string()))?
                        .as_name()?;
<<<<<<< HEAD
                    current_encoding = encodings.get(current_font);
                }
                "Tj" => match current_encoding {
                    Some(encoding) => try_to_replace_encoded_text(operation, encoding, text, &other_text)?,
=======
                    // Check font size operand
                    let font_size = operation
                        .operands
                        .get(1)
                        .ok_or_else(|| Error::Syntax("missing font size operand".to_string()))?
                        .as_i64();
                    if font_size.is_err() {
                        continue;
                    }

                    current_encoding = encodings.get(font_name);

                    let font_key = font_name; // Create an owned key
                    if let Some(cached_font) = font_cache.get(&font_key) {
                        current_font = Some(cached_font);
                    } else {
                        if let Some(font) = document.get_font(page_id, font_name)? {
                            font_cache.insert(font_key, font);
                            current_font = Some(font_cache.get(&font_key).unwrap());
                        }
                    }

                    new_operations.push(operation.clone());
                }
                "Tj" | "TJ" => match current_encoding {
                    Some(encoding) => {
                        if !buffered_tj_operations.is_empty() {
                            let first_tj = buffered_tj_operations.first().unwrap();
                            new_operations.extend(first_tj.iter().cloned());
                            buffered_tj_operations = buffered_tj_operations.into_iter().skip(1).collect();
                            continue;
                        }

                        // current_font is expected to be Some because a Tf operation should precede this
                        let font = current_font
                            .as_ref()
                            .expect("Font should be set before text operations");
                        let redact_left_operations = applier(&content.operations, oid, encoding, font, text)?;
                        let first_tj = redact_left_operations.first().unwrap();
                        new_operations.extend(first_tj.iter().cloned());
                        buffered_tj_operations.extend(redact_left_operations.into_iter().skip(1));
                    }
>>>>>>> 202afb8 (WIP)
                    None => {
                        warn!("Could not decode extracted text, some of the occurances might not be properly replaced")
                    }
                },
<<<<<<< HEAD
                _ => {}
            }
        }

        let mut new_stream = Self::new(self.dict.clone(), content.encode()?);
        new_stream.set_plain_content(content.encode()?);
=======
                _ => new_operations.push(operation.clone()),
            }
        }

        let new_content = Content {
            operations: new_operations,
        };
        let mut new_stream = self.clone();
        new_stream.set_plain_content(new_content.encode()?);
>>>>>>> 202afb8 (WIP)
        if is_compressed {
            new_stream.compress()?;
        }

        Ok(new_stream)
    }
}
