//! Features: what each adds to the core's tables, so the core names none of
//! them. Every lookup of a command by name goes through here.

use crate::commands::{MappableCommand, TypableCommand, TYPABLE_COMMAND_LIST};

pub struct Feature {
    pub name: &'static str,
    /// Its static commands, which the keymap and the command palette name.
    pub commands: &'static [MappableCommand],
    /// Its typable commands.
    pub typable: &'static [TypableCommand],
}

/// The features Helix is built with.
const FEATURES: &[Feature] = &[crate::commands::review::FEATURE];

pub fn features() -> &'static [Feature] {
    FEATURES
}

/// The core's static commands, then each feature's.
pub fn static_commands() -> impl Iterator<Item = &'static MappableCommand> {
    MappableCommand::STATIC_COMMAND_LIST
        .iter()
        .chain(features().iter().flat_map(|feature| feature.commands))
}

/// The core's typable commands, then each feature's.
pub fn typable_commands() -> impl Iterator<Item = &'static TypableCommand> {
    TYPABLE_COMMAND_LIST
        .iter()
        .chain(features().iter().flat_map(|feature| feature.typable))
}
