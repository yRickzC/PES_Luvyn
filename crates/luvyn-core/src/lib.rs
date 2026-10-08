//! Shared compiler, indexed graph, editor services and compact context for Luvyn.
pub mod binary;
pub mod editor;
pub mod export;
pub mod formatter;
pub mod model;
pub mod parser;
pub mod query;
pub mod resolver;
pub mod workspace;

pub use model::*;
pub use workspace::{Project, ProjectConfig};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
