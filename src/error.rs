//! Tipo de error común del programa.
//!
//! Cada fuente de error (Windows, SQLite, archivos, JSON) se envuelve en una
//! variante. Con las implementaciones de `From`, el operador `?` convierte
//! automáticamente cualquiera de ellas en [`Error`].

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Windows(windows::core::Error),
    Sql(rusqlite::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    /// Error con un mensaje propio (por ejemplo, un parámetro inválido).
    Mensaje(String),
}

pub type Resultado<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Windows(e) => write!(f, "Windows: {e}"),
            Error::Sql(e) => write!(f, "SQLite: {e}"),
            Error::Io(e) => write!(f, "archivo: {e}"),
            Error::Json(e) => write!(f, "JSON: {e}"),
            Error::Mensaje(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<windows::core::Error> for Error {
    fn from(e: windows::core::Error) -> Self {
        Error::Windows(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sql(e)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

/// Atajo para crear un error con mensaje.
pub fn error(m: impl Into<String>) -> Error {
    Error::Mensaje(m.into())
}
