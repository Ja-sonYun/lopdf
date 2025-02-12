#![cfg(feature = "nom_parser")]
use log::{debug, warn};
<<<<<<< HEAD
=======
use nom::AsBytes;
>>>>>>> 202afb8 (WIP)

use crate::{
    content::{Content, Operation},
    document::Document,
    encodings::Encoding,
    error::ParseError,
    fonts::Fonts,
    object::Object::Name,
    parser::ParserInput,
    styles::text::{TextOperation, TextStyle},
    xref::{Xref, XrefEntry, XrefType},
    Error, Result,
};
use crate::{parser, Dictionary, Object, ObjectId, Stream};
use std::{
    collections::BTreeMap,
    io::{self, Cursor, Read, Write},
};

impl Content<Vec<Operation>> {
    /// Decode content operations.
    pub fn decode(data: &[u8]) -> Result<Self> {
        parser::content(ParserInput::new_extra(data, "content operations"))
            .ok_or(ParseError::InvalidContentStream.into())
    }
}

impl Stream {
    /// Decode content after decoding all stream filters.
    pub fn decode_content(&self) -> Result<Content<Vec<Operation>>> {
        Content::decode(&self.content)
    }
}

pub fn get_text_chunks_from_content(
    content: &Content<Vec<Operation>>, encodings: &BTreeMap<Vec<u8>, Encoding>,
) -> Result<Vec<Result<String>>> {
    fn collect_text(text: &mut String, encoding: &Encoding, operands: &[Object]) -> Result<()> {
        for operand in operands.iter() {
            match operand {
                Object::String(bytes, _) => {
                    text.push_str(&Document::decode_text(encoding, bytes)?);
                }
                Object::Array(arr) => {
                    collect_text(text, encoding, arr)?;
                    text.push(' ');
                }
                Object::Integer(i) => {
                    if *i < -100 {
                        text.push(' ');
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    let mut collected_chunks_and_errs: Vec<std::result::Result<String, Error>> = Vec::new();

    // each text with different encoding is extracted as separate chunk
    let mut current_encoding = None;
    let mut current_text = String::new();
    for operation in &content.operations {
        match operation.operator.as_ref() {
            "Tf" => {
                let current_font = operation
                    .operands
                    .first()
                    .ok_or_else(|| Error::Syntax("missing font operand".to_string()))?
                    .as_name();
                current_encoding = match current_font {
                    Ok(font) => encodings.get(font),
                    Err(err) => {
                        collected_chunks_and_errs.push(Err(err));
                        None
                    }
                };

                if !current_text.is_empty() {
                    collected_chunks_and_errs.push(Ok(current_text));
                    current_text = String::new();
                }
            }
            "Tj" | "TJ" => match current_encoding {
                Some(encoding) => {
                    let res = collect_text(&mut current_text, encoding, &operation.operands);
                    if let Err(err) = res {
                        collected_chunks_and_errs.push(Err(err));
                    }
                }
                None => warn!("Could not decode extracted text"),
            },
            "ET" => {
                if !current_text.ends_with('\n') {
                    current_text.push('\n')
                }
            }
            _ => {}
        }
    }
    if !current_text.is_empty() {
        collected_chunks_and_errs.push(Ok(current_text));
    }

    Ok(collected_chunks_and_errs)
}

impl Document {
    /// Get decoded page content;
    pub fn get_and_decode_page_content(&self, page_id: ObjectId) -> Result<Content<Vec<Operation>>> {
        let content_data = self.get_page_content(page_id)?;
        Content::decode(&content_data)
    }

    /// Add content to a page. All existing content will be unchanged.
    pub fn add_to_page_content(&mut self, page_id: ObjectId, content: Content<Vec<Operation>>) -> Result<()> {
        let content_data = Content::encode(&content)?;
        self.add_page_contents(page_id, content_data)?;
        Ok(())
    }

    pub fn extract_text(&self, page_numbers: &[u32]) -> Result<String> {
        let text_fragments = self.extract_text_chunks(page_numbers);
        let mut text = String::new();
        for maybe_text_fragment in text_fragments.into_iter() {
            let text_fragment = maybe_text_fragment?;
            text.push_str(&text_fragment);
        }

        Ok(text)
    }

    pub fn extract_text_chunks(&self, page_numbers: &[u32]) -> Vec<Result<String>> {
        let pages: BTreeMap<u32, (u32, u16)> = self.get_pages();
        page_numbers
            .iter()
            .flat_map(|page_number| {
                let result = self.extract_text_chunks_from_page(&pages, *page_number);
                match result {
                    Ok(text_chunks) => text_chunks,
                    Err(err) => vec![Err(err)],
                }
            })
            .collect()
    }

    fn extract_text_chunks_from_content(
        &self, content: &Content<Vec<Operation>>, encodings: &BTreeMap<Vec<u8>, Encoding>,
    ) -> Result<Vec<Result<String>>> {
        get_text_chunks_from_content(content, encodings)
    }

    fn extract_text_chunks_from_page(
        &self, pages: &BTreeMap<u32, (u32, u16)>, page_number: u32,
    ) -> Result<Vec<Result<String>>> {
        let mut collected_chunks_and_errs: Vec<std::result::Result<String, Error>> = Vec::new();

        let page_id = *pages.get(&page_number).ok_or(Error::PageNumberNotFound(page_number))?;
        let fonts = self.get_page_fonts(page_id)?;
        let encodings: BTreeMap<Vec<u8>, Encoding> = fonts
            .into_iter()
            .filter_map(|(name, font)| match font.get_font_encoding(self) {
                Ok(it) => Some((name, it)),
                Err(err) => {
                    collected_chunks_and_errs.push(Err(err));
                    None
                }
            })
            .collect();
        let content_data = self.get_page_content(page_id)?;
        let content = Content::decode(&content_data)?;

        Ok(collected_chunks_and_errs
            .into_iter()
            .chain(self.extract_text_chunks_from_content(&content, &encodings)?.into_iter())
            .collect())
    }

    pub fn replace_text(&mut self, page_number: u32, text: &str, other_text: &str) -> Result<()> {
        let page = page_number.saturating_sub(1) as usize;
        let page_id = self
            .page_iter()
            .nth(page)
            .ok_or(Error::PageNumberNotFound(page_number))?;
        let encodings: BTreeMap<Vec<u8>, Encoding> = self
            .get_page_fonts(page_id)?
            .into_iter()
            .map(|(name, font)| font.get_font_encoding(self).map(|it| (name, it)))
            .collect::<Result<BTreeMap<Vec<u8>, Encoding>>>()?;
        let content_data = self.get_page_content(page_id)?;
        let mut content = Content::decode(&content_data)?;
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
                    Some(encoding) => try_to_replace_encoded_text(operation, encoding, text, other_text)?,
                    None => {
                        warn!("Could not decode extracted text, some of the occurances might not be properly replaced")
                    }
                },
                _ => {}
            }
        }
        let modified_content = content.encode()?;
        self.change_page_content(page_id, modified_content)
    }

    pub fn insert_image(
        &mut self, page_id: ObjectId, img_object: Stream, position: (f32, f32), size: (f32, f32),
    ) -> Result<()> {
        let img_id = self.add_object(img_object);
        let img_name = format!("X{}", img_id.0);

        self.add_xobject(page_id, img_name.as_bytes(), img_id)?;

        let mut content = self.get_and_decode_page_content(page_id)?;
        content.operations.push(Operation::new("q", vec![]));
        content.operations.push(Operation::new(
            "cm",
            vec![
                size.0.into(),
                0.into(),
                0.into(),
                size.1.into(),
                position.0.into(),
                position.1.into(),
            ],
        ));
        content
            .operations
            .push(Operation::new("Do", vec![Name(img_name.as_bytes().to_vec())]));
        content.operations.push(Operation::new("Q", vec![]));

        self.change_page_content(page_id, content.encode()?)
    }

    pub fn insert_form_object(&mut self, page_id: ObjectId, form_obj: Stream) -> Result<()> {
        let form_id = self.add_object(form_obj);
        let form_name = format!("X{}", form_id.0);

        let mut content = self.get_and_decode_page_content(page_id)?;
        content.operations.insert(0, Operation::new("q", vec![]));
        content.operations.push(Operation::new("Q", vec![]));
        content
            .operations
            .push(Operation::new("Do", vec![Name(form_name.as_bytes().to_vec())]));
        let modified_content = content.encode()?;
        self.add_xobject(page_id, form_name, form_id)?;

        self.change_page_content(page_id, modified_content)
    }
}

pub fn try_to_replace_encoded_text(
    operation: &mut Operation, encoding: &Encoding, text_to_replace: &str, replacement: &str,
) -> Result<()> {
    debug!("Replacing text in operation: {:?}", operation);
    for bytes in operation.operands.iter_mut().flat_map(Object::as_str_mut) {
        let decoded_text = Document::decode_text(encoding, bytes)?;
<<<<<<< HEAD
        debug!("decoded_text: {:?}", decoded_text);
        println!("decoded_text: {:?}", decoded_text);
        // *bytes = bytes.clone();
        // // If text is included in the decoded text, replace it.
        if decoded_text.contains(text_to_replace) {
            let new_text = decoded_text.replace(text_to_replace, replacement);
            debug!("new_text: {:?}", new_text);
            println!("new_text: {:?}", new_text);
            let encoded_bytes = Document::encode_text(encoding, &new_text);
            // debug!("encoded_bytes: {:?}", String::from_utf8(encoded_bytes.clone()).unwrap());
=======

        if decoded_text.contains(text_to_replace) {
            let new_text = decoded_text.replace(text_to_replace, replacement);
            let encoded_bytes = Document::encode_text(encoding, &new_text);
>>>>>>> 202afb8 (WIP)
            *bytes = encoded_bytes;
        }
    }
    Ok(())
}

pub fn highlight_encoded_text(
    operations: &Vec<Operation>,
    operation_index: usize,
    encoding: &Encoding,
    font: &Fonts,
    text: &str, // target text to highlight
) -> Result<Vec<Vec<Operation>>> {
    let text_operation = &operations[operation_index];
    let mut highlighted_operation: Vec<Operation> = Vec::new();
    let mut redacted_future_operations = Vec::<Vec<Operation>>::new();

    // Extract the encoded bytes and spacing information from the original operation.
    let (target_bytes, mut spacing_loc): (Vec<u8>, Option<Vec<(usize, f32)>>) = match &text_operation.operands[0] {
        Object::String(ref bytes, _) => (bytes.clone(), None),
        Object::Array(ref array) => {
            let mut encoded_bytes = Vec::new();
            let mut spacing_info = Vec::new();

            for obj in array.iter() {
                match obj {
                    Object::String(ref bytes, _) => {
                        encoded_bytes.extend_from_slice(bytes);
                    }
                    Object::Real(space) => {
                        spacing_info.push((encoded_bytes.len(), *space));
                    }
                    Object::Integer(space) => {
                        spacing_info.push((encoded_bytes.len(), *space as f32));
                    }
                    _ => {}
                }
            }

            let spacing_option = if spacing_info.is_empty() {
                None
            } else {
                Some(spacing_info)
            };

            (encoded_bytes, spacing_option)
        }
        _ => panic!("Wrong operation type"),
    };

    // Helper function to create a new text showing operation.
    // Currently always returns a "Tj" operator.
    fn make_new_tj_operation(string: Vec<u8>, spacing: &mut Option<Vec<(usize, f32)>>) -> Operation {
        Operation::new("Tj", vec![Object::string_literal(string)])
        // If needed, TJ operator can be rebuilt using the provided spacing info.
    }

    // Decode the accumulated bytes to a String.
    let target_string = Document::decode_text(encoding, &target_bytes)?;
    debug!("text_operation: {:?}", text_operation.operands);
    debug!("spacing_loc: {:?}", spacing_loc);
    debug!("target_string: {:?} (len: {})", target_string, target_string.len());

    // Get text style (position, font size, etc.) from operations.
    let text_operations = TextOperation::new(operations);
    let text_style = text_operations.get_style_at(operation_index)?;

    // Calculate the width of the target text (to be highlighted).
    let highlight_text_width = font.get_str_glyph_width(text, text_style.font_size);

    let mut box_x_offset = text_style.pos.0;
    let mut text_rel_x_offset = 0.0;

    // Split the target string by the target text.
    // NOTE: split() removes the delimiter, so we need to reinsert it as highlighted text.
    let mut segments: Vec<&str> = target_string.split(text).collect();
    debug!("segments: {:?}", segments);

    // Process the left-most segment (before the first occurrence).
    if let Some(left) = segments.first() {
        let left_encoded_bytes = Document::encode_text(encoding, left);
        let tj_operation = make_new_tj_operation(left_encoded_bytes, &mut spacing_loc);
        highlighted_operation.push(tj_operation);
        let left_text_width = font.get_str_glyph_width(left, text_style.font_size);
        box_x_offset += left_text_width;
        text_rel_x_offset = left_text_width;
        segments.remove(0);
    }

    // For each occurrence of the target text (highlight it) and then output the following visible segment.
    for segment in segments {
        // --- Highlight the target text (the one to be highlighted) ---
        // Calculate highlight box dimensions.
        let ascent = font.get_ascent(text_style.font_size).unwrap_or(0.0);
        let descent = font.get_descent(text_style.font_size).unwrap_or(0.0);
        let box_start_x = box_x_offset;
        let box_width = highlight_text_width;
        // For background rectangle, we use full height (from descent to ascent).
        let box_start_y = text_style.pos.1 + descent;
        let box_height = ascent - descent;

        // Draw blue highlight rectangle behind the text.
        highlighted_operation.push(Operation::new("q", vec![])); // Save graphics state
        highlighted_operation.push(Operation::new(
            "rg",
            vec![
                Object::Real(1.0), // Blue fill: RGB(0,0,1)
                Object::Real(1.0),
                Object::Real(0.0),
            ],
        ));
        highlighted_operation.push(Operation::new(
            "re",
            vec![
                Object::Real(box_start_x),
                Object::Real(box_start_y),
                Object::Real(box_width),
                Object::Real(box_height),
            ],
        ));
        highlighted_operation.push(Operation::new("f", vec![])); // Fill rectangle
        highlighted_operation.push(Operation::new("Q", vec![])); // Restore graphics state

        // Draw blue underline below the highlighted text.
        // We'll draw a thin line (e.g. 1 unit thickness) a few units below the baseline.
        let underline_y = text_style.pos.1 + descent - 0.3; // 2 units below baseline
        highlighted_operation.push(Operation::new("q", vec![]));
        highlighted_operation.push(Operation::new(
            "rg",
            vec![
                Object::Real(1.0), // Blue stroke: RGB(0,0,1)
                Object::Real(1.0),
                Object::Real(0.0),
            ],
        ));
        highlighted_operation.push(Operation::new("w", vec![Object::Real(1.0)])); // Set line width
                                                                                  // Move to start of underline.
        highlighted_operation.push(Operation::new(
            "m",
            vec![Object::Real(box_start_x), Object::Real(underline_y)],
        ));
        // Draw line to end of highlighted text.
        highlighted_operation.push(Operation::new(
            "l",
            vec![Object::Real(box_start_x + box_width), Object::Real(underline_y)],
        ));
        highlighted_operation.push(Operation::new("S", vec![])); // Stroke the line
        highlighted_operation.push(Operation::new("Q", vec![]));

        // Output the target text (highlighted text) so it remains visible.
        let highlighted_text_bytes = Document::encode_text(encoding, text);
        let tj_operation = make_new_tj_operation(highlighted_text_bytes, &mut spacing_loc);
        highlighted_operation.push(tj_operation);

        // --- Process the visible text segment after the highlighted text ---
        // Move the text position by the width of the highlighted text.
        let new_x_offset = text_rel_x_offset + highlight_text_width;
        highlighted_operation.push(Operation::new(
            "Td",
            vec![Object::Real(new_x_offset), Object::Integer(0)],
        ));
        // Output the following visible text segment.
        let segment_encoded_bytes = Document::encode_text(encoding, segment);
        let tj_operation_segment = make_new_tj_operation(segment_encoded_bytes, &mut spacing_loc);
        highlighted_operation.push(tj_operation_segment);

        let segment_text_width = font.get_str_glyph_width(segment, text_style.font_size);
        // Update offsets.
        box_x_offset += highlight_text_width + segment_text_width;
        text_rel_x_offset = segment_text_width;
    }

    if highlighted_operation.is_empty() {
        return Ok(vec![vec![text_operation.clone()]]);
    }

    Ok(redacted_future_operations)
}

fn has_partial_suffix(segment: &str, target: &str) -> bool {
    // Iterate over each possible suffix of `segment`
    segment.char_indices().any(|(i, _)| {
        let suffix = &segment[i..];
        // Skip if the suffix exactly matches target (unlikely after splitting)
        suffix != target && target.contains(suffix)
    })
}

fn is_possibily_parts_of_string(string: &str, target: &str) -> bool {
    // If the string exactly equals the target, then it's already complete.
    if string == target {
        return false;
    }

    // Split the string by occurrences of the complete target,
    // effectively removing the fully matched parts.
    string.split(target).any(|segment| has_partial_suffix(segment, target))
}

fn split_text<'a>(text: &'a str, removal_ranges: &[(usize, usize)]) -> Vec<(&'a str, bool)> {
    debug!("Text: {:?}", text);
    debug!("Removal ranges: {:?}", removal_ranges);
    let mut segments = Vec::<(&str, bool)>::new();
    let text_len = text.len();
    let mut start = 0;

    for &(rem_start, rem_end) in removal_ranges {
        debug!("Removal range: {}..{}", rem_start, rem_end);
        let head_text = &text[start..rem_start];
        if !head_text.is_empty() {
            segments.push((head_text, false));
            segments.push((&text[rem_start..rem_end], true));
        } else {
            // Removal range starts at the beginning of the text
            segments.push((&text[rem_start..rem_end], true));
        }
        start = rem_end;
        // Ensure removal start is within text length
        // let safe_rem_start = if rem_start < text_len { rem_start } else { text_len };
        // Clamp removal end within text; note: if text is empty safe_rem_end becomes 0
        // let safe_rem_end = if rem_end < text_len {
        //     rem_end
        // } else {
        //     text_len.saturating_sub(1)
        // };

        // Only slice if there is a gap prior to the removal interval
        // if start < safe_rem_start {
        //     segments.push(&text[start..safe_rem_start]);
        // }
        // Update start position, ensuring we don't overflow text length
        // start = safe_rem_end.saturating_add(1);
        if start >= text_len {
            break;
        }
    }
    if start < text_len {
        segments.push((&text[start..], false));
    }
    debug!("Segments: {:?}", segments);
    // Add last segment if any remains
    // if start < text_len {
    //     segments.push(&text[start..]);
    // }

    segments
}

pub fn redact_encoded_text(
    operations: &Vec<Operation>, operation_index: usize, encoding: &Encoding, font: &Fonts, text: &str,
) -> Result<Vec<Vec<Operation>>> {
    let text_operation = &operations[operation_index];
    let mut redacted_future_operations = Vec::<Vec<Operation>>::new();

    let (target_bytes, mut spacing_loc): (Vec<u8>, Option<Vec<(usize, f32)>>) = match &text_operation.operands[0] {
        Object::String(ref bytes, _) => (bytes.clone(), None),
        Object::Array(ref array) => {
            let mut encoded_bytes = Vec::new();
            let mut spacing_info = Vec::new();

            for obj in array.iter() {
                match obj {
                    Object::String(ref bytes, _) => {
                        encoded_bytes.extend_from_slice(bytes);
                    }
                    Object::Real(space) => {
                        spacing_info.push((encoded_bytes.len(), *space));
                    }
                    Object::Integer(space) => {
                        spacing_info.push((encoded_bytes.len(), *space as f32));
                    }
                    _ => {}
                }
            }

            let spacing_option = if spacing_info.is_empty() {
                None
            } else {
                Some(spacing_info)
            };

            (encoded_bytes, spacing_option)
        }
        _ => panic!("Wrong operation type"),
    };

    fn make_new_tj_operation(string: Vec<u8>, spacing: &mut Option<Vec<(usize, f32)>>) -> Operation {
        Operation::new("Tj", vec![Object::string_literal(string)])
        // TODO: Create TJ operation with spacing
        // match spacing {
        //     Some(spacing_vec) => {
        //         let mut operands = Vec::new();
        //         let mut start = 0;
        //         for &(index, space) in spacing_vec.iter() {
        //             if index > string.len() {
        //                 break;
        //             }
        //             let pre_str = &string[start..index];
        //             operands.push(Object::string_literal(pre_str.to_vec()));
        //             operands.push(Object::Real(space));
        //             start = index;
        //         }
        //         if start < string.len() {
        //             operands.push(Object::string_literal(string[start..].to_vec()));
        //         } else {
        //             operands.push(Object::string_literal(Vec::new()));
        //         }
        //         Operation::new("TJ", operands)
        //     }
        //     None => Operation::new("Tj", vec![Object::string_literal(string)]),
        // }
    }

    let target_string = Document::decode_text(encoding, &target_bytes)?;

    debug!("text_operation: {:?}", text_operation.operands);
    debug!("spacing_loc: {:?}", spacing_loc);
    debug!("target_strings: {:?} {:?}", target_string, target_string.len());
    debug!(
        "is part of string: {:?}",
        is_possibily_parts_of_string(&target_string, &text)
    );

    // Get text style (position, font size, etc.) from operations
    let text_operations = TextOperation::new(operations);

    let mut get_next_string = is_possibily_parts_of_string(&target_string, &text);
    let mut current_operation_index = operation_index;
    // Vec < operation index, (string target range) >
    // let mut future_redact_query = Vec::<(usize, (usize, usize))>::new();
    let mut full_possibily_same_ctx_string = target_string.to_owned();
    // Each text start index in the full_possibily_same_ctx_string
    let mut possibily_same_ctx_chunk_index = Vec::<usize>::new();
    // Vec < operation index, Vec < text range to redact > >
    let mut redact_targets = Vec::<(usize, Vec<(usize, usize)>)>::new();
    redact_targets.push((current_operation_index, Vec::new()));

    while get_next_string {
        let _text_operations = TextOperation::new(operations);
        let next_style = _text_operations.get_next_style_after(current_operation_index);
        if let Some(next_style) = next_style {
            let (next_id, _next_style) = next_style;
            current_operation_index = next_id;
            let decoded_text = _next_style.get_text(&encoding)?;
            possibily_same_ctx_chunk_index.push(full_possibily_same_ctx_string.len());
            full_possibily_same_ctx_string += &decoded_text;
            redact_targets.push((next_id, Vec::new()));
            // debug!("operations length: {:?}", operations.len());
            get_next_string = is_possibily_parts_of_string(&decoded_text, &text);
            // future_redact_query.append((
        } else {
            get_next_string = false
        }
    }
    possibily_same_ctx_chunk_index.push(full_possibily_same_ctx_string.len());

    if full_possibily_same_ctx_string != target_string {
        if full_possibily_same_ctx_string.contains(&text) {
            debug!("Fully concated string: {:?}", full_possibily_same_ctx_string);
            debug!("Possibily same ctx chunk index: {:?}", possibily_same_ctx_chunk_index);
            // Find the text in the full_possibily_same_ctx_string and get it's range
            let mut positions = Vec::<(usize, usize)>::new();
            let mut start_pos = 0;
            while let Some(pos) = full_possibily_same_ctx_string[start_pos..].find(text) {
                let real_pos = start_pos + pos;
                positions.push((real_pos, real_pos + text.len()));
                start_pos = real_pos + text.len(); // Move past the current occurrence
            }

            debug!("Positions: {:?}", positions);

            // Make redact_targets
            let mut start = 0;
            for (chunk_id, end) in possibily_same_ctx_chunk_index.iter().enumerate() {
                let chunk = &full_possibily_same_ctx_string[start..*end];
                debug!("Chunk: {:?}", chunk);

                let mut redact_ranges = Vec::<(usize, usize)>::new();
                for (start_pos, end_pos) in positions.iter() {
                    if (start_pos >= &start) && (end_pos <= end) {
                        // If chunk contain target text entirely
                        redact_ranges.push((start_pos - start, end_pos - start));
                    } else if (start_pos < &start) && (end_pos > &start) {
                        let redact_start = 0;
                        let redact_end = if end_pos < end { end_pos - start } else { end - start };
                        redact_ranges.push((redact_start, redact_end));
                    } else if (end_pos > end) && (start_pos < end) {
                        let redact_start = start_pos - start;
                        let redact_end = end - start;
                        redact_ranges.push((redact_start, redact_end));
                    } else if start_pos < &start && end_pos > end {
                        redact_ranges.push((0, end - start));
                    }
                }

                // Attempt to redact the text
                for (start_pos, end_pos) in redact_ranges.iter() {
                    let redact_target = &chunk[*start_pos..*end_pos];
                    debug!("Redact target: {:?}", redact_target);
                }

                redact_targets[chunk_id].1 = redact_ranges;

                start = *end;
            }
            debug!("Redact targets: {:?}", redact_targets);
        }
    } else {
        debug!("No need to concat string, {:?}", full_possibily_same_ctx_string);
        let mut redact_ranges = Vec::<(usize, usize)>::new();

        let mut start = 0;
        while let Some(pos) = target_string[start..].find(text) {
            let real_pos = start + pos;
            redact_ranges.push((real_pos, real_pos + text.len()));
            start = real_pos + text.len(); // Move past the current occurrence
        }

        redact_targets.push((current_operation_index, redact_ranges));
    }

    debug!("Redact targets: {:?}", redact_targets);

    // let mut remaining_segments: Vec<&str> = target_string.split(text).collect();
    // debug!("remaining_segments: {:?}", remaining_segments);

    for (operation_id, redact_ranges) in redact_targets.iter() {
        if redact_ranges.is_empty() {
            continue;
        }
        let mut redacted_operation = Vec::<Operation>::new();
        let chunk_style = text_operations.get_style_at(*operation_id)?;
        let chunk_text = chunk_style.get_text(&encoding)?;
        let remaining_segments = split_text(&chunk_text, redact_ranges);
        // let mut remaining_segments: Vec<&str> = chunk_text.split(text).collect();
        debug!("remaining_segments: {:?}", remaining_segments);

        let mut box_x_offset = chunk_style.pos.0;

        for (segment, is_redacted) in remaining_segments {
            // Encode left text portion
            if is_redacted {
                debug!("Redacted segment: {:?}", segment);
                let redacted_text_length = font.get_str_glyph_width(segment, chunk_style.font_size);

                let box_start_x = box_x_offset;
                let box_width = redacted_text_length;
                let ascent = font.get_ascent(chunk_style.font_size).unwrap_or(0.0);
                let descent = font.get_descent(chunk_style.font_size).unwrap_or(0.0);
                let box_start_y = chunk_style.pos.1 + descent;
                let box_height = ascent - descent;

                box_x_offset += redacted_text_length;

                // Draw redaction box covering the redacted text area
                redacted_operation.push(Operation::new("q", vec![])); // Save graphics state
                redacted_operation.push(Operation::new(
                    "rg",
                    vec![
                        Object::Real(0.0), // Red color
                        Object::Real(0.0),
                        Object::Real(0.0),
                    ],
                ));
                redacted_operation.push(Operation::new(
                    "re",
                    vec![
                        Object::Real(box_start_x),
                        Object::Real(box_start_y),
                        Object::Real(box_width),
                        Object::Real(box_height),
                    ],
                ));
                redacted_operation.push(Operation::new("f", vec![])); // Fill the rectangle
                redacted_operation.push(Operation::new("Q", vec![])); // Restore graphics state

                redacted_operation.push(Operation::new(
                    "Td",
                    vec![Object::Real(redacted_text_length), Object::Integer(0)],
                ));
            } else {
                let text_length = font.get_str_glyph_width(segment, chunk_style.font_size);
                let right_encoded_bytes = Document::encode_text(encoding, segment);

                // redacted_operation.push(Operation::new("Tj", vec![Object::string_literal(right_encoded_bytes)]));
                let tj_operation = make_new_tj_operation(right_encoded_bytes, &mut spacing_loc);
                redacted_operation.push(tj_operation);

                box_x_offset += text_length;

                redacted_operation.push(Operation::new(
                    "Td",
                    vec![Object::Real(text_length), Object::Integer(0)],
                ));
            }
        }

        redacted_future_operations.push(redacted_operation);
    }

    if redacted_future_operations.is_empty() {
        return Ok(vec![vec![text_operation.clone()]]);
    }

    Ok(redacted_future_operations)
}

/// Decode CrossReferenceStream
pub fn decode_xref_stream(mut stream: Stream, length: usize) -> Result<(Xref, Dictionary)> {
    if stream.is_compressed() {
        stream.decompress()?;
    }
    let mut dict = stream.dict;
    let mut reader = Cursor::new(stream.content);
    let size = dict
        .get(b"Size")
        .and_then(Object::as_i64)
        .map_err(|_| ParseError::InvalidXref)?;
    let mut xref = Xref::new(size as u32, XrefType::CrossReferenceStream);
    xref.bytes_len = Some(length);
    {
        let section_indice = dict
            .get(b"Index")
            .and_then(parse_integer_array)
            .unwrap_or_else(|_| vec![0, size]);
        let field_widths = dict
            .get(b"W")
            .and_then(parse_integer_array)
            .map_err(|_| ParseError::InvalidXref)?;

        if field_widths.len() < 3
            || field_widths[0].is_negative()
            || field_widths[1].is_negative()
            || field_widths[2].is_negative()
        {
            return Err(ParseError::InvalidXref.into());
        }

        let mut bytes1 = vec![0_u8; field_widths[0] as usize];
        let mut bytes2 = vec![0_u8; field_widths[1] as usize];
        let mut bytes3 = vec![0_u8; field_widths[2] as usize];

        for i in 0..section_indice.len() / 2 {
            let start = section_indice[2 * i];
            let count = section_indice[2 * i + 1];

            for j in 0..count {
                let entry_type = if !bytes1.is_empty() {
                    read_big_endian_integer(&mut reader, bytes1.as_mut_slice())?
                } else {
                    1
                };
                match entry_type {
                    0 => {
                        // free object
                        read_big_endian_integer(&mut reader, bytes2.as_mut_slice())?;
                        read_big_endian_integer(&mut reader, bytes3.as_mut_slice())?;
                    }
                    1 => {
                        // normal object
                        let offset = read_big_endian_integer(&mut reader, bytes2.as_mut_slice())?;
                        let generation = if !bytes3.is_empty() {
                            read_big_endian_integer(&mut reader, bytes3.as_mut_slice())?
                        } else {
                            0
                        } as u16;
                        xref.insert((start + j) as u32, XrefEntry::Normal { offset, generation });
                    }
                    2 => {
                        // compressed object
                        let container = read_big_endian_integer(&mut reader, bytes2.as_mut_slice())?;
                        let index = read_big_endian_integer(&mut reader, bytes3.as_mut_slice())? as u16;
                        xref.insert((start + j) as u32, XrefEntry::Compressed { container, index });
                    }
                    _ => {}
                }
            }
        }
    }
    dict.remove(b"Length");
    dict.remove(b"W");
    dict.remove(b"Index");
    Ok((xref, dict))
}

fn read_big_endian_integer(reader: &mut Cursor<Vec<u8>>, buffer: &mut [u8]) -> Result<u32> {
    reader.read_exact(buffer)?;
    let mut value = 0;
    for &mut byte in buffer {
        value = (value << 8) + u32::from(byte);
    }
    Ok(value)
}

fn parse_integer_array(array: &Object) -> Result<Vec<i64>> {
    let array = array.as_array()?;
    let mut out = Vec::with_capacity(array.len());

    for n in array {
        out.push(n.as_i64()?);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creator::tests::{create_document, create_document_with_texts, save_document};

    #[cfg(not(feature = "async"))]
    #[test]
    fn load_and_save() {
        // test load_from() and save_to()
        use std::fs::File;
        use std::io::Cursor;
        // Create temporary folder to store file.
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_1_load_and_save.pdf");

        let mut doc = create_document();

        save_document(&file_path, &mut doc);

        let in_file = File::open(file_path).unwrap();
        let mut in_doc = Document::load_from(in_file).unwrap();

        let out_buf = Vec::new();
        let mut memory_cursor = Cursor::new(out_buf);
        in_doc.save_to(&mut memory_cursor).unwrap();
        // Check if saved file is not an empty bytes vector.
        assert!(!memory_cursor.get_ref().is_empty());
    }

    #[test]
    fn extract_text_chunks() {
        let text1 = "Hello world!";
        let text2 = "Ferris is the best!";
        let doc = create_document_with_texts(&[text1, text2]);
        let extracted_texts = doc.extract_text_chunks(&[1, 2]);
        assert_eq!(extracted_texts.len(), 2);
        assert_eq!(
            [
                extracted_texts[0].as_ref().unwrap().trim(),
                extracted_texts[1].as_ref().unwrap().trim()
            ],
            [text1, text2]
        );
    }

    #[test]
    fn extract_text_concatenates_text_from_multiple_pages() {
        let text1 = "Hello world!";
        let text2 = "Ferris is the best!";
        let doc = create_document_with_texts(&[text1, text2]);
        let extracted_text = doc.extract_text(&[1, 2]);
        assert_eq!(extracted_text.unwrap(), format!("{text1}\n{text2}\n"));
    }
}
