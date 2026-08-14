//! The layout engine: cull -> pack -> crop -> score -> pace -> assemble.
//!
//! Every stage is a pure function over plain data, testable without an
//! `AppHandle` or a live sidecar -- the pattern this codebase already
//! follows in `finalize_photos`, `analyze_batches`, `lookup_cache`,
//! `percentiles` and `imageNormalizedTopLeft`.

pub mod crop;
pub mod cull;
pub mod manifest;
pub mod pace;
pub mod pack;
pub mod preflight;
pub mod score;
