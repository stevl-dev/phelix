use crate::keymap;
use crate::keymap::{merge_keys, KeyTrie};
use helix_loader::merge_toml_values;
use helix_view::editor::{default_picker_keys, overlay_picker_keys, PickerCommand};
use helix_view::features::Features;
use helix_view::input::KeyEvent;
use helix_view::{document::Mode, theme};
use serde::de::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::Error as IOError;
use std::str::FromStr;
use toml::de::Error as TomlError;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub theme: Option<theme::Config>,
    pub keys: HashMap<Mode, KeyTrie>,
    pub editor: helix_view::editor::Config,
    /// The top-level tables features read, by name, as the file has them.
    pub features: HashMap<&'static str, toml::Value>,
}

/// A feature's top-level table: its name, and how it reads the table into the
/// feature's state, or why it can't. It gets `None` when the table is absent.
pub struct FeatureTable {
    pub name: &'static str,
    pub apply: fn(&mut Features, Option<&toml::Value>) -> Result<(), TomlError>,
}

/// The features' tables.
const FEATURE_TABLES: &[FeatureTable] = &[FeatureTable {
    name: "review",
    apply: crate::commands::review::apply_config,
}];

/// The top-level tables of the core's own.
const CORE_TABLES: &[&str] = &["theme", "keys", "editor"];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawKeys {
    pub modes: HashMap<Mode, KeyTrie>,
    /// Bindings from `[keys.picker]` only. Defaults are applied later.
    pub picker: HashMap<KeyEvent, PickerCommand>,
}

impl<'de> Deserialize<'de> for RawKeys {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut value = toml::Value::deserialize(deserializer).map_err(D::Error::custom)?;
        let picker = match value
            .as_table_mut()
            .ok_or_else(|| D::Error::custom("keys must be a table"))?
            .remove("picker")
        {
            Some(picker) => parse_picker_keys(picker).map_err(D::Error::custom)?,
            None => HashMap::new(),
        };
        let modes = HashMap::<Mode, KeyTrie>::deserialize(value).map_err(D::Error::custom)?;
        Ok(Self { modes, picker })
    }
}

fn parse_picker_keys(value: toml::Value) -> Result<HashMap<KeyEvent, PickerCommand>, String> {
    let table = value
        .as_table()
        .ok_or_else(|| "picker keymap must be a table of key = \"command\"".to_string())?;
    let mut keys = HashMap::new();
    for (key, command) in table {
        let key = KeyEvent::from_str(key).map_err(|err| err.to_string())?;
        let command = command
            .as_str()
            .ok_or_else(|| format!("picker binding for '{key}' must be a command name"))?;
        let command = PickerCommand::from_name(command)
            .ok_or_else(|| format!("unknown picker command '{command}'"))?;
        keys.insert(key, command);
    }
    Ok(keys)
}

fn apply_picker_keys(
    editor: &mut helix_view::editor::Config,
    overlays: impl IntoIterator<Item = HashMap<KeyEvent, PickerCommand>>,
) {
    let mut picker_keys = default_picker_keys();
    for overlay in overlays {
        overlay_picker_keys(&mut picker_keys, overlay);
    }
    editor.picker_keys = picker_keys;
}

// WHY: no `deny_unknown_fields`, since the features' tables sit beside these.
// `feature_tables` refuses a name that's neither the core's nor a feature's.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConfigRaw {
    pub theme: Option<theme::Config>,
    pub keys: Option<RawKeys>,
    pub editor: Option<toml::Value>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: None,
            keys: keymap::default(),
            editor: helix_view::editor::Config::default(),
            features: HashMap::new(),
        }
    }
}

/// The tables of `file` that features read, by name. Refused with a
/// top-level name that's neither the core's nor a feature's.
fn feature_tables(file: &str) -> Result<HashMap<&'static str, toml::Value>, TomlError> {
    let table: toml::Table = toml::from_str(file)?;
    let mut tables = HashMap::new();
    for (name, value) in table {
        if CORE_TABLES.contains(&name.as_str()) {
            continue;
        }
        let Some(feature) = FEATURE_TABLES.iter().find(|feature| feature.name == name) else {
            let expected: Vec<_> = CORE_TABLES
                .iter()
                .chain(FEATURE_TABLES.iter().map(|feature| &feature.name))
                .map(|name| format!("`{name}`"))
                .collect();
            return Err(TomlError::custom(format!(
                "unknown field `{name}`, expected one of {}",
                expected.join(", ")
            )));
        };
        tables.insert(feature.name, value);
    }
    Ok(tables)
}

fn parse(file: &str) -> Result<(ConfigRaw, HashMap<&'static str, toml::Value>), ConfigLoadError> {
    let raw = toml::from_str(file).map_err(ConfigLoadError::BadConfig)?;
    let tables = feature_tables(file).map_err(ConfigLoadError::BadConfig)?;
    Ok((raw, tables))
}

#[derive(Debug)]
pub enum ConfigLoadError {
    BadConfig(TomlError),
    Error(IOError),
}

impl Default for ConfigLoadError {
    fn default() -> Self {
        ConfigLoadError::Error(IOError::new(std::io::ErrorKind::NotFound, "place holder"))
    }
}

impl Display for ConfigLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigLoadError::BadConfig(err) => err.fmt(f),
            ConfigLoadError::Error(err) => err.fmt(f),
        }
    }
}

impl Config {
    pub fn load(
        global: Result<&String, ConfigLoadError>,
        local: Result<String, ConfigLoadError>,
    ) -> Result<Config, ConfigLoadError> {
        let global_config = global.and_then(|file| parse(file));
        let local_config = local.and_then(|file| parse(&file));
        let res = match (global_config, local_config) {
            (Ok((global, mut features)), Ok((local, local_features))) => {
                for (name, local) in local_features {
                    let merged = match features.remove(name) {
                        Some(global) => merge_toml_values(global, local, 3),
                        None => local,
                    };
                    features.insert(name, merged);
                }
                let mut keys = keymap::default();
                let mut picker_overlays = Vec::new();
                if let Some(global_keys) = global.keys {
                    merge_keys(&mut keys, global_keys.modes);
                    picker_overlays.push(global_keys.picker);
                }
                if let Some(local_keys) = local.keys {
                    merge_keys(&mut keys, local_keys.modes);
                    picker_overlays.push(local_keys.picker);
                }

                let mut editor = match (global.editor, local.editor) {
                    (None, None) => helix_view::editor::Config::default(),
                    (None, Some(val)) | (Some(val), None) => {
                        val.try_into().map_err(ConfigLoadError::BadConfig)?
                    }
                    (Some(global), Some(local)) => merge_toml_values(global, local, 3)
                        .try_into()
                        .map_err(ConfigLoadError::BadConfig)?,
                };
                apply_picker_keys(&mut editor, picker_overlays);

                Config {
                    theme: local.theme.or(global.theme),
                    keys,
                    editor,
                    features,
                }
            }
            // if any configs are invalid return that first
            (_, Err(ConfigLoadError::BadConfig(err)))
            | (Err(ConfigLoadError::BadConfig(err)), _) => {
                return Err(ConfigLoadError::BadConfig(err))
            }
            (Ok((config, features)), Err(_)) | (Err(_), Ok((config, features))) => {
                let mut keys = keymap::default();
                let picker_overlay = if let Some(keymap) = config.keys {
                    merge_keys(&mut keys, keymap.modes);
                    keymap.picker
                } else {
                    HashMap::new()
                };
                let mut editor = config.editor.map_or_else(
                    || Ok(helix_view::editor::Config::default()),
                    |val| val.try_into().map_err(ConfigLoadError::BadConfig),
                )?;
                apply_picker_keys(&mut editor, [picker_overlay]);
                Config {
                    theme: config.theme,
                    keys,
                    editor,
                    features,
                }
            }

            // these are just two io errors return the one for the global config
            (Err(err), Err(_)) => return Err(err),
        };

        // WHY: a feature's table is read into its state only once the editor
        // exists. Reading it into a scratch state here refuses a bad one with
        // the rest of the file, as a bad `[editor]` is.
        res.apply_features(&mut Features::default())
            .map_err(ConfigLoadError::BadConfig)?;
        Ok(res)
    }

    /// Reads each feature's table into its state in `features`.
    pub fn apply_features(&self, features: &mut Features) -> Result<(), TomlError> {
        for table in FEATURE_TABLES {
            (table.apply)(features, self.features.get(table.name))?;
        }
        Ok(())
    }

    pub fn load_default() -> Result<Config, ConfigLoadError> {
        let global_config =
            fs::read_to_string(helix_loader::config_file()).map_err(ConfigLoadError::Error)?;
        let local_config = fs::read_to_string(helix_loader::workspace_config_file())
            .map_err(ConfigLoadError::Error);

        let phony_config = ConfigLoadError::Error(IOError::other("hacky placeholder"));
        let global_parsed = Config::load(Ok(&global_config), Err(phony_config))?;

        // We need to build a transient `WorkspaceTrust` just to ask whether the workspace is
        // trusted enough to load its `.helix/config.toml`. The persisted-trust file on disk is the
        // source of truth either way; this transient instance has an empty cache and is dropped
        // after the check.
        let trust = helix_loader::workspace_trust::WorkspaceTrust::new(
            (&global_parsed.editor.workspace_trust).into(),
        );
        if trust
            .query_current(helix_loader::workspace_trust::TrustQuery::LocalConfig)
            .is_trusted()
        {
            let mut merged = Config::load(Ok(&global_config), local_config)?;
            // editor.workspace-trust is global/user-scope only. Without this override, a
            // workspace's `.helix/config.toml` could set `level = "insecure"`; once the user trusted
            // *that* workspace, refresh_config would re-load with the override merged in and from
            // then on every subsequent workspace in the session would be implicitly trusted. Pin
            // the gate's own configuration to the global file.
            merged.editor.workspace_trust = global_parsed.editor.workspace_trust;
            Ok(merged)
        } else {
            Ok(global_parsed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Config {
        fn load_test(config: &str) -> Config {
            Config::load(Ok(&config.to_owned()), Err(ConfigLoadError::default())).unwrap()
        }
    }

    #[test]
    fn parsing_keymaps_config_file() {
        use crate::keymap;
        use helix_core::hashmap;
        use helix_view::document::Mode;

        let sample_keymaps = r#"
            [keys.insert]
            y = "move_line_down"
            S-C-a = "delete_selection"

            [keys.normal]
            A-F12 = "move_next_word_end"
        "#;

        let mut keys = keymap::default();
        merge_keys(
            &mut keys,
            hashmap! {
                Mode::Insert => keymap!({ "Insert mode"
                    "y" => move_line_down,
                    "S-C-a" => delete_selection,
                }),
                Mode::Normal => keymap!({ "Normal mode"
                    "A-F12" => move_next_word_end,
                }),
            },
        );

        assert_eq!(
            Config::load_test(sample_keymaps),
            Config {
                keys,
                ..Default::default()
            }
        );
    }

    #[test]
    fn keys_resolve_to_correct_defaults() {
        // From serde default
        let default_keys = Config::load_test("").keys;
        assert_eq!(default_keys, keymap::default());

        // From the Default trait
        let default_keys = Config::default().keys;
        assert_eq!(default_keys, keymap::default());
    }

    #[test]
    fn picker_keys_overlay_defaults_and_no_op_unbinds() {
        use helix_view::editor::PickerCommand;
        use helix_view::input::KeyEvent;
        use std::str::FromStr;

        let config = Config::load_test(
            r#"
            [keys.picker]
            C-k = "previous"
            C-p = "no_op"
            "#,
        );
        let keys = &config.editor.picker_keys;
        let ctrl_k = KeyEvent::from_str("C-k").unwrap();
        let ctrl_p = KeyEvent::from_str("C-p").unwrap();
        let ctrl_n = KeyEvent::from_str("C-n").unwrap();
        assert_eq!(keys.get(&ctrl_k), Some(&PickerCommand::Previous));
        assert!(!keys.contains_key(&ctrl_p));
        assert_eq!(keys.get(&ctrl_n), Some(&PickerCommand::Next));
    }

    #[test]
    fn config_refuses_a_top_level_table_no_one_reads() {
        let refused = Config::load(Ok(&"[keyss]\n".to_owned()), Err(ConfigLoadError::default()))
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("unknown field `keyss`, expected one of `theme`, `keys`, `editor`"),
            "{refused}"
        );
    }
}
