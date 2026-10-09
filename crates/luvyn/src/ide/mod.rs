//! Host adapters share the same workspace session and embedded frontend.
mod android;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod host;
pub mod projects;
pub(crate) mod session;
