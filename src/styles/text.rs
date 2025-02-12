use log::debug;

use crate::{content::Operation, encodings::Encoding, Document, Error, Object, Result};

#[derive(Debug, Clone)]
pub struct TextStyle {
    pub text: Vec<u8>,
    /// X, Y
    pub pos: (f32, f32),
    pub font: String,
    pub font_size: f32,
    pub leading: f32,
    pub color: (u8, u8, u8),
}

impl TextStyle {
    pub fn new(text: Vec<u8>) -> TextStyle {
        TextStyle {
            text,
            pos: (0., 0.),
            font: String::new(),
            font_size: 0.,
            leading: 0.,
            color: (0, 0, 0),
        }
    }

    pub fn get_text(&self, encoding: &Encoding) -> Result<String> {
        let decoded_text = Document::decode_text(encoding, &self.text)?;
        Ok(decoded_text.as_str().to_string())
    }
}

#[derive(Debug, Clone)]
pub struct TextOperation<'a> {
    // original_operators_offset: usize,
    operators: &'a Vec<Operation>,
}

impl<'a> TextOperation<'a> {
    pub fn new(operators: &'a Vec<Operation>) -> TextOperation<'a> {
        // Self::validate_operators(operators)?;

        Self {
            // original_operators_offset: 0,
            operators,
        }
    }

    pub fn get_next_style_after(&self, start_index: usize) -> Option<(usize, TextStyle)> {
        let mut found_et_once = false;

        for (id, op) in self.operators.iter().enumerate().skip(start_index) {
            match op.operator.as_str() {
                "ET" => {
                    if found_et_once {
                        break;
                    }
                    found_et_once = true;
                }
                "Tj" | "TJ" => {
                    // Skip ET connected with start_index
                    if !found_et_once {
                        continue;
                    }
                    let operation = self.get_style_at(id);
                    if operation.is_ok() {
                        return Some((id, operation.unwrap()));
                    }
                    return None;
                }
                _ => {}
            }
        }
        None
    }

    //     pub fn get_texts(&self) -> Result<Vec<TextStyle>> {
    //         let mut texts: Vec<TextStyle> = Vec::new();
    //         for (index, op) in self.operators.iter().enumerate() {
    //             if Self::is_text_operation(op) {
    //                 texts.push(self.get_style_at(index)?);
    //             }
    //         }
    //         Ok(texts)
    //     }

    //     pub fn get_first_text_style(&self) -> Result<TextStyle> {
    //         for (index, op) in self.operators.iter().enumerate() {
    //             if Self::is_text_operation(op) {
    //                 return self.get_style_at(index);
    //             }
    //         }
    //         Err(Error::ObjectType {
    //             expected: "Name",
    //             found: "Wrong operator",
    //         })
    //     }

    pub fn get_style_at(&self, index: usize) -> Result<TextStyle> {
        let tj_operation = &self.operators[index];
        if !Self::is_text_operation(tj_operation) {
            return Err(Error::ObjectType {
                expected: "Name",
                found: "Wrong operator",
            });
        }
        let tj_bytes = match tj_operation.operands[0] {
            Object::String(ref bytes, _) => bytes.clone(),
            Object::Array(ref array) => array
                .iter()
                .filter_map(|obj| match obj {
                    Object::String(ref bytes, _) => Some(bytes.as_slice()),
                    _ => None,
                })
                .flatten()
                .cloned()
                .collect(),
            _ => {
                panic!("Wrong operation type")
            }
        };
        let mut style = TextStyle::new(tj_bytes.to_vec());

        let mut tm_found = false;
        let mut tf_found = false;
        let mut td_found = false;
        let mut tl_found = false;

        for op in self.operators[..index].iter().rev() {
            match op.operator.as_str() {
                "Tm" => {
                    if tm_found {
                        continue;
                    }
                    // Skip matrix transformation
                    // TODO: Implement matrix transformation
                    // [ 1, 0, 0, 1, X, Y ]
                    let x = op.operands[4].as_float()?;
                    let y = op.operands[5].as_float()?;
                    style.pos = (x, y);
                    tm_found = true;
                }
                "Td" => {
                    if td_found {
                        continue;
                    }
                    // Apply relative text position move
                    let tx = op.operands[0].as_float()?;
                    let ty = op.operands[1].as_float()?;
                    // Update current position with the relative offset
                    style.pos = (style.pos.0 + tx, style.pos.1 + ty);
                    td_found = true;
                }
                "TL" => {
                    if tl_found {
                        continue;
                    }
                    // Set text leading (line spacing)
                    style.leading = op.operands[0].as_float()?;
                    tl_found = true;
                }
                "Tf" => {
                    if tf_found {
                        continue;
                    }
                    style.font_size = op.operands[1].as_float().unwrap();
                    tf_found = true;
                }
                "BT" => {
                    break;
                }
                _ => {}
            }
        }

        Ok(style)
    }

    fn is_text_operation(operation: &Operation) -> bool {
        operation.operator == "Tj" || operation.operator == "TJ"
    }

    fn validate_operators(operators: &Vec<Operation>) -> Result<()> {
        if operators.len() < 2 {
            return Err(Error::ObjectType {
                expected: "Name",
                found: "Wrong length of operators(BT ET not found)",
            });
        }
        if operators[0].operator != "BT" || operators[operators.len() - 1].operator != "ET" {
            return Err(Error::ObjectType {
                expected: "Name",
                found: "Wrong length of operators(BT ET not found)",
            });
        }
        Ok(())
    }
}
