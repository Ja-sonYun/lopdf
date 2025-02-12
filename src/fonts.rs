use fontdb::{Database, Family};
use std::fs::File;
use std::io::Read;
use ttf_parser::Face;

#[derive(Debug)]
pub struct Fonts {
    pub name: String,

    path: Option<String>,
    face: Option<Face<'static>>,
}

impl Fonts {
    /// Creates a new font instance and searches for it in the system
    pub fn new(font_name: &str) -> Self {
        let mut db = Database::new();
        db.load_system_fonts();

        // Searches for the system font path
        let font_path = db
            .query(&fontdb::Query {
                families: &[Family::Name(font_name)],
                ..Default::default()
            })
            .and_then(|id| db.face(id))
            .and_then(|font| match &font.source {
                fontdb::Source::File(path) => Some(path.to_string_lossy().to_string()),
                _ => None,
            });

        let face = font_path.as_ref().and_then(|path| Self::load_face(path));

        Fonts {
            name: font_name.to_string(),
            path: font_path,
            face,
        }
    }

    pub fn from_path(path: &str) -> Self {
        let face = Self::load_face(path);
        Fonts {
            name: path.to_string(),
            path: Some(path.to_string()),
            face,
        }
    }

    /// Loads and parses the font file
    fn load_face(path: &str) -> Option<Face<'static>> {
        let mut font_data = Vec::new();
        if File::open(path)
            .and_then(|mut file| file.read_to_end(&mut font_data))
            .is_err()
        {
            println!("Failed to read font file '{}'", path);
            return None;
        }

        // Convert font_data to 'static lifetime
        let font_data = Box::leak(font_data.into_boxed_slice());
        Face::parse(font_data, 0).ok()
    }

    /// Get the width of a specific character as PDF units
    pub fn get_glyph_width(&self, character: char, pdf_font_size: f32) -> Option<f32> {
        let face = self.face.as_ref()?;
        let glyph_id = face.glyph_index(character)?;
        let width = face.glyph_hor_advance(glyph_id)?;
        let units_per_em = face.units_per_em();

        // Convert to PDF units
        Some((width as f32 / units_per_em as f32) * pdf_font_size)
    }

    /// Get the width of a string as PDF units
    pub fn get_str_glyph_width(&self, string: &str, pdf_font_size: f32) -> f32 {
        string
            .chars()
            .filter_map(|c| self.get_glyph_width(c, pdf_font_size)) // Skip missing glyphs
            .sum() // Sum all character widths
    }

    /// Get the ascent (distance above the baseline) in PDF units
    pub fn get_ascent(&self, pdf_font_size: f32) -> Option<f32> {
        let face = self.face.as_ref()?;
        let ascent = face.ascender(); // Font units for ascent
        let units_per_em = face.units_per_em();

        // Convert to PDF units
        Some((ascent as f32 / units_per_em as f32) * pdf_font_size)
    }

    /// Get the descent (distance below the baseline) in PDF units
    pub fn get_descent(&self, pdf_font_size: f32) -> Option<f32> {
        let face = self.face.as_ref()?;
        let descent = face.descender(); // Font units for descent
        let units_per_em = face.units_per_em();

        // Convert to PDF units
        Some((descent as f32 / units_per_em as f32) * pdf_font_size)
    }

    /// Get the line height (distance from one baseline to the next) in PDF units
    pub fn get_line_height(&self, pdf_font_size: f32) -> Option<f32> {
        let face = self.face.as_ref()?;
        let height = face.height(); // Font units for line height
        let units_per_em = face.units_per_em();

        // Convert to PDF units
        Some((height as f32 / units_per_em as f32) * pdf_font_size)
    }
}
