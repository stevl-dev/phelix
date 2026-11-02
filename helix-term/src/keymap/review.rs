//! The review stream's keymap: the user's keymap with the stream's commands
//! swapped in and its bindings over it. The review's buffer kind hands it to
//! `KindKeys`, which swaps it in while the stream has focus.

use std::collections::HashMap;

use helix_core::hashmap;
use helix_view::document::Mode;

use indexmap::IndexMap;

use super::macros::key;
use super::{merge_keys, KeyTrie, KeyTrieNode, MappableCommand};
use crate::commands::review;

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
        "goto_next_change" => Some(review::NEXT_HUNK),
        "goto_prev_change" => Some(review::PREV_HUNK),
        "goto_first_change" => Some(review::FIRST_HUNK),
        "goto_last_change" => Some(review::LAST_HUNK),
        // WHY: the review's log is a line, with no branches to step through,
        // so one step earlier is one undo.
        "undo" | "earlier" => Some(review::UNDO),
        "redo" | "later" => Some(review::REDO),
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
    let keys = KeyTrie::Node(KeyTrieNode::new(
        "Review",
        IndexMap::from([(key!(Enter), KeyTrie::MappableCommand(review::OPEN))]),
    ));
    hashmap!(
        Mode::Normal => keys.clone(),
        Mode::Select => keys,
    )
}
