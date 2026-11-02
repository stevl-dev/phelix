//! `:review` and the commands of the review stream. The rows and the buffer
//! live in `helix_view::review`. This module lists the changed files, reads
//! both sides of each, and opens a row's file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context as _};
use helix_core::command_line::{Args, Signature};
use helix_core::fuzzy::fuzzy_match;
use helix_core::line_ending::line_end_char_index;
use helix_core::movement::Direction;
use helix_core::syntax::OverlayHighlights;
use helix_core::{encoding, Range, Rope, RopeSlice, Selection};
use helix_event::register_hook;
use helix_loader::workspace_trust::TrustQuery;
use helix_vcs::{index, visible_changes, ChangeKind, ChangeView, DiffProviderRegistry, FileChange};
use helix_view::document::from_reader;
use helix_view::document::Mode;
use helix_view::editor::Action;
use helix_view::features::Features;
use helix_view::input::KeyEvent;
use helix_view::keyboard::{KeyCode, KeyModifiers};
use helix_view::review::{
    self, Content, ReviewConfig, ReviewState, Row, Section, Stream, REVIEW_TAB,
};
use helix_view::tree::Tab;
use helix_view::{
    align_view, current, current_ref, doc, view, Align, Document, DocumentId, Editor, Theme, View,
};

mod lsp;
mod restore;
mod stage;
mod write;

pub(crate) use lsp::replacement;
pub(super) use restore::review_restore;
pub(super) use stage::{stage_hunk, unstage_hunk};

use super::{push_jump, Context};
use crate::commands::typed::{CommandCompleter, TypableCommand};
use crate::commands::MappableCommand;
use crate::commands::WriteOptions;
use crate::config::FeatureTable;
use crate::events::EditorWillUpdate;
use crate::feature::{Feature, PickerAction};
use crate::keymap::KeyTrie;
use crate::kind::{self, BufferKindUi};
use crate::ui::text_decorations::DecorationManager;

/// The review's commands.
pub const FEATURE: Feature = Feature {
    name: "review",
    commands: &[
        OPEN, NEXT_HUNK, PREV_HUNK, FIRST_HUNK, LAST_HUNK, UNDO, REDO,
    ],
    tabs: &[REVIEW_TAB],
    config: Some(FeatureTable {
        name: "review",
        apply: apply_config,
    }),
    register,
    picker_actions: &[PickerAction {
        name: "review_jump",
        key: KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::ALT,
        },
        run: |cx, scope, change| jump(cx, scope, change.path()),
    }],
    typable: &[
        TypableCommand {
            name: "review",
            aliases: &[],
            doc: "Show the changes of every changed file in one buffer, in a tab of its own, or switch to it. Scopes: all (HEAD against the files, the default), unstaged (the index against the files), staged (HEAD against the index)",
            fun: review,
            completer: CommandCompleter::positional(&[complete_scope]),
            signature: Signature {
                positionals: (0, Some(1)),
                ..Signature::DEFAULT
            },
        },
        TypableCommand {
            name: "stage-hunk",
            aliases: &[],
            doc: "Stage the hunks the selections touch in the review",
            fun: stage_hunk,
            completer: CommandCompleter::none(),
            signature: Signature {
                positionals: (0, Some(0)),
                ..Signature::DEFAULT
            },
        },
        TypableCommand {
            name: "unstage-hunk",
            aliases: &[],
            doc: "Unstage the hunks the selections touch in the review",
            fun: unstage_hunk,
            completer: CommandCompleter::none(),
            signature: Signature {
                positionals: (0, Some(0)),
                ..Signature::DEFAULT
            },
        },
        TypableCommand {
            name: "review-restore",
            aliases: &[],
            doc: "Put the base's text back over the hunks the selections touch in the review, in each file's buffer",
            fun: review_restore,
            completer: CommandCompleter::none(),
            signature: Signature {
                positionals: (0, Some(0)),
                ..Signature::DEFAULT
            },
        },
        TypableCommand {
            name: "review-context",
            aliases: &[],
            doc: "Show the given number of unchanged lines around each hunk of the review, or say how many it shows",
            fun: review_context,
            completer: CommandCompleter::none(),
            signature: Signature {
                positionals: (0, Some(1)),
                ..Signature::DEFAULT
            },
        },
    ],
};

pub(crate) const OPEN: MappableCommand = MappableCommand::Static {
    name: "review_open",
    fun: open,
    doc: "Open the file of the review row under the cursor, or show the lines of a gap",
};
pub(crate) const NEXT_HUNK: MappableCommand = MappableCommand::Static {
    name: "review_next_hunk",
    fun: next_hunk,
    doc: "Select the next hunk of the review",
};
pub(crate) const PREV_HUNK: MappableCommand = MappableCommand::Static {
    name: "review_prev_hunk",
    fun: prev_hunk,
    doc: "Select the previous hunk of the review",
};
pub(crate) const FIRST_HUNK: MappableCommand = MappableCommand::Static {
    name: "review_first_hunk",
    fun: first_hunk,
    doc: "Select the first hunk of the review",
};
pub(crate) const LAST_HUNK: MappableCommand = MappableCommand::Static {
    name: "review_last_hunk",
    fun: last_hunk,
    doc: "Select the last hunk of the review",
};
pub(crate) const UNDO: MappableCommand = MappableCommand::Static {
    name: "review_undo",
    fun: undo,
    doc: "Undo the review's last edit in the files it changed",
};
pub(crate) const REDO: MappableCommand = MappableCommand::Static {
    name: "review_redo",
    fun: redo,
    doc: "Redo the review's last undone edit in the files it changed",
};

/// The `helix-term` side of the review's buffer kind.
struct Ui;

static UI: Ui = Ui;

impl BufferKindUi for Ui {
    fn reload(&self, cx: &mut compositor::Context, _doc: DocumentId) {
        reload(cx);
    }

    fn write(
        &self,
        cx: &mut compositor::Context,
        _doc: DocumentId,
        path: Option<&str>,
        options: WriteOptions,
    ) -> anyhow::Result<()> {
        write::write(cx, path, options)
    }

    fn keymap(&self, user: &HashMap<Mode, KeyTrie>) -> Option<HashMap<Mode, KeyTrie>> {
        Some(crate::keymap::review::keymap(user))
    }

    fn decorate<'a>(
        &self,
        editor: &'a Editor,
        doc: &'a Document,
        view: &View,
        theme: &Theme,
        decorations: &mut DecorationManager<'a>,
        overlays: &mut Vec<OverlayHighlights>,
    ) {
        crate::ui::text_decorations::review::render(
            editor,
            doc,
            view,
            theme,
            decorations,
            overlays,
        );
    }
}

/// Reads `[review]` into the review's state.
fn apply_config(
    features: &mut Features,
    table: Option<&toml::Value>,
) -> Result<(), toml::de::Error> {
    let config = match table {
        Some(table) => table.clone().try_into()?,
        None => ReviewConfig::default(),
    };
    features.review_mut().set_config(config);
    Ok(())
}

/// Registers the review's hook and the UI part of its buffer kind.
fn register() {
    kind::register(review::KIND_NAME, &UI);
    register_hook!(move |event: &mut EditorWillUpdate<'_>| {
        review::sync(event.editor, event.completing);
        Ok(())
    });
}
use crate::compositor;
use crate::job::Callback;
use crate::ui::prompt::Completion;
use crate::ui::PromptEvent;

const SCOPES: [ChangeView; 3] = [ChangeView::All, ChangeView::Unstaged, ChangeView::Staged];

fn review(cx: &mut compositor::Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let scope = match args.first() {
        None => None,
        Some(name) => Some(
            SCOPES
                .into_iter()
                .find(|scope| scope.label() == name)
                .ok_or_else(|| anyhow!("'{name}' is not one of all, unstaged, or staged"))?,
        ),
    };
    let open = open_stream(cx.editor).map(|stream| (stream.doc, stream.scope));
    match (open, scope) {
        (Some((doc, open)), scope) if scope.is_none_or(|scope| scope == open) => {
            review::switch_to(cx.editor, doc);
            cx.editor
                .set_status(format!("Reviewing {} changes", open.label()));
        }
        (_, scope) => read_changes(cx, scope.unwrap_or(ChangeView::All), None, None),
    }
    Ok(())
}

fn complete_scope(_editor: &Editor, input: &str) -> Vec<Completion> {
    fuzzy_match(input, SCOPES.iter().map(|scope| scope.label()), false)
        .into_iter()
        .map(|(label, _)| ((0..), label.into()))
        .collect()
}

/// `:reload` in the review, and `:reload-all`: lists and reads the open
/// review's changes again.
fn reload(cx: &mut compositor::Context) {
    if let Some(scope) = open_stream(cx.editor).map(|stream| stream.scope) {
        read_changes(cx, scope, None, None);
    }
}

fn review_context(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let context = open_stream(cx.editor)
        .context("There is no review. Start one with :review")?
        .context;
    let Some(lines) = args.first() else {
        let plural = if context == 1 { "" } else { "s" };
        cx.editor.set_status(format!(
            "The review shows {context} line{plural} of context"
        ));
        return Ok(());
    };
    let lines = lines
        .parse()
        .map_err(|_| anyhow!("'{lines}' is not a number of lines"))?;
    review::set_context(cx.editor, lines);
    Ok(())
}

fn open_stream(editor: &Editor) -> Option<&Stream> {
    editor
        .features
        .review()
        .stream()
        .filter(|stream| editor.documents.contains_key(&stream.doc))
}

/// The changed-file picker's `review_jump`: the open review at `path`'s header
/// when it shows `scope`'s changes and has the file, or else a new read of
/// `scope`'s changes, then the header.
fn jump(cx: &mut compositor::Context, scope: ChangeView, path: &Path) {
    if let Some(doc) = open_stream(cx.editor)
        .filter(|stream| stream.scope == scope)
        .map(|stream| stream.doc)
    {
        review::switch_to(cx.editor, doc);
        if review::focus_file(cx.editor, path) {
            return;
        }
    }
    read_changes(cx, scope, None, Some(path.to_path_buf()));
}

/// Lists the `scope` changes of the repository the open review shows, or else
/// the one that holds the current file, or the working directory for a buffer
/// with no path. Shows them in the stream, then `done` in the status line, and
/// puts the cursor on `focus`'s header.
fn read_changes(
    cx: &mut compositor::Context,
    scope: ChangeView,
    done: Option<String>,
    focus: Option<PathBuf>,
) {
    let root = match open_stream(cx.editor) {
        Some(stream) => stream.root.clone(),
        None => doc!(cx.editor)
            .path()
            .and_then(Path::parent)
            .filter(|dir| dir.is_dir())
            .map(Path::to_path_buf)
            .unwrap_or_else(helix_stdx::env::current_working_dir),
    };
    let trust_full = cx
        .editor
        .workspace_trust
        .query(&helix_loader::find_workspace_in(&root).0, TrustQuery::Git)
        .is_trusted();
    let providers = cx.editor.diff_providers.clone();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    providers
        .clone()
        .for_each_changed_file(root.clone(), trust_full, move |change| {
            tx.send(change).is_ok()
        });
    cx.jobs.callback(async move {
        let (mut unstaged, mut staged, mut error) = (Vec::new(), Vec::new(), None);
        while let Some(change) = rx.recv().await {
            match change {
                Ok((ChangeKind::Unstaged, change)) => unstaged.push(change),
                Ok((ChangeKind::Staged, change)) => staged.push(change),
                Err(err) => {
                    error.get_or_insert(err);
                }
            }
        }
        let changes = visible_changes(scope, &unstaged, &staged);
        let files = tokio::task::spawn_blocking(move || {
            changes
                .into_iter()
                .map(|change| File::read(&providers, scope, change, trust_full))
                .collect::<Vec<_>>()
        })
        .await?;

        let show = move |editor: &mut Editor| {
            if let (Some(err), true) = (error, files.is_empty()) {
                editor.set_error(format!("Failed to list the changed files: {err}"));
                return;
            }
            if files.is_empty() {
                no_changes(editor, scope);
                return;
            }
            let sections = files.into_iter().map(|file| file.section(editor)).collect();
            review::show(editor, root, scope, sections);
            if let Some(path) = focus {
                review::focus_file(editor, &path);
            }
            editor
                .set_status(done.unwrap_or_else(|| format!("Reviewing {} changes", scope.label())));
        };
        Ok(Callback::Editor(Box::new(show)))
    });
}

/// Says there are no `scope` changes, and closes the open review if it shows
/// them. A review of other changes stays.
fn no_changes(editor: &mut Editor, scope: ChangeView) {
    let unsaved = if open_stream(editor).is_some_and(|stream| stream.scope == scope) {
        review::close_empty(editor)
    } else {
        Vec::new()
    };
    let none = match scope {
        ChangeView::All => "There are no changes to review".to_string(),
        scope => format!("There are no {} changes to review", scope.label()),
    };
    match unsaved.as_slice() {
        [] => editor.set_status(none),
        [name] => editor.set_status(format!("{none}. {name} has unsaved changes")),
        names => editor.set_status(format!("{none}. {} have unsaved changes", names.join(", "))),
    }
}

/// The bytes of both sides of a changed file, read off the main thread.
struct File {
    change: FileChange,
    base: Vec<u8>,
    source: Vec<u8>,
    /// Whether the source is the index, not the file.
    staged: bool,
}

impl File {
    /// `scope`'s sides of `change`: `HEAD` or the index against the file, or
    /// `HEAD` against the index.
    fn read(
        providers: &DiffProviderRegistry,
        scope: ChangeView,
        change: FileChange,
        trust_full: bool,
    ) -> File {
        let base_path = match &change {
            FileChange::Renamed { from_path, .. } => from_path,
            _ => change.path(),
        };
        let head = |path: &Path| {
            providers
                .get_diff_base(path, trust_full)
                .unwrap_or_default()
        };
        let index = |path: &Path| {
            index::get_index_base(path, trust_full)
                .inspect_err(|err| log::debug!("failed to read the index version: {err:#}"))
                .ok()
                .flatten()
                .unwrap_or_default()
        };
        let base = match (scope, &change) {
            // WHY: a conflicted file has no unconflicted index version.
            (ChangeView::Unstaged, FileChange::Conflict { .. }) => head(base_path),
            (ChangeView::Unstaged, _) => index(base_path),
            (ChangeView::All | ChangeView::Staged, _) => head(base_path),
        };
        let source = match (scope, &change) {
            (_, FileChange::Deleted { .. }) => Vec::new(),
            (ChangeView::Staged, _) => index(change.path()),
            _ => std::fs::read(change.path()).unwrap_or_default(),
        };
        File {
            change,
            base,
            source,
            staged: scope == ChangeView::Staged,
        }
    }

    fn section(self, editor: &Editor) -> Section {
        let doc = editor
            .document_by_path(self.change.path())
            .filter(|_| !self.staged);
        let (source, encoding) = match doc {
            Some(doc) => (doc.text().clone(), doc.encoding()),
            None => decode(&self.source, None),
        };
        let content =
            if crate::is_binary(&self.base) || (doc.is_none() && crate::is_binary(&self.source)) {
                Content::Binary
            } else {
                Content::Text {
                    base: decode(&self.base, Some(encoding)).0,
                    source,
                }
            };
        Section {
            change: self.change,
            content,
            encoding,
        }
    }
}

fn decode(
    bytes: &[u8],
    encoding: Option<&'static encoding::Encoding>,
) -> (Rope, &'static encoding::Encoding) {
    match from_reader(&mut &bytes[..], encoding) {
        Ok((text, encoding, _)) => (text, encoding),
        Err(_) => (Rope::new(), encoding.unwrap_or(encoding::UTF_8)),
    }
}

fn open(cx: &mut Context) {
    let (view, doc) = current_ref!(cx.editor);
    let Some(stream) = cx.editor.features.review().stream_of(doc.id()) else {
        not_a_review(cx.editor);
        return;
    };
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    let row = text.char_to_line(cursor);
    if let Some(Row::Gap { .. }) = stream.rows.get(row) {
        review::open_gap(cx.editor, row);
        return;
    }
    let Some(target) = stream.target(row) else {
        return;
    };
    let path = target.change.path().to_path_buf();
    if let FileChange::Deleted { .. } = target.change {
        let path = helix_stdx::path::get_relative_path(&path);
        cx.editor
            .set_error(format!("{} is deleted", path.display()));
        return;
    }
    let line = target.line;
    let column = target.same_line.then(|| cursor - text.line_to_char(row));

    cx.editor.switch_tab(Tab::CODE);
    let (view, doc) = current!(cx.editor);
    push_jump(view, doc);
    if let Err(err) = cx.editor.open(&path, Action::Replace) {
        cx.editor
            .set_error(format!("Failed to open {}: {err}", path.display()));
        return;
    }

    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let line = line.min(text.len_lines() - 1);
    let start = text.line_to_char(line);
    let pos = column.map_or(start, |column| {
        (start + column).min(line_end_char_index(&text, line))
    });
    doc.set_selection(view.id, Selection::point(pos));
    align_view(doc, view, Align::Center);
}

fn next_hunk(cx: &mut Context) {
    step_hunk(cx, Direction::Forward);
}

fn prev_hunk(cx: &mut Context) {
    step_hunk(cx, Direction::Backward);
}

fn step_hunk(cx: &mut Context, direction: Direction) {
    let count = cx.count() - 1;
    cx.editor.apply_motion(move |editor| {
        let Some(hunks) = stream_rows(editor, Stream::hunks) else {
            return;
        };
        let mode = editor.mode;
        let (view, doc) = current!(editor);
        let text = doc.text().slice(..);
        let selection = doc.selection(view.id).clone().transform(|range| {
            let row = range.cursor_line(text);
            // WHY: the rules of `]g` and `[g` in a file. The next hunk starts
            // below the cursor's row, and the previous one ends above it.
            let index = match direction {
                Direction::Forward => hunks
                    .iter()
                    .position(|hunk| hunk.start > row)
                    .map(|index| (index + count).min(hunks.len() - 1)),
                Direction::Backward => hunks
                    .iter()
                    .rposition(|hunk| hunk.end <= row)
                    .map(|index| index.saturating_sub(count)),
            };
            let Some(hunk) = index.map(|index| &hunks[index]) else {
                return range;
            };
            select_rows(text, range, hunk, mode, direction)
        });
        push_jump(view, doc);
        doc.set_selection(view.id, selection);
    });
}

fn first_hunk(cx: &mut Context) {
    edge_hunk(cx, Direction::Backward);
}

fn last_hunk(cx: &mut Context) {
    edge_hunk(cx, Direction::Forward);
}

fn edge_hunk(cx: &mut Context, end: Direction) {
    let Some(hunks) = stream_rows(cx.editor, Stream::hunks) else {
        return;
    };
    let hunk = match end {
        Direction::Forward => hunks.last(),
        Direction::Backward => hunks.first(),
    };
    let Some(hunk) = hunk else {
        return;
    };
    let mode = cx.editor.mode;
    let (view, doc) = current!(cx.editor);
    let text = doc.text().slice(..);
    let range = select_rows(
        text,
        doc.selection(view.id).primary(),
        hunk,
        mode,
        Direction::Forward,
    );
    push_jump(view, doc);
    doc.set_selection(view.id, Selection::single(range.anchor, range.head));
}

fn select_rows(
    text: RopeSlice,
    range: Range,
    rows: &std::ops::Range<usize>,
    mode: Mode,
    direction: Direction,
) -> Range {
    let rows = Range::new(text.line_to_char(rows.start), text.line_to_char(rows.end));
    if mode != Mode::Select {
        return rows.with_direction(direction);
    }
    let head = if rows.head < range.anchor {
        rows.anchor
    } else {
        rows.head
    };
    Range::new(range.anchor, head)
}

fn undo(cx: &mut Context) {
    if stream_rows(cx.editor, |_| ()).is_some() {
        review::undo(cx.editor, cx.count());
    }
}

fn redo(cx: &mut Context) {
    if stream_rows(cx.editor, |_| ()).is_some() {
        review::redo(cx.editor, cx.count());
    }
}

fn stream_rows<T>(editor: &mut Editor, rows: impl Fn(&Stream) -> T) -> Option<T> {
    let rows = editor
        .features
        .review()
        .stream_of(view!(editor).doc)
        .map(rows);
    if rows.is_none() {
        not_a_review(editor);
    }
    rows
}

fn not_a_review(editor: &mut Editor) {
    editor.set_error("This buffer isn't a review. Start one with :review");
}

#[cfg(test)]
mod tests {
    use helix_view::features::Features;
    use helix_view::review::{ReviewConfig, ReviewState};

    use crate::config::{Config, ConfigLoadError};

    fn load(global: &str, local: Option<&str>) -> Result<Config, String> {
        crate::feature::init(&[super::FEATURE]);
        let local = local
            .map(str::to_owned)
            .ok_or_else(ConfigLoadError::default);
        Config::load(Ok(&global.to_owned()), local).map_err(|err| err.to_string())
    }

    fn review(config: Config) -> ReviewConfig {
        let mut features = Features::default();
        config.apply_features(&mut features).unwrap();
        features.review().config()
    }

    #[test]
    fn review_table_reads_its_default_an_override_and_both_files() {
        let both = |row_background, word_background| ReviewConfig {
            row_background,
            word_background,
        };
        assert_eq!(review(load("", None).unwrap()), both(true, true));
        let global = "[review]\nrow-background = false";
        assert_eq!(review(load(global, None).unwrap()), both(false, true));
        let local = "[review]\nword-background = false";
        assert_eq!(
            review(load(global, Some(local)).unwrap()),
            both(false, false),
            "the workspace's table merges over the global one"
        );
    }

    #[test]
    fn review_table_refuses_an_unknown_key_and_the_old_place() {
        let in_review = load("[review]\nrow-backgrounds = false", None).unwrap_err();
        assert!(
            in_review.contains("unknown field `row-backgrounds`"),
            "{in_review}"
        );
        let in_editor = load("[editor.review]\nrow-background = false", None).unwrap_err();
        assert!(in_editor.contains("unknown field `review`"), "{in_editor}");
    }

    #[test]
    fn keys_bind_the_reviews_commands_by_name() {
        use std::str::FromStr;

        use helix_view::document::Mode;
        use helix_view::input::KeyEvent;

        use crate::keymap::KeyTrie;

        let config = load(
            "[keys.normal]\nC-r = \"review_open\"\nC-t = \":review staged\"",
            None,
        )
        .unwrap();
        let name = |key: &str| {
            let key = KeyEvent::from_str(key).unwrap();
            match config.keys[&Mode::Normal].search(&[key]) {
                Some(KeyTrie::MappableCommand(command)) => command.name().to_owned(),
                _ => panic!("{key} is bound to a command"),
            }
        };
        assert_eq!(name("C-r"), "review_open");
        assert_eq!(name("C-t"), "review");
    }

    #[test]
    fn picker_keys_bind_review_jump_by_default_by_name_and_unbind_it() {
        use std::str::FromStr;

        use helix_view::editor::PickerCommand;
        use helix_view::input::KeyEvent;

        let key = |name: &str| KeyEvent::from_str(name).unwrap();
        let jump = Some(&PickerCommand::Action("review_jump"));
        let loaded = load("", None).unwrap();
        assert_eq!(loaded.editor.picker_keys.get(&key("A-r")), jump);
        assert_eq!(
            Config::default().editor.picker_keys.get(&key("A-r")),
            jump,
            "with no config file too"
        );
        let config = load(
            "[keys.picker]\nC-j = \"review_jump\"\nA-r = \"no_op\"",
            None,
        )
        .unwrap();
        assert_eq!(config.editor.picker_keys.get(&key("C-j")), jump);
        assert_eq!(config.editor.picker_keys.get(&key("A-r")), None);
    }
}
