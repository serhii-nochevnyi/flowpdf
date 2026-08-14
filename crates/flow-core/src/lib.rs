//! Canonical FlowPDF document core.
//!
//! Phase 1 expands this crate from a compile-safe boundary into the durable
//! document model. Keeping the boundary real from Wave 0 makes Cargo metadata,
//! native tests, and the WASM adapter share one locked workspace immediately.

#![forbid(unsafe_code)]
