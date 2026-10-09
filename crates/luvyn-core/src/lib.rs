//! Shared compiler, indexed graph, editor services and compact context for Luvyn.
pub mod binary;
pub mod editor;
pub mod export;
pub mod formatter;
pub mod ide;
pub mod language;
pub mod lexer;
pub mod model;
pub mod parser;
pub mod projects;
pub mod query;
pub mod resolver;
pub mod sync;
pub mod workspace;

pub use binary::Artifact as LuReader;
pub use model::*;
pub use model::{Edge as GraphEdge, Symbol as GraphNode};
pub use workspace::{Project, ProjectConfig};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
