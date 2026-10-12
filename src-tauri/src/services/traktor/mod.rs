//! Read-only Traktor `collection.nml` import support (S3: parser + dry-run report).
//!
//! This module is deliberately hermetic: it only turns an NML document into a
//! typed model plus an [`mapper::ImportPlan`] describing what an import *would*
//! do. It performs no database access, no filesystem writes, no Tauri calls and
//! no `async` work. The `desktop` feature must not gate it — mobile builds
//! compile this module too (AGENTS.md §5.7).
#![allow(dead_code)] // S4 (the real importer) consumes this surface; S3 ships parser + report only.

pub mod key;
pub mod mapper;
pub mod model;
pub mod parser;
pub mod path;

#[cfg(test)]
mod tests;

use thiserror::Error;

/// Failure modes of reading and mapping a Traktor NML file.
#[derive(Debug, Error)]
pub enum TraktorError {
    /// The document is not well-formed XML, or an attribute could not be decoded.
    #[error("NML XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("NML I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid NML: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, TraktorError>;
