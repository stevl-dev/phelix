//! Features: what each adds to the core's tables, so the core names none of
//! them. Every lookup of a command by name goes through here.

use std::sync::OnceLock;

use helix_vcs::{ChangeView, FileChange};
use helix_view::input::KeyEvent;
use helix_view::tree::Tab;

use crate::commands::{MappableCommand, TypableCommand, TYPABLE_COMMAND_LIST};
use crate::compositor;
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
    /// Its actions in the changed-file picker.
    pub picker_actions: &'static [PickerAction],
    /// Registers its hooks and the UI parts of its buffer kinds. Runs once
    /// per `Application`, from `handlers::setup`.
    pub register: fn(),
}

/// An action on the changed-file picker's selected file, which
/// `[keys.picker]` binds by its name, as `PickerCommand::Action`.
pub struct PickerAction {
    pub name: &'static str,
    /// The key it has before `[keys.picker]`.
    pub key: KeyEvent,
    /// Runs it with the picker's list, unstaged, staged or both, and the file.
    pub run: fn(&mut compositor::Context, ChangeView, &FileChange),
}

static FEATURES: OnceLock<&'static [Feature]> = OnceLock::new();

/// Sets the features Helix is built with. The binary calls it before it reads
/// a config, and a test harness before it builds an `Application`. Again
/// with the same features does nothing, so every test can call it.
pub fn init(features: &'static [Feature]) {
    let set = FEATURES.get_or_init(|| features);
    let names = |features: &[Feature]| {
        features
            .iter()
            .map(|feature| feature.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(set), names(features), "the features are set once");
}

/// The features Helix is built with, or none before `init`.
pub fn features() -> &'static [Feature] {
    FEATURES.get().copied().unwrap_or(&[])
}

/// The core's static commands, then each feature's.
pub fn static_commands() -> impl Iterator<Item = &'static MappableCommand> {
    MappableCommand::STATIC_COMMAND_LIST
        .iter()
        .chain(features().iter().flat_map(|feature| feature.commands))
}

/// The typable command `name` names, by its name or an alias.
// WHY: no map built once. One built before `init` would leave the features'
// commands out for good.
pub fn typable_command(name: &str) -> Option<&'static TypableCommand> {
    typable_commands().find(|command| command.name == name || command.aliases.contains(&name))
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

/// The features' picker actions.
pub fn picker_actions() -> impl Iterator<Item = &'static PickerAction> {
    features().iter().flat_map(|feature| feature.picker_actions)
}
