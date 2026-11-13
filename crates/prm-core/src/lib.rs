//! Domain core for Prism Reference Manager.
//!
//! The core owns reference identity, project membership, citation rendering, annotations,
//! imports, exports and change events. The desktop shell, the local API and tests all call
//! the same operations through [`library::Library`].

pub mod error;
pub mod util;
pub mod model;
pub mod store;
pub mod library;
pub mod render;
pub mod formats;
pub mod search;
pub mod dedupe;
pub mod backup;
pub mod pdf;
pub mod lookup;
pub mod health;
pub mod citekey;
pub mod types;

pub use error::{Error, Result};
