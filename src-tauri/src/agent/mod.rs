//! The agent layer's Rust half. Everything a model may learn about a book is
//! built here, so the privacy boundary of design §9.1 is one module rather
//! than a convention spread across the tools.

pub mod keys;
pub mod view;
