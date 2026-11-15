//! The review stream: one scratch buffer that shows the hunks of every changed
//! file, one row per line. An edit to an added or unchanged line goes to the
//! file's buffer, and anything else is put back.
//!
//! `helix-term` lists the changed files and reads both sides of each. This
//! module lays out the rows, owns the buffer's text, and forwards its edits.

use std::collections::HashMap;
use std::num::NonZeroU16;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use helix_core::encoding::Encoding;
use helix_core::indent::{auto_detect_indent_style, IndentStyle};
use helix_core::line_ending::{auto_detect_line_ending, get_line_ending, LineEnding};
use helix_core::movement::Direction;
use helix_core::syntax::Loader;
use helix_core::{Assoc, ChangeSet, Language, Transaction};
use helix_core::{Rope, RopeSlice, Selection};
use helix_lsp::{lsp, OffsetEncoding};
use helix_vcs::commit::Commit;
use helix_vcs::{ChangeView, FileChange, Hunk};
use serde::de::Unexpected;
use serde::{Deserialize, Deserializer};

use helix_view::document::Mode;
use helix_view::editor::{Action, GutterType, TabOptions};
use helix_view::features::Features;
use helix_view::gutter::GutterFn;
use helix_view::kind::BufferKind;
use helix_view::tree::Tab;
use helix_view::{align_view, Align, Document, DocumentId, Editor, Theme, ViewId};
use helix_view::{current, view};
use reconcile::Key;

mod complete;
mod diagnostics;
pub mod files;
mod forward;
mod gutter;
pub mod layout;
mod log;
mod reconcile;
mod source;
mod sync;
mod syntax;
mod words;

pub use log::{log_step, redo, undo};
pub use source::source;
pub use sync::{load, sync};
pub use syntax::overlays as syntax_overlays;

/// The tab the review opens in.
pub const REVIEW_TAB: Tab = Tab("review");

/// The name of the review's buffer kind.
pub const KIND_NAME: &str = "review";

/// The review's buffer kind, on the stream's buffer.
struct Kind;

static KIND: Kind = Kind;

impl BufferKind for Kind {
    fn name(&self) -> &'static str {
        KIND_NAME
    }

    fn source(&self, editor: &mut Editor, view: ViewId) -> Result<DocumentId, String> {
        source(editor, view)
    }

    fn source_open<'e>(&self, editor: &'e Editor, view: ViewId) -> Option<&'e Document> {
        complete::file_at_cursor(editor, view)
    }

    // WHY: the sync would forward the preview to the file, and its server
    // would then resolve items against text that holds it.
    fn previews_completion(&self) -> bool {
        false
    }

    fn accept_completion(
        &self,
        editor: &mut Editor,
        view: ViewId,
        item: &mut lsp::CompletionItem,
        encoding: OffsetEncoding,
    ) {
        let doc = &editor.documents[&editor.tree.get(view).doc];
        complete::accept_item(editor.features.review_mut(), doc, view, item, encoding);
    }

    // WHY: the review's buffer is never modified. Its files are.
    fn close(
        &self,
        editor: &mut Editor,
        _doc: DocumentId,
        force: bool,
    ) -> Result<(), Vec<(DocumentId, String)>> {
        close(editor, force)
    }

    fn goto_buffer(&self, editor: &mut Editor, direction: Direction, count: usize) -> bool {
        step_rows(editor, direction, count, |review, doc| {
            Some(review.stream_of(doc)?.headers().collect())
        });
        true
    }

    fn gutter<'d>(
        &self,
        editor: &'d Editor,
        doc: &'d Document,
        theme: &Theme,
        gutter: GutterType,
        focused: bool,
    ) -> Option<GutterFn<'d>> {
        match gutter {
            GutterType::LineNumbers => gutter::line_numbers(editor, doc, theme, focused),
            GutterType::Diff => gutter::signs(editor, doc, theme),
            GutterType::Diagnostics => gutter::diagnostics(editor, doc, theme),
            _ => None,
        }
    }
}

/// Moves each cursor of the focused view `count` on through its buffer's
/// `stops`, which are rows in order.
fn step_rows(
    editor: &mut Editor,
    direction: Direction,
    count: usize,
    stops: fn(&Review, DocumentId) -> Option<Vec<usize>>,
) {
    let count = count - 1;
    editor.apply_motion(move |editor| {
        let view = editor.tree.get(editor.tree.focus);
        let Some(stops) = stops(editor.features.review(), view.doc) else {
            return;
        };
        let extend = editor.mode == Mode::Select;
        let (view, doc) = current!(editor);
        let text = doc.text().slice(..);
        let selection = doc.selection(view.id).clone().transform(|range| {
            let row = range.cursor_line(text);
            let beyond: Vec<_> = match direction {
                Direction::Forward => stops.iter().filter(|&&stop| stop > row).collect(),
                Direction::Backward => stops.iter().rev().filter(|&&stop| stop < row).collect(),
            };
            let Some(&stop) = beyond.get(count).or(beyond.last()).copied() else {
                return range;
            };
            range.put_cursor(text, text.line_to_char(stop), extend)
        });
        doc.append_changes_to_history(view);
        let jump = (doc.id(), doc.selection(view.id).clone());
        view.push_jump(doc, jump);
        doc.set_selection(view.id, selection);
    });
}

/// What a line of the stream shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The path of `files[file]` and how it changed.
    Header { file: usize },
    /// Unchanged source lines `start..end` that aren't shown.
    Gap {
        file: usize,
        start: usize,
        end: usize,
    },
    /// A line of the base that the source no longer has.
    Removed { file: usize, base: usize },
    /// A line both sides have.
    Context {
        file: usize,
        base: usize,
        source: usize,
    },
    /// A line of the source that the base doesn't have.
    Added { file: usize, source: usize },
    /// A line of the commit's message, above the first file.
    Message { line: usize },
}

impl Row {
    /// Its file. None for a row of the commit's message.
    pub fn file(self) -> Option<usize> {
        match self {
            Row::Header { file }
            | Row::Gap { file, .. }
            | Row::Removed { file, .. }
            | Row::Context { file, .. }
            | Row::Added { file, .. } => Some(file),
            Row::Message { .. } => None,
        }
    }

    fn key(self) -> (Option<usize>, Key) {
        let key = match self {
            Row::Header { .. } => Key::Header,
            Row::Gap { start, .. } => Key::Gap(start),
            Row::Removed { base, .. } => Key::Removed(base),
            Row::Context { source, .. } | Row::Added { source, .. } => Key::Source(source),
            Row::Message { line } => Key::Message(line),
        };
        (self.file(), key)
    }

    pub fn lines(self) -> (Option<usize>, Option<usize>) {
        match self {
            Row::Removed { base, .. } => (Some(base), None),
            Row::Context { base, source, .. } => (Some(base), Some(source)),
            Row::Added { source, .. } => (None, Some(source)),
            Row::Header { .. } | Row::Gap { .. } | Row::Message { .. } => (None, None),
        }
    }
}

/// A changed file, with both sides of it.
pub struct Section {
    pub change: FileChange,
    pub content: Content,
    /// The source's encoding, which staging writes the index in.
    pub encoding: &'static Encoding,
}

#[derive(Clone)]
pub enum Content {
    /// `base` is empty for a new file, and `source` for a deleted one.
    Text {
        base: Rope,
        source: Rope,
    },
    Binary,
}

/// The `Editor` field. Holds the stream, when there is one.
#[derive(Default)]
pub struct Review {
    stream: Option<Stream>,
    /// Whether a view may still have the stream's line number width.
    widened: bool,
    config: ReviewConfig,
}

/// `[review]`: which of the theme's diff styles the review draws, and its
/// file list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct ReviewConfig {
    /// Tint added and removed rows. Defaults to `true`.
    pub row_background: bool,
    /// Mark changed words. Defaults to `true`.
    pub word_background: bool,
    /// When the file list shows. Defaults to `"auto"`.
    pub sidebar: Sidebar,
    /// The file list's width in columns. Defaults to 30.
    pub sidebar_width: NonZeroU16,
}

impl ReviewConfig {
    const DEFAULT: ReviewConfig = ReviewConfig {
        row_background: true,
        word_background: true,
        sidebar: Sidebar::Auto,
        sidebar_width: NonZeroU16::new(30).unwrap(),
    };
}

/// When the review shows its file list: `true`, `false`, or `"auto"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sidebar {
    Shown,
    Hidden,
    /// While the review's tab is wide enough for both.
    Auto,
}

impl<'de> Deserialize<'de> for Sidebar {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = Sidebar;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("true, false, or \"auto\"")
            }

            fn visit_bool<E>(self, shown: bool) -> Result<Sidebar, E> {
                Ok(if shown {
                    Sidebar::Shown
                } else {
                    Sidebar::Hidden
                })
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Sidebar, E> {
                match value {
                    "auto" => Ok(Sidebar::Auto),
                    other => Err(E::invalid_value(Unexpected::Str(other), &self)),
                }
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// WHY: what the review reads before anything has stored its state.
static NO_REVIEW: Review = Review {
    stream: None,
    widened: false,
    config: ReviewConfig::DEFAULT,
};

/// The review's state in the editor's features.
pub trait ReviewState {
    fn review(&self) -> &Review;
    fn review_mut(&mut self) -> &mut Review;
}

impl ReviewState for Features {
    fn review(&self) -> &Review {
        self.get().unwrap_or(&NO_REVIEW)
    }

    fn review_mut(&mut self) -> &mut Review {
        self.get_or_default()
    }
}

impl Review {
    pub fn stream(&self) -> Option<&Stream> {
        self.stream.as_ref()
    }

    pub fn config(&self) -> ReviewConfig {
        self.config
    }

    pub fn set_config(&mut self, config: ReviewConfig) {
        self.config = config;
    }

    pub fn stream_of(&self, doc: DocumentId) -> Option<&Stream> {
        self.stream.as_ref().filter(|stream| stream.doc == doc)
    }
}

/// What a review shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The working tree's changes, the index's, or both.
    Changes(ChangeView),
    /// A commit's changes against its first parent.
    Commit(Arc<Commit>),
}

impl Scope {
    /// Whether its sources are the files. Such a review follows the files'
    /// buffers and edits them. Any other is read-only: a staged review's
    /// sources are the index, and a commit's the commit.
    pub fn follows_files(&self) -> bool {
        matches!(self, Scope::Changes(ChangeView::All | ChangeView::Unstaged))
    }

    /// What a review that doesn't follow the files says to an edit.
    pub fn read_only(&self) -> &'static str {
        match self {
            Scope::Commit(_) => "A commit's review is read-only. ret opens the file.",
            Scope::Changes(_) => "A staged review is read-only. ret opens the file.",
        }
    }

    /// What such a review says to a command that asks about a file or
    /// changes one.
    pub fn not_the_files(&self) -> &'static str {
        match self {
            Scope::Commit(_) => "A commit's review shows the commit, not the files.",
            Scope::Changes(_) => {
                "A staged review shows the index, not the files. ret opens the file."
            }
        }
    }
}

pub struct Stream {
    pub doc: DocumentId,
    /// The directory the changes were listed from.
    pub root: PathBuf,
    pub scope: Scope,
    /// Unchanged lines shown around each hunk.
    pub context: usize,
    files: Vec<File>,
    /// One per line of the buffer.
    pub rows: Vec<Row>,
    /// What the buffer has to hold.
    text: Rope,
    /// The buffer's version when it last held `text`.
    version: i32,
    /// Each view's selection when the buffer last held `text`.
    selections: HashMap<ViewId, Selection>,
    /// Digits of the widest base and source line numbers.
    widths: [usize; 2],
    pub ranges: Ranges,
    /// The buffer's own indent, for a file that shows none.
    indent: IndentStyle,
    /// Files' buffers with forwarded changes not yet in their history, and the
    /// view each was made from.
    pending: Vec<(DocumentId, ViewId)>,
    /// Files' buffers the review opened in the background.
    loaded: Vec<DocumentId>,
    /// Whether an edit was forwarded since the rows were laid out.
    relayout: bool,
    log: log::Log,
    reports: diagnostics::Reports,
    /// An accepted completion's other edits, until the sync forwards it.
    deferred: Option<complete::Deferred>,
    list: files::List,
}

/// The rows of each hunk: a run of removed and added rows.
fn hunks(rows: &[Row]) -> Vec<Range<usize>> {
    let mut hunks: Vec<Range<usize>> = Vec::new();
    for (row, kind) in rows.iter().enumerate() {
        if !matches!(kind, Row::Removed { .. } | Row::Added { .. }) {
            continue;
        }
        match hunks.last_mut() {
            Some(hunk) if hunk.end == row => hunk.end += 1,
            _ => hunks.push(row..row + 1),
        }
    }
    hunks
}

/// Char ranges of the stream's text, by how they're drawn. Each list is in
/// order, and leaves out line endings.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Ranges {
    pub removed: Vec<Range<usize>>,
    pub headers: Vec<Range<usize>>,
    pub gaps: Vec<Range<usize>>,
    /// The words that changed, on both sides of a hunk with removed and added
    /// rows.
    pub words: Vec<Range<usize>>,
}

impl Ranges {
    fn new(rows: &[Row], hunks: &[Range<usize>], text: &Rope) -> Ranges {
        let mut ranges = Ranges::default();
        for (row, kind) in rows.iter().enumerate() {
            let list = match kind {
                Row::Removed { .. } => &mut ranges.removed,
                Row::Header { .. } => &mut ranges.headers,
                Row::Gap { .. } => &mut ranges.gaps,
                Row::Context { .. } | Row::Added { .. } | Row::Message { .. } => continue,
            };
            let line = text.line(row);
            let ending = get_line_ending(&line).map_or(0, |ending| ending.len_chars());
            let start = text.line_to_char(row);
            list.push(start..start + line.len_chars() - ending);
        }
        let text = text.slice(..);
        for hunk in hunks {
            let added = hunk.start
                + rows[hunk.clone()]
                    .iter()
                    .take_while(|row| matches!(row, Row::Removed { .. }))
                    .count();
            if let Some(words) =
                words::changed_words(text, hunk.start..added, text, added..hunk.end)
            {
                ranges.words.extend(words.base);
                ranges.words.extend(words.doc);
            }
        }
        ranges
    }

    fn map(&mut self, changes: &ChangeSet) {
        for list in [
            &mut self.removed,
            &mut self.headers,
            &mut self.gaps,
            &mut self.words,
        ] {
            changes.update_positions(list.iter_mut().flat_map(|range| {
                [
                    (&mut range.start, Assoc::After),
                    (&mut range.end, Assoc::Before),
                ]
            }));
        }
    }
}

/// A file of the stream, and what its rows came from.
#[derive(Clone)]
struct File {
    change: FileChange,
    content: Content,
    encoding: &'static Encoding,
    /// The open buffer the source came from, and its version then.
    seen: Option<(DocumentId, i32)>,
    /// Source lines whose gap was opened.
    opened: Vec<Range<usize>>,
    language: Option<Language>,
    trees: syntax::Trees,
    /// The hunks of `content`, once laid out. `None` inside when it's too
    /// large to diff.
    hunks: OnceLock<Option<Vec<Hunk>>>,
    /// The indent the source shows, or else its language's, once looked for.
    indent: OnceLock<Option<IndentStyle>>,
}

impl File {
    // WHY: headers, gaps, and a last line with no line ending show the file's
    // own line ending, so `O` below them inserts the same one as the stream.
    fn ending(&self) -> LineEnding {
        let Content::Text { base, source } = &self.content else {
            return LineEnding::LF;
        };
        auto_detect_line_ending(source)
            .or_else(|| auto_detect_line_ending(base))
            .unwrap_or(LineEnding::LF)
    }

    /// The lines its hunks add and remove. `None` for a binary file, or one
    /// too large to diff.
    fn counts(&self) -> Option<(usize, usize)> {
        let Content::Text { base, source } = &self.content else {
            return None;
        };
        let hunks = self.hunks.get_or_init(|| layout::hunks(base, source));
        let counts = hunks
            .as_ref()?
            .iter()
            .fold((0, 0), |(added, removed), hunk| {
                (added + hunk.after.len(), removed + hunk.before.len())
            });
        Some(counts)
    }

    fn indent(&self, loader: &Loader) -> Option<IndentStyle> {
        *self.indent.get_or_init(|| {
            let Content::Text { source, .. } = &self.content else {
                return None;
            };
            auto_detect_indent_style(source).or_else(|| {
                let config = loader.language(self.language?).config();
                let unit = &config.indent.as_ref()?.unit;
                Some(IndentStyle::from_str(unit))
            })
        })
    }
}

impl From<Section> for File {
    fn from(section: Section) -> File {
        File {
            change: section.change,
            content: section.content,
            encoding: section.encoding,
            seen: None,
            opened: Vec::new(),
            language: None,
            trees: syntax::Trees::default(),
            hunks: OnceLock::new(),
            indent: OnceLock::new(),
        }
    }
}

/// The hunks a selection touches in one file.
pub struct Picked<'a> {
    pub change: &'a FileChange,
    pub base: &'a Rope,
    pub source: &'a Rope,
    pub encoding: &'static Encoding,
    /// Each hunk's source lines, first and last. A removal stands on the line
    /// below it.
    pub lines: Vec<(usize, usize)>,
    /// The base line of each of the file's removed rows, and whether the
    /// selection covers the row.
    pub removed: Vec<(usize, bool)>,
    /// The same for its added rows, by source line.
    pub added: Vec<(usize, bool)>,
}

/// Where `ret` on a row opens its file.
pub struct Target<'a> {
    pub change: &'a FileChange,
    /// The zero-based line of the source.
    pub line: usize,
    /// Whether the row shows that line, so the cursor's column carries over.
    pub same_line: bool,
}

impl Stream {
    pub fn hunks(&self) -> Vec<Range<usize>> {
        hunks(&self.rows)
    }

    /// The list of its files.
    pub fn list(&self) -> &files::List {
        &self.list
    }

    /// How each file changed, with the lines its hunks add and remove. A
    /// binary file, or one too large to diff, has no counts.
    pub fn changes(&self) -> impl Iterator<Item = (&FileChange, Option<(usize, usize)>)> + '_ {
        self.files.iter().map(|file| (&file.change, file.counts()))
    }

    /// The hunks that `rows`, pairs of first and last rows, touch, by file.
    pub fn picked(&self, rows: &[(usize, usize)]) -> Vec<Picked<'_>> {
        let mut picked: Vec<Picked> = Vec::new();
        for hunk in self.hunks() {
            if !rows
                .iter()
                .any(|&(first, last)| hunk.start <= last && first < hunk.end)
            {
                continue;
            }
            let Some(file) = self.rows[hunk.start].file() else {
                continue;
            };
            let Content::Text { base, source } = &self.files[file].content else {
                continue;
            };
            let added: Vec<usize> = self.rows[hunk.clone()]
                .iter()
                .filter_map(|row| row.lines().1)
                .collect();
            let lines = match (added.first(), added.last()) {
                (Some(&first), Some(&last)) => (first, last),
                _ => {
                    let below = self.line_below(hunk.start);
                    (below, below)
                }
            };
            let change = &self.files[file].change;
            match picked.last_mut() {
                Some(last) if std::ptr::eq(last.change, change) => last.lines.push(lines),
                _ => {
                    let mut new = Picked {
                        change,
                        base,
                        source,
                        encoding: self.files[file].encoding,
                        lines: vec![lines],
                        removed: Vec::new(),
                        added: Vec::new(),
                    };
                    for (row, kind) in self.rows.iter().enumerate() {
                        let covered = || {
                            rows.iter()
                                .any(|&(first, last)| first <= row && row <= last)
                        };
                        match *kind {
                            Row::Removed { file: of, base } if of == file => {
                                new.removed.push((base, covered()))
                            }
                            Row::Added { file: of, source } if of == file => {
                                new.added.push((source, covered()))
                            }
                            _ => {}
                        }
                    }
                    picked.push(new);
                }
            }
        }
        picked
    }

    pub fn headers(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, Row::Header { .. }))
            .map(|(row, _)| row)
    }

    /// The paths of the files it shows.
    pub fn paths(&self) -> impl Iterator<Item = &Path> + '_ {
        self.files.iter().map(|file| file.change.path())
    }

    pub fn gutter_width(&self) -> usize {
        self.widths[0] + 1 + self.widths[1]
    }

    pub fn target(&self, row: usize) -> Option<Target<'_>> {
        let (line, same_line) = match *self.rows.get(row)? {
            Row::Context { source, .. } | Row::Added { source, .. } => (source, true),
            Row::Gap { .. } | Row::Message { .. } => return None,
            Row::Removed { .. } => (self.line_below(row), false),
            Row::Header { file } => {
                let change = self.rows[row + 1..]
                    .iter()
                    .take_while(|next| next.file() == Some(file))
                    .position(|next| matches!(next, Row::Removed { .. } | Row::Added { .. }));
                let line =
                    match change.map(|offset| (row + 1 + offset, self.rows[row + 1 + offset])) {
                        Some((_, Row::Added { source, .. })) => source,
                        Some((change, _)) => self.line_below(change),
                        None => 0,
                    };
                (line, false)
            }
        };
        Some(Target {
            change: &self.files[self.rows[row].file()?].change,
            line,
            same_line,
        })
    }

    fn line_below(&self, row: usize) -> usize {
        let file = self.rows[row].file();
        let same_file = |other: &&Row| other.file() == file;
        let next = self.rows[row + 1..]
            .iter()
            .take_while(same_file)
            .find_map(|next| next.lines().1);
        let previous = || {
            self.rows[..row]
                .iter()
                .rev()
                .take_while(same_file)
                .find_map(|previous| previous.lines().1)
                .map(|line| line + 1)
        };
        next.or_else(previous).unwrap_or(0)
    }
}

struct Layout {
    rows: Vec<Row>,
    text: String,
    widths: [usize; 2],
}

/// The rows above the first file of a commit's review, as `git show` heads
/// its output: the hash, who wrote it and when, then the message.
fn message(scope: &Scope) -> Vec<String> {
    let Scope::Commit(commit) = scope else {
        return Vec::new();
    };
    let mut lines = vec![format!("commit {}", commit.id)];
    if commit.parents.len() > 1 {
        lines.push(format!("Merge: {}", commit.parents.join(" ")));
    }
    lines.push(format!("Author: {} <{}>", commit.author, commit.email));
    lines.push(format!("Date:   {}", commit.date));
    lines.push(String::new());
    lines.extend(commit.message.lines().map(|line| match line.trim_end() {
        "" => String::new(),
        line => format!("    {line}"),
    }));
    lines.push(String::new());
    lines
}

fn layout(message: &[String], files: &[File], context: usize) -> Layout {
    let mut rows = Vec::new();
    let mut text = String::new();
    for (line, shown) in message.iter().enumerate() {
        text.push_str(shown);
        text.push('\n');
        rows.push(Row::Message { line });
    }
    for (index, file) in files.iter().enumerate() {
        let (body, note) = match &file.content {
            Content::Binary => (Vec::new(), Some("binary")),
            Content::Text { base, source } => {
                match file.hunks.get_or_init(|| layout::hunks(base, source)) {
                    None => (Vec::new(), Some("too large to diff")),
                    Some(hunks) if hunks.is_empty() => (Vec::new(), Some("no text changes")),
                    Some(hunks) => {
                        let lines = layout::line_count(source);
                        let rows = layout::rows(index, hunks, lines, context, &file.opened);
                        (rows, None)
                    }
                }
            }
        };

        let ending = file.ending().as_str();
        text.push_str(&header(&file.change, note));
        text.push_str(ending);
        rows.push(Row::Header { file: index });
        for row in &body {
            match (*row, &file.content) {
                (Row::Gap { start, end, .. }, _) => {
                    let lines = end - start;
                    let plural = if lines == 1 { "" } else { "s" };
                    text.push_str(&format!("⋯ {lines} unchanged line{plural}{ending}"));
                }
                (Row::Removed { base, .. }, Content::Text { base: rope, .. }) => {
                    push_line(&mut text, rope.line(base), ending)
                }
                (
                    Row::Context { source, .. } | Row::Added { source, .. },
                    Content::Text { source: rope, .. },
                ) => push_line(&mut text, rope.line(source), ending),
                _ => unreachable!("only a text file has rows below its header"),
            }
        }
        rows.extend(body);
    }

    let digits = |line: Option<usize>| line.map_or(1, |line| (line + 1).to_string().len());
    let widths = [
        digits(rows.iter().filter_map(|row| row.lines().0).max()),
        digits(rows.iter().filter_map(|row| row.lines().1).max()),
    ];
    Layout { rows, text, widths }
}

fn push_line(text: &mut String, line: RopeSlice, ending: &str) {
    // INVARIANT: a row keeps its line's own line ending, so the row's chars
    // match the line's one to one. `ret` carries the column over on that.
    text.extend(line.chunks());
    if get_line_ending(&line).is_none() {
        text.push_str(ending);
    }
}

fn header(change: &FileChange, note: Option<&str>) -> String {
    let show = |path: &Path| {
        helix_stdx::path::get_relative_path(path)
            .display()
            .to_string()
    };
    let status = match change {
        FileChange::Modified { .. } => "modified".to_string(),
        FileChange::Untracked { .. } | FileChange::Added { .. } => "new file".to_string(),
        FileChange::Deleted { .. } => "deleted".to_string(),
        FileChange::Conflict { .. } => "conflict".to_string(),
        FileChange::Renamed { from_path, .. } => format!("renamed from {}", show(from_path)),
    };
    match note {
        Some(note) => format!("{}  {status}, {note}", show(change.path())),
        None => format!("{}  {status}", show(change.path())),
    }
}

pub fn show(editor: &mut Editor, root: PathBuf, scope: Scope, sections: Vec<Section>) {
    let read_only = !scope.follows_files();
    let open = editor
        .features
        .review()
        .stream
        .as_ref()
        .map(|stream| stream.doc)
        .filter(|doc| editor.documents.contains_key(doc));
    if let Some(doc) = open {
        // WHY: a read of the changes it already shows leaves you in your tab,
        // for `:reload-all` from the code tab. The changed-file picker's
        // `review_jump` switches before it reads them.
        let shown = editor.features.review().stream.as_ref();
        let changed = shown.is_some_and(|stream| stream.scope != scope);
        // WHY: a commit's files aren't the working tree's, even at the same
        // paths, so no row of one stands for a row of the other.
        let replaced = changed
            && shown.is_some_and(|stream| {
                matches!(stream.scope, Scope::Commit(_)) || matches!(scope, Scope::Commit(_))
            });
        if changed {
            switch_to(editor, doc);
        }
        if let Some(stream) = editor.features.review_mut().stream.as_mut() {
            stream.root = root;
            stream.scope = scope;
        }
        if let Some(doc) = editor.documents.get_mut(&doc) {
            doc.readonly = read_only;
        }
        // INVARIANT: after the scope is set, so the emptied stream holds no
        // row of the message it showed.
        if replaced {
            start_over(editor);
        }
        update(editor, sections.into_iter().map(File::from).collect(), None);
        if replaced {
            to_top(editor, doc);
        }
        return;
    }

    let doc_id = editor.new_file(Action::Load);
    switch_to(editor, doc_id);
    let list = files::List::open(editor);
    let doc = editor.documents.get_mut(&doc_id).expect("just opened");
    // WHY: upstream only uses the flag for writes. Here it shows in the
    // statusline that a staged review takes no edits.
    doc.readonly = read_only;
    doc.listed = false;
    doc.name_without_path = "[review]";
    doc.kind = Some(&KIND);
    editor.features.review_mut().stream = Some(Stream {
        doc: doc_id,
        root,
        scope,
        context: layout::CONTEXT,
        files: Vec::new(),
        rows: Vec::new(),
        text: doc.text().clone(),
        version: doc.version(),
        selections: HashMap::new(),
        widths: [1, 1],
        ranges: Ranges::default(),
        indent: doc.indent_style,
        pending: Vec::new(),
        loaded: Vec::new(),
        relayout: false,
        log: log::Log::default(),
        reports: diagnostics::Reports::new(),
        deferred: None,
        list,
    });
    update(editor, sections.into_iter().map(File::from).collect(), None);
    to_top(editor, doc_id);
}

/// Switches to the review tab, opening it on the stream `doc` if it isn't open,
/// and focuses a view of it that shows the stream. With none, the tab's
/// focused view shows it.
pub fn switch_to(editor: &mut Editor, doc: DocumentId) {
    if !editor.switch_tab(REVIEW_TAB) {
        let options = TabOptions {
            keeps: &[KIND_NAME, files::KIND_NAME],
        };
        editor.open_tab(REVIEW_TAB, doc, options);
    } else if let Some(&view) = files::views_of(editor, doc).first() {
        editor.focus(view);
    } else {
        editor.switch(doc, Action::Replace);
    }
}

/// The buffers of the files it shows that have unsaved changes. A staged review
/// shows the index, not the buffers, so none.
pub fn unsaved(editor: &Editor) -> Vec<DocumentId> {
    let Some(stream) = editor.features.review().stream.as_ref() else {
        return Vec::new();
    };
    if !stream.scope.follows_files() {
        return Vec::new();
    }
    stream
        .paths()
        .filter_map(|path| editor.document_by_path(path))
        .filter(|doc| doc.is_modified())
        .map(|doc| doc.id())
        .collect()
}

/// Closes the review and the files' buffers it loaded. Refused with the
/// `unsaved` files and their names, unless `force`, which drops the changes of
/// those it loaded. A file you opened keeps its own.
pub fn close(editor: &mut Editor, force: bool) -> Result<(), Vec<(DocumentId, String)>> {
    if !force {
        let unsaved = unsaved_names(editor);
        if !unsaved.is_empty() {
            return Err(unsaved);
        }
    }
    shut(editor, force);
    Ok(())
}

/// Closes the review once a read of its changes finds none. The files it
/// showed with unsaved changes keep their buffers, and come back by name.
pub fn close_empty(editor: &mut Editor) -> Vec<String> {
    let unsaved = unsaved_names(editor)
        .into_iter()
        .map(|(_, name)| name)
        .collect();
    shut(editor, false);
    unsaved
}

pub fn unsaved_names(editor: &Editor) -> Vec<(DocumentId, String)> {
    unsaved(editor)
        .into_iter()
        .map(|doc| (doc, editor.documents[&doc].display_name().into_owned()))
        .collect()
}

fn shut(editor: &mut Editor, force: bool) {
    let Some(doc) = editor
        .features
        .review()
        .stream
        .as_ref()
        .map(|stream| stream.doc)
    else {
        return;
    };
    // WHY: closing the buffer closes only the views that show it, so a split
    // you opened on a file would keep the tab open after the review.
    editor.close_tab(REVIEW_TAB);
    let _ = editor.close_document(doc, force);
    if let Some(stream) = editor.features.review_mut().stream.take() {
        sync::close_owned(editor, stream, force);
    }
}

/// Applies `transaction` to a file's buffer from the review's `view`. The next
/// sync outside insert mode commits it, as part of one step of the review's
/// undo log.
pub fn edit_file(editor: &mut Editor, doc: DocumentId, view: ViewId, transaction: &Transaction) {
    let Some(file) = editor.documents.get_mut(&doc) else {
        return;
    };
    file.ensure_view_init(view);
    // WHY: `u` puts back the selection this view had in the file before the
    // first change. Left where `Action::Load` put it, that's the file's start.
    if let Some((from, ..)) = transaction.changes_iter().next() {
        file.set_selection(view, Selection::point(from));
    }
    file.apply(transaction, view);
    if let Some(stream) = editor.features.review_mut().stream.as_mut() {
        if !stream.pending.iter().any(|&(pending, _)| pending == doc) {
            stream.pending.push((doc, view));
        }
    }
}

/// Empties the stream for the rows of another scope, and lets go of the files
/// as a close does: the buffers it loaded close, and its undo log empties.
fn start_over(editor: &mut Editor) {
    update(editor, Vec::new(), None);
    let Some(stream) = editor.features.review_mut().stream.as_mut() else {
        return;
    };
    stream.pending.clear();
    stream.log = log::Log::default();
    stream.deferred = None;
    let loaded = std::mem::take(&mut stream.loaded);
    sync::close_loaded(editor, loaded, false);
}

fn to_top(editor: &mut Editor, doc: DocumentId) {
    let view = editor.tree.get(editor.tree.focus);
    let doc = editor
        .documents
        .get_mut(&doc)
        .expect("the stream's buffer is open");
    doc.set_selection(view.id, Selection::point(0));
    align_view(doc, view, Align::Top);
}

/// Puts the focused view's cursor on the header of `path` in the review it
/// shows. Returns whether the review has the file.
pub fn focus_file(editor: &mut Editor, path: &Path) -> bool {
    let Some(stream) = editor.features.review().stream_of(view!(editor).doc) else {
        return false;
    };
    let header = stream.rows.iter().position(|row| match row {
        Row::Header { file } => stream.files[*file].change.path() == path,
        _ => false,
    });
    let Some(row) = header else {
        return false;
    };
    let (view, doc) = current!(editor);
    let start = doc.text().line_to_char(row);
    doc.set_selection(view.id, Selection::point(start));
    align_view(doc, view, Align::Top);
    true
}

pub fn set_context(editor: &mut Editor, lines: usize) {
    let Some(stream) = editor.features.review_mut().stream.as_mut() else {
        return;
    };
    stream.context = lines;
    let files = stream.files.clone();
    update(editor, files, None);
}

pub fn open_gap(editor: &mut Editor, row: usize) -> bool {
    let Some(stream) = editor.features.review().stream_of(view!(editor).doc) else {
        return false;
    };
    let Some(&Row::Gap { file, start, end }) = stream.rows.get(row) else {
        return false;
    };
    let files = stream.files.clone();
    update(editor, files, Some((file, start..end)));
    // WHY: a selection on the gap row, as a search leaves, maps over every
    // line it shows, so a following `d` would delete them all.
    let first = editor.features.review().stream.as_ref().and_then(|stream| {
        stream.rows.iter().position(|row| {
            matches!(*row, Row::Context { file: f, source, .. } if f == file && source == start)
        })
    });
    if let Some(first) = first {
        let (view, doc) = current!(editor);
        let pos = doc.text().line_to_char(first);
        doc.set_selection(view.id, Selection::point(pos));
    }
    true
}

// INVARIANT: `open` is a range of `files`' source lines, not the stream's.
fn update(editor: &mut Editor, mut files: Vec<File>, open: Option<(usize, Range<usize>)>) {
    let Some(doc_id) = editor
        .features
        .review()
        .stream
        .as_ref()
        .map(|stream| stream.doc)
    else {
        return;
    };
    // INVARIANT: the transaction below is built on the stream's text, so the
    // buffer has to hold it.
    if sync::put_back(editor, doc_id) {
        editor.set_status("The edit was put back");
    }
    // INVARIANT: the buffer's history is committed through a view of it.
    let Some(view) = sync::view_of(editor, doc_id) else {
        return;
    };
    let loader = editor.syn_loader.load();
    let follows_files = editor
        .features
        .review()
        .stream
        .as_ref()
        .is_some_and(|stream| stream.scope.follows_files());
    for file in &mut files {
        file.seen = editor
            .document_by_path(file.change.path())
            .filter(|_| follows_files)
            .map(|doc| (doc.id(), doc.version()));
        if let Content::Text { source, .. } = &file.content {
            file.language = loader
                .language_for_filename(file.change.path())
                .or_else(|| loader.language_for_shebang(source.slice(..)));
        }
    }
    drop(loader);

    let Some(stream) = editor.features.review().stream.as_ref() else {
        return;
    };
    let index: HashMap<PathBuf, usize> = files
        .iter()
        .enumerate()
        .map(|(new, file)| (file.change.path().to_path_buf(), new))
        .collect();
    let mut maps = Vec::with_capacity(stream.files.len());
    for old in &stream.files {
        let Some(&new) = index.get(old.change.path()) else {
            maps.push(None);
            continue;
        };
        let (base, source) = match (&old.content, &files[new].content) {
            (
                Content::Text {
                    base: old_base,
                    source: old_source,
                },
                Content::Text { base, source },
            ) => (
                layout::line_map(old_base, base),
                layout::line_map(old_source, source),
            ),
            // INVARIANT: never a side kept as unchanged. A binary file has no
            // parses to keep.
            _ => (Some(Vec::new()), Some(Vec::new())),
        };
        files[new].opened = layout::map_ranges(&old.opened, source.as_deref());
        // WHY: a side whose text didn't change keeps its parse, and a file
        // with neither side changed its hunks. Diffing every file again made
        // each key typed in a large review slow. A changed side starts over:
        // `files` may be clones that still hold the old ones.
        let file = &mut files[new];
        file.trees.base = match base {
            None => old.trees.base.clone(),
            Some(_) => Default::default(),
        };
        file.trees.source = match source {
            None => old.trees.source.clone(),
            Some(_) => Default::default(),
        };
        file.hunks = match (&base, &source) {
            (None, None) => old.hunks.clone(),
            _ => OnceLock::new(),
        };
        file.indent = match source {
            None => old.indent.clone(),
            Some(_) => OnceLock::new(),
        };
        maps.push(Some((new, base, source)));
    }
    if let Some((file, lines)) = open {
        files[file].opened.push(lines);
    }

    let layout = layout(&message(&stream.scope), &files, stream.context);
    let old_keys: Vec<_> = stream
        .rows
        .iter()
        .map(|row| {
            let (file, key) = row.key();
            let Some(file) = file else {
                return Some((None, key));
            };
            let (new, base, source) = maps[file].as_ref()?;
            let key = match key {
                Key::Header | Key::Message(_) => key,
                Key::Gap(line) => Key::Gap(layout::map_line(source.as_deref(), line)?),
                Key::Removed(line) => Key::Removed(layout::map_line(base.as_deref(), line)?),
                Key::Source(line) => Key::Source(layout::map_line(source.as_deref(), line)?),
            };
            Some((Some(*new), key))
        })
        .collect();
    let new_keys: Vec<_> = layout.rows.iter().map(|row| row.key()).collect();
    let text = Rope::from(layout.text.as_str());
    let transaction = reconcile::transaction(&stream.text, &old_keys, &text, &new_keys);

    let doc = editor.documents.get_mut(&doc_id).expect("a view shows it");
    doc.apply(&transaction, view);
    sync::settle(doc, &mut editor.tree, view);
    debug_assert_eq!(*doc.text(), text);
    let version = doc.version();
    let stream = editor
        .features
        .review_mut()
        .stream
        .as_mut()
        .expect("checked above");
    stream.ranges = Ranges::new(&layout.rows, &hunks(&layout.rows), &text);
    let old = std::mem::replace(&mut stream.files, files);
    stream.rows = layout.rows;
    stream.text = text;
    stream.version = version;
    stream.widths = layout.widths;
    stream.relayout = false;
    sync::save_selections(editor, doc_id);
    diagnostics::place(editor);
    files::rebuild(editor, &old);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn modified(name: &str, base: &str, source: &str) -> Section {
        Section {
            change: FileChange::Modified {
                path: PathBuf::from(name),
            },
            content: Content::Text {
                base: Rope::from(base),
                source: Rope::from(source),
            },
            encoding: helix_core::encoding::UTF_8,
        }
    }

    #[test]
    fn a_commits_message_is_laid_out_above_its_files() {
        let commit = |parents: &[&str], message: &str| {
            Scope::Commit(Arc::new(Commit {
                id: "0123456789abcdef".into(),
                short_id: "0123456".into(),
                parents: parents.iter().map(|parent| parent.to_string()).collect(),
                author: "Ada".into(),
                email: "ada@example.com".into(),
                date: "2000-01-01 00:00:00 +0000".into(),
                message: message.into(),
            }))
        };
        let head = "commit 0123456789abcdef\n";
        let by = "Author: Ada <ada@example.com>\nDate:   2000-01-01 00:00:00 +0000\n\n";
        let cases = [
            (
                "a subject alone",
                commit(&["aaaaaaa"], "Fix it."),
                format!("{head}{by}    Fix it.\n\n"),
            ),
            (
                "a body keeps its empty lines empty",
                commit(&["aaaaaaa"], "Fix it.\n\nIt broke.\n  Twice."),
                format!("{head}{by}    Fix it.\n\n    It broke.\n      Twice.\n\n"),
            ),
            (
                "a merge names its parents",
                commit(&["aaaaaaa", "bbbbbbb"], "Merge it."),
                format!("{head}Merge: aaaaaaa bbbbbbb\n{by}    Merge it.\n\n"),
            ),
            (
                "the working tree's changes have none",
                Scope::Changes(ChangeView::All),
                String::new(),
            ),
        ];
        for (case, scope, expected) in cases {
            let shown = layout(
                &message(&scope),
                &files(vec![modified("a.txt", "old\n", "new\n")]),
                layout::CONTEXT,
            );
            assert_eq!(
                shown.text,
                format!("{expected}a.txt  modified\nold\nnew\n"),
                "{case}"
            );
            let lines = expected.lines().count();
            let mut rows: Vec<Row> = (0..lines).map(|line| Row::Message { line }).collect();
            rows.extend([
                Row::Header { file: 0 },
                Row::Removed { file: 0, base: 0 },
                Row::Added { file: 0, source: 0 },
            ]);
            assert_eq!(shown.rows, rows, "{case}");
        }
    }

    #[test]
    fn layout_text_has_one_line_per_row() {
        let layout = layout(
            &[],
            &files(vec![
                modified(
                    "a.txt",
                    "1\n2\n3\n4\n5\n6\n7\n8\n9\n",
                    "1\n2\n3\n4\n5\nsix\n7\n8\n9\n",
                ),
                modified("b.txt", "x\r\ny", "x\r\nz"),
                Section {
                    change: FileChange::Added {
                        path: PathBuf::from("c.bin"),
                    },
                    content: Content::Binary,
                    encoding: helix_core::encoding::UTF_8,
                },
            ]),
            layout::CONTEXT,
        );
        assert_eq!(
            layout.text,
            "a.txt  modified\n\
             ⋯ 2 unchanged lines\n\
             3\n4\n5\n6\nsix\n7\n8\n9\n\
             b.txt  modified\r\n\
             x\r\ny\r\nz\r\n\
             c.bin  new file, binary\n"
        );
        assert_eq!(layout.rows.len(), Rope::from(layout.text).len_lines() - 1);
        assert_eq!(layout.widths, [1, 1]);
    }

    #[test]
    fn widths_fit_the_widest_line_number_on_each_side() {
        let base: String = (1..=1002).map(|n| format!("{n}\n")).collect();
        let source: String = (6..=1002)
            .map(|n| match n {
                1000 => "thousand\n".to_string(),
                n => format!("{n}\n"),
            })
            .collect();
        let shown = layout(
            &[],
            &files(vec![modified("a.txt", &base, &source)]),
            layout::CONTEXT,
        );
        assert_eq!(shown.widths, [4, 3]);

        let shown = layout(
            &[],
            &files(vec![modified("a.txt", "", "new\n")]),
            layout::CONTEXT,
        );
        assert_eq!(shown.widths, [1, 1]);
    }

    fn files(sections: Vec<Section>) -> Vec<File> {
        sections.into_iter().map(File::from).collect()
    }

    fn stream(sections: Vec<Section>) -> Stream {
        stream_of(files(sections))
    }

    pub(super) fn stream_of(files: Vec<File>) -> Stream {
        let layout = layout(&[], &files, layout::CONTEXT);
        Stream {
            doc: DocumentId::default(),
            root: PathBuf::new(),
            scope: Scope::Changes(ChangeView::All),
            context: layout::CONTEXT,
            files,
            rows: layout.rows,
            text: Rope::from(layout.text.as_str()),
            version: 0,
            selections: HashMap::new(),
            widths: layout.widths,
            ranges: Ranges::default(),
            indent: IndentStyle::Tabs,
            pending: Vec::new(),
            loaded: Vec::new(),
            relayout: false,
            log: log::Log::default(),
            reports: diagnostics::Reports::new(),
            deferred: None,
            list: files::List::default(),
        }
    }

    #[test]
    fn hunks_are_runs_of_changed_rows_within_a_file() {
        let base = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let source = "A\nb\nc\nd\ne\nf\ng\nh\ni\n";
        let stream = stream(vec![
            modified("one.txt", base, source),
            modified("two.txt", "x\n", "y\n"),
        ]);
        assert_eq!(stream.hunks(), [1..3, 10..12, 13..15]);
        assert_eq!(stream.headers().collect::<Vec<_>>(), [0, 12]);
    }

    #[test]
    fn changes_count_the_lines_of_a_files_hunks_and_none_for_a_binary_file() {
        let stream = stream(vec![
            modified(
                "one.txt",
                "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n",
                "A\nB\nb\nc\nd\ne\nf\ng\nh\ni\n",
            ),
            modified("same.txt", "x\n", "x\n"),
            Section {
                change: FileChange::Added {
                    path: PathBuf::from("c.bin"),
                },
                content: Content::Binary,
                encoding: helix_core::encoding::UTF_8,
            },
        ]);
        let counts: Vec<_> = stream.changes().map(|(_, counts)| counts).collect();
        assert_eq!(counts, [Some((2, 3)), Some((0, 0)), None]);
    }

    #[test]
    fn picked_lines_are_the_changed_rows_and_which_the_selection_covers() {
        let base = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let source = "A\nb\nc\nd\ne\nf\ng\nh\ni\n";
        let stream = stream(vec![
            modified("one.txt", base, source),
            modified("two.txt", "x\n", "y\n"),
            modified("three.txt", "p\n", "q\n"),
        ]);
        // Rows: 0 header, 1 -a, 2 +A, 3-5 context, 6 gap, 7-9 context, 10 -j,
        // 11 -k, 12 header, 13 -x, 14 +y, 15 header, 16 -p, 17 +q.
        let picked = stream.picked(&[(2, 10), (12, 13)]);
        let lines: Vec<_> = picked
            .iter()
            .map(|file| {
                (
                    file.change.path().to_str().unwrap(),
                    file.removed.clone(),
                    file.added.clone(),
                )
            })
            .collect();
        assert_eq!(
            lines,
            [
                (
                    "one.txt",
                    vec![(0, false), (9, true), (10, false)],
                    vec![(0, true)]
                ),
                ("two.txt", vec![(0, true)], vec![(0, false)]),
            ],
            "a file with no row under the selection is left out"
        );
        assert!(
            stream.picked(&[(3, 9), (12, 12)]).is_empty(),
            "unchanged rows, a gap, and a header pick nothing"
        );
    }

    #[test]
    fn ranges_cover_rows_without_their_line_endings_and_the_changed_words() {
        let shown = layout(
            &[],
            &files(vec![modified(
                "a.txt",
                "x = 1\r\nkeep\n",
                "x = 2\r\nkeep\n",
            )]),
            layout::CONTEXT,
        );
        let text = Rope::from(shown.text.as_str());
        let ranges = Ranges::new(&shown.rows, &hunks(&shown.rows), &text);
        let texts = |ranges: &[Range<usize>]| -> Vec<String> {
            ranges
                .iter()
                .map(|range| text.slice(range.clone()).to_string())
                .collect()
        };
        assert_eq!(text, "a.txt  modified\r\nx = 1\r\nx = 2\r\nkeep\n");
        assert_eq!(texts(&ranges.headers), ["a.txt  modified"]);
        assert_eq!(texts(&ranges.removed), ["x = 1"]);
        assert!(ranges.gaps.is_empty());
        assert_eq!(texts(&ranges.words), ["1", "2"]);
    }

    #[test]
    fn target_lands_on_the_source_line_a_row_stands_for() {
        let base = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let source = "A\nb\nc\nd\ne\nf\ng\nh\ni\n";
        let stream = stream(vec![
            modified("one.txt", base, source),
            Section {
                change: FileChange::Untracked {
                    path: PathBuf::from("two.txt"),
                },
                content: Content::Text {
                    base: Rope::new(),
                    source: Rope::from("new\n"),
                },
                encoding: helix_core::encoding::UTF_8,
            },
        ]);
        let target = |row| {
            let target = stream.target(row).unwrap();
            (
                target.change.path().to_str().unwrap().to_string(),
                target.line,
                target.same_line,
            )
        };
        let one = |line, same_line| ("one.txt".to_string(), line, same_line);
        assert_eq!(target(0), one(0, false), "a header opens its first change");
        assert_eq!(
            target(1),
            one(0, false),
            "a removed row opens the line below it"
        );
        assert_eq!(target(2), one(0, true), "an added row opens its own line");
        assert_eq!(target(3), one(1, true), "a context row opens its own line");
        assert!(
            stream.target(6).is_none(),
            "a gap opens in place, not in the file"
        );
        assert_eq!(
            target(10),
            one(9, false),
            "a removal at the end opens past the last line"
        );
        assert_eq!(target(12), ("two.txt".to_string(), 0, false));
        assert!(stream.target(14).is_none());
    }
}
