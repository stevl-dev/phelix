use std::collections::HashMap;

use crate::input::KeyEvent;
use crate::keyboard::{KeyCode, KeyModifiers};

/// What a key does inside a picker. `NoOp` removes a default binding so the
/// key is handled by the picker prompt instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerCommand {
    Previous,
    Next,
    PageUp,
    PageDown,
    First,
    Last,
    Open,
    OpenBackground,
    OpenHorizontal,
    OpenVertical,
    TogglePreview,
    Close,
    /// An action of one picker on its selected item, by its name. Only that
    /// picker's table binds it.
    Action(&'static str),
    NoOp,
}

impl PickerCommand {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "previous" => Self::Previous,
            "next" => Self::Next,
            "page_up" => Self::PageUp,
            "page_down" => Self::PageDown,
            "first" => Self::First,
            "last" => Self::Last,
            "open" => Self::Open,
            "open_background" => Self::OpenBackground,
            "open_horizontal" => Self::OpenHorizontal,
            "open_vertical" => Self::OpenVertical,
            "toggle_preview" => Self::TogglePreview,
            "close" => Self::Close,
            "no_op" => Self::NoOp,
            _ => return None,
        })
    }
}

/// The keys of the pickers: the table every picker reads, and the tables of
/// the pickers that have a name, each of which goes first in its picker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickerKeys {
    pub shared: HashMap<KeyEvent, PickerCommand>,
    pub pickers: HashMap<&'static str, HashMap<KeyEvent, PickerCommand>>,
}

impl PickerKeys {
    /// What `key` does in the picker whose table is `picker`, or in a picker
    /// with no table.
    pub fn get(&self, picker: Option<&str>, key: &KeyEvent) -> Option<PickerCommand> {
        picker
            .and_then(|picker| self.pickers.get(picker))
            .and_then(|table| table.get(key))
            .or_else(|| self.shared.get(key))
            .copied()
    }

    /// Puts `overlay`'s bindings over these. `no_op` drops a shared key, and
    /// stays in a picker's table, where it hands the key to the prompt.
    pub fn overlay(&mut self, overlay: PickerKeys) {
        for (key, command) in overlay.shared {
            if command == PickerCommand::NoOp {
                self.shared.remove(&key);
            } else {
                self.shared.insert(key, command);
            }
        }
        for (picker, table) in overlay.pickers {
            self.pickers.entry(picker).or_default().extend(table);
        }
    }
}

/// The core's keys for every picker.
pub fn default_picker_keys() -> PickerKeys {
    let mut keys = HashMap::new();
    let bind = |keys: &mut HashMap<KeyEvent, PickerCommand>,
                code: KeyCode,
                modifiers: KeyModifiers,
                command: PickerCommand| {
        keys.insert(KeyEvent { code, modifiers }, command);
    };

    let none = KeyModifiers::NONE;
    let ctrl = KeyModifiers::CONTROL;
    let shift = KeyModifiers::SHIFT;
    let alt = KeyModifiers::ALT;

    bind(&mut keys, KeyCode::Tab, shift, PickerCommand::Previous);
    bind(&mut keys, KeyCode::Up, none, PickerCommand::Previous);
    bind(&mut keys, KeyCode::Char('p'), ctrl, PickerCommand::Previous);
    bind(&mut keys, KeyCode::Tab, none, PickerCommand::Next);
    bind(&mut keys, KeyCode::Down, none, PickerCommand::Next);
    bind(&mut keys, KeyCode::Char('n'), ctrl, PickerCommand::Next);
    bind(&mut keys, KeyCode::PageDown, none, PickerCommand::PageDown);
    bind(&mut keys, KeyCode::Char('d'), ctrl, PickerCommand::PageDown);
    bind(&mut keys, KeyCode::PageUp, none, PickerCommand::PageUp);
    bind(&mut keys, KeyCode::Char('u'), ctrl, PickerCommand::PageUp);
    bind(&mut keys, KeyCode::Home, none, PickerCommand::First);
    bind(&mut keys, KeyCode::End, none, PickerCommand::Last);
    bind(&mut keys, KeyCode::Enter, none, PickerCommand::Open);
    bind(
        &mut keys,
        KeyCode::Enter,
        alt,
        PickerCommand::OpenBackground,
    );
    bind(
        &mut keys,
        KeyCode::Char('s'),
        ctrl,
        PickerCommand::OpenHorizontal,
    );
    bind(
        &mut keys,
        KeyCode::Char('v'),
        ctrl,
        PickerCommand::OpenVertical,
    );
    bind(
        &mut keys,
        KeyCode::Char('t'),
        ctrl,
        PickerCommand::TogglePreview,
    );
    bind(&mut keys, KeyCode::Esc, none, PickerCommand::Close);
    bind(&mut keys, KeyCode::Char('c'), ctrl, PickerCommand::Close);
    PickerKeys {
        shared: keys,
        pickers: HashMap::new(),
    }
}
