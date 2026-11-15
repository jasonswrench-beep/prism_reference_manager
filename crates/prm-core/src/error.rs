use thiserror::Error;

/// Errors carry a plain-language message for the person using the program and an optional
/// technical detail for the support area. The interface shows `message` and offers `detail`
/// in an expandable section.
#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    User(String),
    #[error("The record could not be found: {0}")]
    NotFound(String),
    #[error("The library database reported a problem: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("A file could not be read or written: {0}")]
    Io(#[from] std::io::Error),
    #[error("The data could not be read: {0}")]
    Json(#[from] serde_json::Error),
    #[error("The archive could not be read or written: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("The network request failed: {0}")]
    Network(String),
    #[error("This library was created by a newer version (schema {found}); it is open read-only. Export what you need or update the program.")]
    SchemaTooNew { found: i64 },
    /// The library is marked open by another program window or computer; the text says which
    /// and what to do (see `store::acquire_lock`).
    #[error("{0}")]
    Locked(String),
    #[error("The PDF could not be processed: {0}")]
    Pdf(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn user(msg: impl Into<String>) -> Self {
        Error::User(msg.into())
    }
}

impl From<lopdf::Error> for Error {
    fn from(e: lopdf::Error) -> Self {
        Error::Pdf(e.to_string())
    }
}

impl From<csv::Error> for Error {
    fn from(e: csv::Error) -> Self {
        Error::User(format!("The spreadsheet file could not be read: {e}"))
    }
}

impl From<quick_xml::Error> for Error {
    fn from(e: quick_xml::Error) -> Self {
        Error::User(format!("The XML file could not be read: {e}"))
    }
}
