use pyo3::prelude::*;

pub mod pdf;

#[pymodule]
pub mod pylopdf {
    use super::*;

    #[pymodule_export]
    use pdf::Pdf;
}
