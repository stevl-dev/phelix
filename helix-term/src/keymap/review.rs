//! The review stream's keymap: the user's keymap with the stream's commands
//! swapped in and its bindings over it. The review's buffer kind hands it to
//! `KindKeys`, which swaps it in while the stream has focus.

use std::collections::HashMap;

use helix_core::hashmap;
use helix_view::document::Mode;

use super::macros::keymap;
use super::{merge_keys, KeyTrie, MappableCommand};

pub(crate) fn keymap(user: &HashMap<Mode, KeyTrie>) -> HashMap<Mode, KeyTrie> {
    let mut map = user.clone();
    for keys in map.values_mut() {
        replace_commands(keys);
    }
    merge_keys(&mut map, bindings());
    map
}

fn replace_commands(keys: &mut KeyTrie) {
    let review = |command: &MappableCommand| match command.name() {
        "goto_next_change" => Some(MappableCommand::review_next_hunk),
        "goto_prev_change" => Some(MappableCommand::review_prev_hunk),
        "goto_first_change" => Some(MappableCommand::review_first_hunk),
        "goto_last_change" => Some(MappableCommand::review_last_hunk),
        // WHY: the review's log is a line, with no branches to step through,
        // so one step earlier is one undo.
        "undo" | "earlier" => Some(MappableCommand::review_undo),
        "redo" | "later" => Some(MappableCommand::review_redo),
        _ => crate::commands::review::replacement(command),
    };
    match keys {
        KeyTrie::MappableCommand(command) => {
            if let Some(review) = review(command) {
                *command = review;
            }
        }
        KeyTrie::Sequence(commands) => {
            for command in commands {
                if let Some(review) = review(command) {
                    *command = review;
                }
            }
        }
        KeyTrie::Node(node) => {
            for keys in node.values_mut() {
                replace_commands(keys);
            }
        }
    }
}

fn bindings() -> HashMap<Mode, KeyTrie> {
    let keys = keymap!({ "Review"
        "ret" => review_open,
    });
    hashmap!(
        Mode::Normal => keys.clone(),
        Mode::Select => keys,
    )
}
