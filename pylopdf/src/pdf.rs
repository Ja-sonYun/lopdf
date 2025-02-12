use lopdf::OverwriteDocument;
use pyo3::prelude::*;
use pyo3::types::PyString;

#[pyclass(subclass)]
#[pyo3(name = "Pdf")]
#[derive(Debug, Clone)]
pub struct Pdf {
    pub(crate) t: OverwriteDocument,
}

#[pymethods]
impl Pdf {
    #[new]
    #[pyo3()]
    fn new(_py: Python<'_>, path: String) -> PyResult<Self> {
        return Ok(Pdf {
            t: OverwriteDocument::load_from_path(path).unwrap(),
        });
    }

    fn is_encrypted(&mut self) -> bool {
        return self.t.is_encrypted;
    }

    fn set_password(&mut self, password: String) {
        self.t.set_password(password);
    }

    fn highlight_text(&mut self, text: String) {
        self.t.highlight_text(&text).unwrap();
    }

    fn redact_text(&mut self, text: String) {
        self.t.redact_text(&text).unwrap();
    }

    fn save(&mut self, path: String) {
        self.t.save(path).unwrap();
    }
}
