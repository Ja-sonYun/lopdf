use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufWriter, Result, Write};
use std::path::Path;
use std::string::String;
use std::vec;

use log::debug;

use super::Object::*;
use super::{Dictionary, Document, Object, Stream, StringFormat};
use crate::overwrite_document::OverwriteDocument;
use crate::parser::ParserInput;
use crate::{parser, xref::*, Error, IncrementalDocument, ObjectId, Reader};

impl Object {
    fn as_bytes(&self, id: ObjectId) -> Vec<u8> {
        let mut buf = Vec::new();
        Writer::_write_indirect_object(&mut buf, id.0, id.1, self).unwrap();
        buf
    }
}

impl Document {
    /// Save PDF document to specified file path.
    #[inline]
    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<File> {
        let mut file = BufWriter::new(File::create(path)?);
        self.save_internal(&mut file)?;
        Ok(file.into_inner()?)
    }

    /// Save PDF to arbitrary target
    #[inline]
    pub fn save_to<W: Write>(&mut self, target: &mut W) -> Result<()> {
        self.save_internal(target)
    }

    fn save_internal<W: Write>(&mut self, target: &mut W) -> Result<()> {
        let mut target = CountingWrite {
            inner: target,
            bytes_written: 0,
        };

        let mut xref = Xref::new(self.max_id + 1, self.reference_table.cross_reference_type);
        writeln!(target, "%PDF-{}", self.version)?;

        Writer::write_binary_mark(&mut target, &self.binary_mark)?;

        for (&(id, generation), object) in &self.objects {
            if object
                .type_name()
                .map(|name| [b"ObjStm".as_slice(), b"XRef".as_slice(), b"Linearized".as_slice()].contains(&name))
                .ok()
                != Some(true)
            {
                Writer::write_indirect_object(&mut target, id, generation, object, &mut xref)?;
            }
        }

        let xref_start = target.bytes_written;

        // Pick right cross reference stream.
        match xref.cross_reference_type {
            XrefType::CrossReferenceTable => {
                Writer::write_xref(&mut target, &xref)?;
                self.write_trailer(&mut target)?;
            }
            XrefType::CrossReferenceStream => {
                // Cross Reference Stream instead of XRef and Trailer
                self.write_cross_reference_stream(&mut target, &mut xref, xref_start as u32)?;
            }
        }
        // Write `startxref` part of trailer
        write!(target, "\nstartxref\n{}\n%%EOF", xref_start)?;

        Ok(())
    }

    /// Write the Cross Reference Stream.
    ///
    /// Insert an `Object` to the end of the PDF (not visible when inspecting `Document`).
    /// Note: This is different from the "Cross Reference Table".
    fn write_cross_reference_stream<W: Write>(
        &mut self, file: &mut CountingWrite<&mut W>, xref: &mut Xref, xref_start: u32,
    ) -> Result<()> {
        // Increment max_id to account for CRS.
        self.max_id += 1;
        let new_obj_id_for_crs = self.max_id;
        xref.insert(
            new_obj_id_for_crs,
            XrefEntry::Normal {
                offset: xref_start,
                generation: 0,
            },
        );
        self.trailer.set("Type", Name(b"XRef".to_vec()));
        // Update `max_id` in trailer
        self.trailer.set("Size", i64::from(self.max_id + 1));
        // Set the size of each entry in bytes (default for PDFs is `[1 2 1]`)
        // In our case we use `[u8, u32, u16]` for each entry
        // to keep things simple and working at all times.
        self.trailer.set("W", Array(vec![Integer(1), Integer(4), Integer(2)]));
        // Note that `ASCIIHexDecode` does not work correctly,
        // but is still useful for debugging sometimes.
        let filter = XRefStreamFilter::None;
        let (stream, stream_length, indexes) = Writer::create_xref_stream(xref, filter)?;
        self.trailer.set("Index", indexes);

        if filter == XRefStreamFilter::ASCIIHexDecode {
            self.trailer.set("Filter", Name(b"ASCIIHexDecode".to_vec()));
        } else {
            self.trailer.remove(b"Filter");
        }

        self.trailer.set("Length", stream_length as i64);

        let trailer = &self.trailer;
        let cross_reference_stream = Stream(Stream {
            dict: trailer.clone(),
            allows_compression: true,
            content: stream,
            start_position: None,
        });
        // Insert Cross Reference Stream as an `Object` to the end of the PDF.
        // The `Object` is not added to `Document` because it is generated every time you save.
        Writer::write_indirect_object(file, new_obj_id_for_crs, 0, &cross_reference_stream, xref)?;

        Ok(())
    }

    fn write_trailer(&mut self, file: &mut dyn Write) -> Result<()> {
        self.trailer.set("Size", i64::from(self.max_id + 1));
        file.write_all(b"trailer\n")?;
        Writer::write_dictionary(file, &self.trailer)?;
        Ok(())
    }
}

impl IncrementalDocument {
    /// Save PDF document to specified file path.
    #[inline]
    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<File> {
        let mut file = BufWriter::new(File::create(path)?);
        self.save_internal(&mut file)?;
        Ok(file.into_inner()?)
    }

    /// Save PDF to arbitrary target
    #[inline]
    pub fn save_to<W: Write>(&mut self, target: &mut W) -> Result<()> {
        self.save_internal(target)
    }

    fn save_internal<W: Write>(&mut self, target: &mut W) -> Result<()> {
        let mut target = CountingWrite {
            inner: target,
            bytes_written: 0,
        };

        // Write previous document versions.
        let prev_document_bytes = self.get_prev_documents_bytes();
        target.inner.write_all(prev_document_bytes)?;
        target.bytes_written += prev_document_bytes.len();

        // Write/Append new document version.
        let mut xref = Xref::new(
            self.new_document.max_id + 1,
            self.get_prev_documents().reference_table.cross_reference_type,
        );

        if let Some(last_byte) = prev_document_bytes.last() {
            if *last_byte != b'\n' {
                // Add a newline if it was not already present
                writeln!(target)?;
            }
        }
        writeln!(target, "%PDF-{}", self.new_document.version)?;

        Writer::write_binary_mark(&mut target, &self.new_document.binary_mark)?;

        for (&(id, generation), object) in &self.new_document.objects {
            if object
                .type_name()
                .map(|name| [b"ObjStm".as_slice(), b"XRef".as_slice(), b"Linearized".as_slice()].contains(&name))
                .ok()
                != Some(true)
            {
                Writer::write_indirect_object(&mut target, id, generation, object, &mut xref)?;
            }
        }

        let xref_start = target.bytes_written;

        // Pick right cross reference stream.
        match xref.cross_reference_type {
            XrefType::CrossReferenceTable => {
                Writer::write_xref(&mut target, &xref)?;
                self.new_document.write_trailer(&mut target)?;
            }
            XrefType::CrossReferenceStream => {
                // Cross Reference Stream instead of XRef and Trailer
                self.new_document
                    .write_cross_reference_stream(&mut target, &mut xref, xref_start as u32)?;
            }
        }
        // Write `startxref` part of trailer
        write!(target, "\nstartxref\n{}\n%%EOF", xref_start)?;

        Ok(())
    }
}

impl OverwriteDocument {
    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<File> {
        let mut file = BufWriter::new(File::create(path)?);
        file.write_all(&self.bytes_documents)?;
        Ok(file.into_inner()?)
    }

    pub fn attempt_decrypt_if_encrypted(&mut self, password: Option<&str>) -> Result<()> {
        let password = password.unwrap_or("");
        if self.document.is_encrypted() {
            self.document
                .decrypt(password)
                .map_err(|_err| io::Error::new(io::ErrorKind::InvalidInput, "Failed to decrypt"))?;
        }
        Ok(())
    }

    pub fn replace_text(&mut self, text: &str, replacement: &str) -> Result<()> {
        let mut should_be_updated = Vec::<(ObjectId, Object)>::new();

        for page_id in self.document.page_iter() {
            println!("Page id: {:?}", page_id);
            let encodings = self.document.get_encodings(page_id).unwrap();
            let object_ids = self.document.get_page_contents(page_id);
            let objects = object_ids
                .iter()
                .map(|object_id| {
                    let mut object = self.document.get_object(*object_id).unwrap().clone();
                    if self.document.is_encrypted() {
                        let password = self.password.as_ref().unwrap();
                        self.document
                            .decrypt_object(*object_id, &mut object, &password)
                            .unwrap();
                    }
                    (object_id, object)
                })
                .collect::<Vec<_>>();

            println!("objects: {:?}", objects.len());

            for (object_id, object) in objects {
                println!("object_id: {:?}", object_id);
                let mut refined_object = Object::from(
                    object
                        .as_stream()
                        .unwrap()
                        .replace_text(&encodings, text, replacement)
                        .unwrap(),
                );
                if self.document.is_encrypted() {
                    let password = self.password.as_ref().unwrap();
                    self.document
                        .encrypt_object(*object_id, &mut refined_object, &password)
                        .unwrap();
                }
                should_be_updated.push((object_id.clone(), refined_object));
            }
        }

        // let target_obj = should_be_updated[1].1.clone();
        // // let target_id = should_be_updated[1].0.clone();

        // debug!("Should be updated: {:?}", should_be_updated[1].0);
        // self.update_object(should_be_updated[1].0, &target_obj).unwrap();
        for (id, object) in should_be_updated {
            self.update_object(id, &object).unwrap();
        }

        Ok(())
    }

    fn verify_buffer(&mut self, buffer: Vec<u8>) -> Result<()> {
        self.document = Reader {
            buffer: &buffer,
            document: Document::new(),
        }
        .read(None)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        self.bytes_documents = buffer;
        Ok(())
    }

    fn get_object_offset(&self, id: ObjectId) -> Result<u32> {
        let entry = self
            .document
            .reference_table
            .get(id.0)
            .ok_or(Error::MissingXrefEntry)
            .unwrap();
        match *entry {
            XrefEntry::Normal { offset, generation } if generation == id.1 => Ok(offset),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Object is not a normal object",
            )),
        }
    }

    fn inject_at_position(buffer: &Vec<u8>, data: Vec<u8>, position: usize) -> Vec<u8> {
        if position > buffer.len() {
            panic!("Invalid position specified!");
        }

        // Create a new buffer with injected data
        let mut new_buffer = Vec::<u8>::new();
        new_buffer.extend_from_slice(&buffer[..position]); // Keep data before injection
        new_buffer.extend(data.clone()); // Inject new data
        new_buffer.extend_from_slice(&buffer[position..]); // Append remaining buffer
        new_buffer
    }

    fn replace_range_in_buffer(buffer: &Vec<u8>, data: Vec<u8>, range: (usize, usize)) -> Vec<u8> {
        let (start_offset, end_offset) = range;

        // Validate the range
        if start_offset > buffer.len() || end_offset > buffer.len() || start_offset > end_offset {
            panic!("Invalid range specified!");
        }

        // Create a new buffer
        let mut new_buffer = Vec::<u8>::new();
        new_buffer.extend_from_slice(&buffer[..start_offset]);
        new_buffer.extend(data.clone());
        new_buffer.extend_from_slice(&buffer[end_offset..]);

        new_buffer
    }

    fn update_xref<F: FnMut(&mut Xref, fn(&mut Xref, usize, isize)) -> i64>(
        buffer: &Vec<u8>, updater: &mut F,
    ) -> Vec<u8> {
        let reader = Reader {
            buffer: &buffer,
            document: Document::new(), // We don't need the document here
        };
        let (xref_start, xref_end) = Reader::get_xref_range(buffer).unwrap();
        let (mut xref, mut trailer) =
            parser::xref_and_trailer(ParserInput::new_extra(&buffer[xref_start..], "xref"), &reader).unwrap();

        // Update the Xref
        let shift_length = updater(&mut xref, |xref: &mut Xref, updated_obj_offset, shift_length| {
            for (_, entry) in xref.entries.iter_mut() {
                if let XrefEntry::Normal { offset, generation: _ } = entry {
                    // if offset is greater than the new object offset,
                    // increase the offset by the difference in length
                    if *offset > updated_obj_offset as u32 {
                        if shift_length > 0 {
                            *offset += shift_length as u32;
                        } else {
                            *offset -= (shift_length * -1) as u32;
                        }
                    }
                }
            }
        });

        let new_xref_start = if shift_length > 0 {
            xref_start + shift_length as usize
        } else {
            xref_start - (shift_length * -1) as usize
        };

        // Prepare new xref stream
        let find_xref_stream = reader.read_object(xref_start, None, &mut HashSet::new());
        let ((xref_id, xref_gen), xref_obj) = match find_xref_stream {
            Ok(((xref_id, xref_gen), mut xref_obj, _)) => {
                // Expect a Stream xref object
                let (xref_stream, _, _) = Writer::create_xref_stream(&xref, XRefStreamFilter::None).unwrap();
                let xref_stream_obj = xref_obj.as_stream_mut().unwrap();
                if xref_stream_obj.dict.has(b"Filter") {
                    xref_stream_obj.decompress().unwrap();
                }
                xref_stream_obj.set_content(xref_stream);
                xref_stream_obj.compress().unwrap();
                let xref_obj = Object::Stream(xref_stream_obj.clone());

                ((xref_id, xref_gen), xref_obj)
            }
            Err(_) => {
                let last_xref_id = xref.entries.keys().max().unwrap();
                let (new_xref_id, new_xref_gen) = (last_xref_id + 1, 0);
                xref.entries.insert(
                    new_xref_id,
                    XrefEntry::Normal {
                        offset: new_xref_start as u32,
                        generation: new_xref_gen,
                    },
                );

                // Update trailer as xref stream object
                trailer.set("Type", Name(b"XRef".to_vec()));
                trailer.set("Size", i64::from(new_xref_id + 1));
                // See `Document::write_cross_reference_stream`
                trailer.set("W", Array(vec![Integer(1), Integer(4), Integer(2)]));

                let (xref_stream, stream_length, indexes) =
                    Writer::create_xref_stream(&xref, XRefStreamFilter::None).unwrap();

                trailer.set("Index", indexes);
                trailer.set("Length", stream_length as i64);

                let cross_reference_stream = Stream(Stream {
                    dict: trailer.clone(),
                    allows_compression: true,
                    content: xref_stream,
                    start_position: None,
                });

                ((new_xref_id, new_xref_gen), cross_reference_stream)
            }
        };

        let mut xref_bytes = Vec::<u8>::new();
        Writer::_write_indirect_object(&mut xref_bytes, xref_id, xref_gen, &xref_obj).unwrap();

        let mut xref_updated_buffer =
            Self::replace_range_in_buffer(buffer, xref_bytes, (xref_start as usize, xref_end as usize));
        Writer::update_xref_offset(&mut xref_updated_buffer, new_xref_start as i64).unwrap();

        xref_updated_buffer
    }

    pub fn append_object(&mut self, object: &Object) -> Result<ObjectId> {
        let reader = Reader {
            buffer: &self.bytes_documents,
            document: Document::new(), // We don't need the document here
        };
        let id = self.document.new_object_id();
        let object_bytes = object.as_bytes(id);
        let increased_length = object_bytes.len() as i64;
        let mut new_object_offset = 0;

        let mut new_document_bytes =
            Self::update_xref(
                &self.bytes_documents,
                &mut |xref: &mut Xref, shift: fn(&mut Xref, usize, isize)| {
                    let mut all_object_offsets = xref
                        .entries
                        .iter()
                        .filter_map(|(_, entry)| match entry {
                            XrefEntry::Normal { offset, generation: _ } => Some(*offset),
                            _ => None,
                        })
                        .collect::<Vec<u32>>();
                    all_object_offsets.sort();

                    let last_object_before_xref = all_object_offsets[all_object_offsets.len() - 2];
                    let (_, _, last_object_length) = reader
                        .read_object(last_object_before_xref as usize, None, &mut HashSet::new())
                        .unwrap();
                    new_object_offset = last_object_before_xref + last_object_length as u32;

                    shift(xref, new_object_offset as usize, increased_length as isize);
                    increased_length
                },
            );
        new_document_bytes = Self::inject_at_position(&new_document_bytes, object_bytes, new_object_offset as usize);

        self.verify_buffer(new_document_bytes)?;
        Ok(id)
    }

    // TODO: This doesn't work for now because of below error
    // ```
    // thread 'main' panicked at examples/extract_text.rs:201:34:
    // called `Result::unwrap()` on an `Err` value: Custom { kind: InvalidData, error: "IO error: failed to fill whole buffer" }
    // stack backtrace:
    //    0: rust_begin_unwind
    //              at /rustc/79e9716c980570bfd1f666e3b16ac583f0168962/library/std/src/panicking.rs:597:5
    //    1: core::panicking::panic_fmt
    //              at /rustc/79e9716c980570bfd1f666e3b16ac583f0168962/library/core/src/panicking.rs:72:14
    //    2: core::result::unwrap_failed
    //              at /rustc/79e9716c980570bfd1f666e3b16ac583f0168962/library/core/src/result.rs:1652:5
    //    3: core::result::Result<T,E>::unwrap
    //              at /rustc/79e9716c980570bfd1f666e3b16ac583f0168962/library/core/src/result.rs:1077:23
    //    4: extract_text::pdf2text
    //              at ./examples/extract_text.rs:201:5
    //    5: extract_text::main
    //              at ./examples/extract_text.rs:226:5
    //    6: core::ops::function::FnOnce::call_once
    //              at /rustc/79e9716c980570bfd1f666e3b16ac583f0168962/library/core/src/ops/function.rs:250:5
    // note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
    // ```
    // pub fn delete_object(&mut self, id: ObjectId) -> Result<()> {
    //     let reader = Reader {
    //         buffer: &self.bytes_documents,
    //         document: Document::new(), // We don't need the document here
    //     };
    //     let object_offset = self.get_object_offset(id).unwrap();
    //     let (_, _, object_length) = reader
    //         .read_object(object_offset as usize, Some(id), &mut HashSet::new())
    //         .unwrap();
    //     let object_end_offset = object_offset + object_length as u32;
    //     let decreased_length = object_length as i64;

    //     let mut new_document_bytes = Self::update_xref(&self.bytes_documents, &mut |xref: &mut Xref| {
    //         xref.entries.remove(&id.0);
    //         for (_, entry) in xref.entries.iter_mut() {
    //             if let XrefEntry::Normal { offset, generation: _ } = entry {
    //                 if *offset > object_offset {
    //                     *offset -= decreased_length as u32;
    //                 }
    //             }
    //         }
    //         -decreased_length
    //     });
    //     new_document_bytes = Self::replace_range_in_buffer(
    //         &new_document_bytes,
    //         Vec::new(),
    //         (object_offset as usize, object_end_offset as usize),
    //     );

    //     self.verify_buffer(new_document_bytes)
    // }

    pub fn update_object(&mut self, id: ObjectId, object: &Object) -> Result<()> {
        let reader = Reader {
            buffer: &self.bytes_documents,
            document: Document::new(), // We don't need the document here
        };

        let object_offset = self.get_object_offset(id).unwrap();
        let (_, _, object_length) = reader
            .read_object(object_offset as usize, Some(id), &mut HashSet::new())
            .unwrap();

        // if there is two \n\n, then make it single \n ( now we're doing it with -1 )
        let object_end_offset = object_offset + object_length as u32 - 1;

        let prev_object_bytes = self.bytes_documents[object_offset as usize..object_end_offset as usize].to_vec();

        let new_object_bytes = object.as_bytes(id);
        let new_object_length = new_object_bytes.len() as u32;
        let increased_length = new_object_length as isize - prev_object_bytes.len() as isize;

        let mut new_document_bytes =
            Self::update_xref(
                &self.bytes_documents,
                &mut |xref: &mut Xref, shift: fn(&mut Xref, usize, isize)| {
                    shift(xref, object_offset as usize, increased_length as isize);
                    increased_length as i64
                },
            );

        new_document_bytes = Self::replace_range_in_buffer(
            &new_document_bytes,
            new_object_bytes,
            (object_offset as usize, object_end_offset as usize),
        );

        debug!("---");
        parser::debug_nearby_bytes_by_cursor(&new_document_bytes, object_offset as usize);
        parser::debug_nearby_bytes_by_cursor(&new_document_bytes, object_offset as usize + new_object_length as usize);

        self.verify_buffer(new_document_bytes)
    }
}

pub struct Writer;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum XRefStreamFilter {
    ASCIIHexDecode,
    _FlateDecode, //this is generally a Zlib compressed Stream.
    None,
}

impl Writer {
    fn need_separator(object: &Object) -> bool {
        matches!(
            *object,
            Null | Boolean(_) | Integer(_) | Real(_) | Reference(_) | Name(_)
        )
    }

    fn need_end_separator(object: &Object) -> bool {
        matches!(
            *object,
            Null | Boolean(_) | Integer(_) | Real(_) | Name(_) | Reference(_)
        )
    }

    /// Write Cross Reference Table.
    ///
    /// Note: This is different from a "Cross Reference Stream".
    fn write_xref(file: &mut dyn Write, xref: &Xref) -> Result<()> {
        writeln!(file, "xref")?;

        let mut xref_section = XrefSection::new(0);
        // Add first (0) entry
        xref_section.add_unusable_free_entry();

        for obj_id in 1..xref.size {
            // If section is empty change number of starting id.
            if xref_section.is_empty() {
                xref_section = XrefSection::new(obj_id);
            }
            if let Some(entry) = xref.get(obj_id) {
                match *entry {
                    XrefEntry::Normal { offset, generation } => {
                        // Add entry
                        xref_section.add_entry(XrefEntry::Normal { offset, generation });
                    }
                    XrefEntry::Compressed { container: _, index: _ } => {
                        xref_section.add_unusable_free_entry();
                    }
                    XrefEntry::Free => {
                        xref_section.add_entry(XrefEntry::Free);
                    }
                    XrefEntry::UnusableFree => {
                        xref_section.add_unusable_free_entry();
                    }
                }
            } else {
                // Skip over `obj_id`, but finish section if not empty.
                if !xref_section.is_empty() {
                    xref_section.write_xref_section(file)?;
                    xref_section = XrefSection::new(obj_id);
                }
            }
        }
        // Print last section
        if !xref_section.is_empty() {
            xref_section.write_xref_section(file)?;
        }
        Ok(())
    }

    fn update_xref_offset(input: &mut Vec<u8>, new_offset: i64) -> Result<()> {
        // Locate "%%EOF" by searching for its byte sequence
        let eof_index = input
            .windows(5) // Length of "%%EOF"
            .rposition(|window| window == b"%%EOF")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing %%EOF"))?;

        // Locate "startxref" before "%%EOF"
        let startxref_index = input[..eof_index]
            .windows(9) // Length of "startxref"
            .rposition(|window| window == b"startxref")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing startxref"))?;

        // Calculate the start and end of the current offset
        let offset_start = startxref_index + 10; // "startxref\n".len()
        let offset_end = input[offset_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| offset_start + i)
            .unwrap_or(eof_index);

        // Ensure indices are valid
        if offset_start >= offset_end || startxref_index >= eof_index {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid PDF structure: startxref and %%EOF are out of order",
            ));
        }

        // Create the new "startxref" line as bytes
        let new_startxref_line = format!("{}", new_offset);
        let new_line_bytes = new_startxref_line.as_bytes();

        // Calculate the space difference
        let space_needed = new_line_bytes.len() as isize - (offset_end - offset_start) as isize;

        if space_needed > 0 {
            // Expand the buffer to fit the new line
            let additional_space = space_needed as usize;
            input.resize(input.len() + additional_space, 0);

            // Adjust the slice to make room for the new data
            let move_start = offset_end;
            let move_end = input.len() - additional_space;
            let move_to = offset_end + additional_space;

            // Ensure valid ranges for copy_within
            if move_to <= input.len() {
                input.copy_within(move_start..move_end, move_to);
            }
        }

        // Write the new offset into the buffer
        input[offset_start..offset_start + new_line_bytes.len()].copy_from_slice(new_line_bytes);

        // If the new line is shorter, fill the remaining space with spaces
        if space_needed < 0 {
            for i in (offset_start + new_line_bytes.len())..offset_end {
                input[i] = b' ';
            }
        }

        Ok(())
    }

    /// Create stream for Cross reference stream.
    fn create_xref_stream(xref: &Xref, filter: XRefStreamFilter) -> Result<(Vec<u8>, usize, Object)> {
        let mut xref_sections = Vec::new();
        let mut xref_section = XrefSection::new(0);

        for obj_id in 1..xref.size + 1 {
            // If section is empty change number of starting id.
            if xref_section.is_empty() {
                xref_section = XrefSection::new(obj_id);
            }
            if let Some(entry) = xref.get(obj_id) {
                xref_section.add_entry(entry.clone());
            } else {
                // Skip over but finish section if not empty
                if !xref_section.is_empty() {
                    xref_sections.push(xref_section);
                    xref_section = XrefSection::new(obj_id);
                }
            }
        }
        // Print last section
        if !xref_section.is_empty() {
            xref_sections.push(xref_section);
        }

        let mut xref_stream = Vec::new();
        let mut xref_index = Vec::new();

        for section in xref_sections {
            // Add indexes to list
            xref_index.push(Integer(section.starting_id as i64));
            xref_index.push(Integer(section.entries.len() as i64));
            // Add entries to stream
            let mut obj_id = section.starting_id;
            for entry in section.entries {
                match entry {
                    XrefEntry::Free => {
                        // Type 0
                        xref_stream.push(0);
                        xref_stream.extend(obj_id.to_be_bytes());
                        xref_stream.extend(vec![0, 0]); // TODO add generation number
                    }
                    XrefEntry::UnusableFree => {
                        // Type 0
                        xref_stream.push(0);
                        xref_stream.extend(obj_id.to_be_bytes());
                        xref_stream.extend(65535_u16.to_be_bytes());
                    }
                    XrefEntry::Normal { offset, generation } => {
                        // Type 1
                        xref_stream.push(1);
                        xref_stream.extend(offset.to_be_bytes());
                        xref_stream.extend(generation.to_be_bytes());
                        // TODO: Check if this is correct
                        // THIS NEED ON pdf 1.7. why????????
                        // xref_stream.extend(vec![0, 0]); // TODO add generation number
                    }
                    XrefEntry::Compressed { container, index } => {
                        // Type 2
                        xref_stream.push(2);
                        xref_stream.extend(container.to_be_bytes());
                        xref_stream.extend(index.to_be_bytes());
                    }
                }
                obj_id += 1;
            }
        }

        // The end of line character should not be counted, added later.
        let stream_length = xref_stream.len();

        if filter == XRefStreamFilter::ASCIIHexDecode {
            xref_stream = xref_stream
                .iter()
                .flat_map(|c| format!("{:02X}", c).as_bytes().to_vec())
                .collect::<Vec<u8>>();
        }

        Ok((xref_stream, stream_length, Array(xref_index)))
    }

    pub fn _write_indirect_object<W: Write>(file: &mut W, id: u32, generation: u16, object: &Object) -> Result<()> {
        write!(
            file,
            "{} {} obj\n{}",
            id,
            generation,
            if Writer::need_separator(object) { " " } else { "" }
        )?;
        Writer::write_object(file, object)?;
        writeln!(
            file,
            "{}\nendobj",
            if Writer::need_end_separator(object) { " " } else { "" }
        )?;
        Ok(())
    }

    fn write_indirect_object<W: Write>(
        file: &mut CountingWrite<&mut W>, id: u32, generation: u16, object: &Object, xref: &mut Xref,
    ) -> Result<()> {
        let offset = file.bytes_written as u32;
        xref.insert(id, XrefEntry::Normal { offset, generation });
        Writer::_write_indirect_object(file, id, generation, object)
    }

    pub fn write_object(file: &mut dyn Write, object: &Object) -> Result<()> {
        match object {
            Null => file.write_all(b"null"),
            Boolean(value) => {
                if *value {
                    file.write_all(b"true")
                } else {
                    file.write_all(b"false")
                }
            }
            Integer(value) => {
                let mut buf = itoa::Buffer::new();
                file.write_all(buf.format(*value).as_bytes())
            }
            Real(value) => write!(file, "{}", value),
            Name(name) => Writer::write_name(file, name),
            String(text, format) => Writer::write_string(file, text, format),
            Array(array) => Writer::write_array(file, array),
            Object::Dictionary(dict) => Writer::write_dictionary(file, dict),
            Object::Stream(stream) => Writer::write_stream(file, stream),
            Reference(id) => write!(file, "{} {} R", id.0, id.1),
        }
    }

    fn write_name(file: &mut dyn Write, name: &[u8]) -> Result<()> {
        file.write_all(b"/")?;
        for &byte in name {
            // white-space and delimiter chars are encoded to # sequences
            // also encode bytes outside of the range 33 (!) to 126 (~)
            if b" \t\n\r\x0C()<>[]{}/%#".contains(&byte) || !(33..=126).contains(&byte) {
                write!(file, "#{:02X}", byte)?;
            } else {
                file.write_all(&[byte])?;
            }
        }
        Ok(())
    }

    fn write_string(file: &mut dyn Write, text: &[u8], format: &StringFormat) -> Result<()> {
        match *format {
            // Within a Literal string, backslash (\) and unbalanced parentheses should be escaped.
            // This rule apply to each individual byte in a string object,
            // whether the string is interpreted as single-byte or multiple-byte character codes.
            // If an end-of-line marker appears within a literal string without a preceding backslash, the result is
            // equivalent to \n. So \r also need be escaped.
            StringFormat::Literal => {
                let mut escape_indice = Vec::new();
                let mut parentheses = Vec::new();
                for (index, &byte) in text.iter().enumerate() {
                    match byte {
                        b'(' => parentheses.push(index),
                        b')' => {
                            if !parentheses.is_empty() {
                                parentheses.pop();
                            } else {
                                escape_indice.push(index);
                            }
                        }
                        b'\\' | b'\r' => escape_indice.push(index),
                        _ => continue,
                    }
                }
                escape_indice.append(&mut parentheses);

                file.write_all(b"(")?;
                if !escape_indice.is_empty() {
                    for (index, &byte) in text.iter().enumerate() {
                        if escape_indice.contains(&index) {
                            file.write_all(b"\\")?;
                            file.write_all(&[if byte == b'\r' { b'r' } else { byte }])?;
                        } else {
                            file.write_all(&[byte])?;
                        }
                    }
                } else {
                    file.write_all(text)?;
                }
                file.write_all(b")")?;
            }
            StringFormat::Hexadecimal => {
                file.write_all(b"<")?;
                for &byte in text {
                    write!(file, "{:02X}", byte)?;
                }
                file.write_all(b">")?;
            }
        }
        Ok(())
    }

    fn write_array(file: &mut dyn Write, array: &[Object]) -> Result<()> {
        file.write_all(b"[")?;
        let mut first = true;
        for object in array {
            if first {
                first = false;
            } else if Writer::need_separator(object) {
                file.write_all(b" ")?;
            }
            Writer::write_object(file, object)?;
        }
        file.write_all(b"]")?;
        Ok(())
    }

    fn write_dictionary(file: &mut dyn Write, dictionary: &Dictionary) -> Result<()> {
        file.write_all(b"<<")?;
        for (key, value) in dictionary {
            Writer::write_name(file, key)?;
            if Writer::need_separator(value) {
                file.write_all(b" ")?;
            }
            Writer::write_object(file, value)?;
            if Writer::need_end_separator(value) {
                file.write_all(b" ")?;
            }
        }
        file.write_all(b">>")?;
        Ok(())
    }

    pub fn write_stream_content(file: &mut dyn Write, content: &[u8]) -> Result<()> {
        file.write_all(b"\nstream\n")?;
        file.write_all(content)?;
        file.write_all(b"\nendstream")?;
        Ok(())
    }

    fn write_stream(file: &mut dyn Write, stream: &Stream) -> Result<()> {
        Writer::write_dictionary(file, &stream.dict)?;
        Writer::write_stream_content(file, &stream.content)?;
        Ok(())
    }

    /// Write Binary mark as follows: %{binary_mark[4]}\n -> %Çì¢ or Hex(%25 c3 87 c3 ac)
    ///
    /// Note: Specified in  ISO 19005-2:2011, ISO 19005-3:2012
    /// headerByte1 > 127 && headerByte2 > 127 && headerByte3 > 127 && headerByte4 > 127
    fn write_binary_mark(file: &mut dyn Write, binary_mark: &[u8]) -> Result<()> {
        if binary_mark.iter().all(|&byte| byte >= 128) {
            file.write_all(b"%")?;
            file.write_all(binary_mark)?;
            file.write_all(b"\n")?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid binary mark",
            ));
        }

        Ok(())
    }
}

pub struct CountingWrite<W: Write> {
    inner: W,
    bytes_written: usize,
}

impl<W: Write> Write for CountingWrite<W> {
    #[inline]
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        let result = self.inner.write(buffer);
        if let Ok(bytes) = result {
            self.bytes_written += bytes;
        }
        result
    }

    #[inline]
    fn write_all(&mut self, buffer: &[u8]) -> Result<()> {
        self.bytes_written += buffer.len();
        // If this returns `Err` we can’t know how many bytes were actually written (if any)
        // but that doesn’t matter since we’re gonna abort the entire PDF generation anyway.
        self.inner.write_all(buffer)
    }

    #[inline]
    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

#[test]
fn save_document() {
    let mut doc = Document::with_version("1.5");
    doc.objects.insert((1, 0), Null);
    doc.objects.insert((2, 0), Boolean(true));
    doc.objects.insert((3, 0), Integer(3));
    doc.objects.insert((4, 0), Real(0.5));
    doc.objects
        .insert((5, 0), String("text((\r)".as_bytes().to_vec(), StringFormat::Literal));
    doc.objects.insert(
        (6, 0),
        String("text((\r)".as_bytes().to_vec(), StringFormat::Hexadecimal),
    );
    doc.objects.insert((7, 0), Name(b"name \t".to_vec()));
    doc.objects.insert((8, 0), Reference((1, 0)));
    doc.objects
        .insert((9, 2), Array(vec![Integer(1), Integer(2), Integer(3)]));
    doc.objects
        .insert((11, 0), Stream(Stream::new(Dictionary::new(), vec![0x41, 0x42, 0x43])));
    let mut dict = Dictionary::new();
    dict.set("A", Null);
    dict.set("B", false);
    dict.set("C", Name(b"name".to_vec()));
    doc.objects.insert((12, 0), Object::Dictionary(dict));
    doc.max_id = 12;

    // Create temporary folder to store file.
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test_0_save.pdf");
    doc.save(&file_path).unwrap();
    // Check if file was created.
    assert!(file_path.exists());
    // Check if path is file
    assert!(file_path.is_file());
    // Check if the file is above 400 bytes (should be about 610 bytes)
    assert!(file_path.metadata().unwrap().len() > 400);
}
