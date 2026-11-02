//! Features: what each adds to the core's tables, so the core names none of
//! them. Every lookup of a command by name goes through here.

use helix_view::tree::Tab;

use crate::commands::{MappableCommand, TypableCommand, TYPABLE_COMMAND_LIST};
use crate::config::FeatureTable;

pub struct Feature {
    pub name: &'static str,
    /// Its static commands, which the keymap and the command palette name.
    pub commands: &'static [MappableCommand],
    /// Its typable commands.
    pub typable: &'static [TypableCommand],
    /// The tabs it opens, by name.
    pub tabs: &'static [Tab],
    /// Its top-level table of `config.toml`, if it has one.
    pub config: Option<FeatureTable>,
    /// Registers its hooks and the UI parts of its buffer kinds. Runs once
    /// per `Application`, from `handlers::setup`.
    pub register: fn(),
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

/// The code tab, then the tabs features open.
pub fn tabs() -> impl Iterator<Item = Tab> {
    std::iter::once(Tab::CODE).chain(
        features()
            .iter()
            .flat_map(|feature| feature.tabs.iter().copied()),
    )
}

/// The features' top-level tables.
pub fn config_tables() -> impl Iterator<Item = &'static FeatureTable> {
    features()
        .iter()
        .filter_map(|feature| feature.config.as_ref())
}
