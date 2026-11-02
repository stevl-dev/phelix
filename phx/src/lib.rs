//! The features Helix is built with. The binary hands them to `helix-term`
//! before anything reads a command or a config by name.

use helix_term::feature::Feature;

pub const FEATURES: &[Feature] = &[helix_review::FEATURE];
