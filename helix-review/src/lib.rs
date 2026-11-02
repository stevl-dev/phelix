//! The review: every changed file's hunks in one buffer, in a tab of its own,
//! with edits that go to the files. `view` holds the stream and its sync,
//! beside `helix-view`, and `term` its commands, keys and drawing, beside
//! `helix-term`. The core names neither: the review reaches it through
//! `FEATURE`, which the `hx` crate lists.

pub mod term;
pub mod view;

pub use term::FEATURE;
