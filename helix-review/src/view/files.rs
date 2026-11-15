//! The review's file list: the stream's files as a tree of directories, one
//! row each, in a buffer of its own. A side view of the review's tab shows
//! it. Hiding the list closes that view, and the buffer stays for as long as
//! the stream's.
//!
//! The stream's sync holds the buffer's text, as it holds the stream's, and
//! keeps the views that show it narrow and without a gutter.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::ops::Range;
use std::path::PathBuf;

use helix_core::diff::compare_ropes;
use helix_core::movement::Direction;
use helix_core::{Rope, Selection};
use helix_vcs::FileChange;
use helix_view::document::Mode;
use helix_view::editor::Action;
use helix_view::kind::BufferKind;
use helix_view::{align_view, Align, Document, DocumentId, Editor, ViewId};

use super::sync::{settle, view_of};
use super::{File, ReviewState, Row, REVIEW_TAB};

/// The name of the list's buffer kind.
pub const KIND_NAME: &str = "review-files";

const PUT_BACK: &str = "The edit was put back";

struct Kind;

static KIND: Kind = Kind;

impl BufferKind for Kind {
    fn name(&self) -> &'static str {
        KIND_NAME
    }

    fn source(&self, _editor: &mut Editor, _view: ViewId) -> Result<DocumentId, String> {
        Err("The review's file list shows no code".to_string())
    }

    fn source_open<'e>(&self, _editor: &'e Editor, _view: ViewId) -> Option<&'e Document> {
        None
    }

    // WHY: the buffer lives as long as the review's, so `:bc` on it only
    // hides the list.
    fn close(
        &self,
        editor: &mut Editor,
        _doc: DocumentId,
        _force: bool,
    ) -> Result<(), Vec<(DocumentId, String)>> {
        set_shown(editor, false);
        Ok(())
    }

    fn goto_buffer(&self, editor: &mut Editor, direction: Direction, count: usize) -> bool {
        super::step_rows(editor, direction, count, |review, doc| {
            let list = &review.stream.as_ref()?.list;
            (list.doc == doc).then(|| list.file_rows().collect())
        });
        true
    }
}

/// What a line of the list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// A directory, and the first of the stream's files under it.
    Dir { first: usize },
    /// The stream's file of this index.
    File { file: usize },
}

impl Entry {
    /// The file the row stands for: its own, or a directory's first.
    pub fn file(self) -> usize {
        match self {
            Entry::Dir { first: file } | Entry::File { file } => file,
        }
    }
}

/// The list's buffer, and what it has to hold.
#[derive(Default)]
pub struct List {
    pub doc: DocumentId,
    /// One per line of the buffer.
    pub rows: Vec<Entry>,
    text: Rope,
    /// The buffer's version when it last held `text`.
    version: i32,
    /// Whether `text` changed since the buffer last held it.
    stale: bool,
    /// Each view's selection when the buffer last held `text`.
    selections: HashMap<ViewId, Selection>,
    /// The view of the tab that last had focus while it showed the review.
    review_view: Option<ViewId>,
    /// The file of that view's cursor.
    current: Option<usize>,
    /// Whether the review's view moved since the focus came to the list.
    visited: bool,
}

impl List {
    /// Opens the list's buffer, empty and in no view.
    pub(super) fn open(editor: &mut Editor) -> List {
        let id = editor.new_file(Action::Load);
        let doc = editor.documents.get_mut(&id).expect("just opened");
        doc.readonly = true;
        doc.listed = false;
        doc.name_without_path = "[files]";
        doc.kind = Some(&KIND);
        List {
            doc: id,
            text: doc.text().clone(),
            version: doc.version(),
            ..List::default()
        }
    }

    /// The file the review's cursor is in.
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    fn file_rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, entry)| matches!(entry, Entry::File { .. }))
            .map(|(row, _)| row)
    }
}

struct Tree {
    rows: Vec<Entry>,
    /// One line per row.
    text: String,
}

/// The rows for `changes`, in their order. A directory gets a row above the
/// run of files under it, so an order that isn't a tree's shows it once per
/// run.
fn tree<'a>(changes: impl Iterator<Item = &'a FileChange>) -> Tree {
    let paths: Vec<_> = changes
        .map(|change| helix_stdx::path::get_relative_path(change.path()))
        .collect();
    let files: Vec<Vec<&OsStr>> = paths
        .iter()
        .map(|path| path.components().map(|part| part.as_os_str()).collect())
        .collect();
    let mut tree = Tree {
        rows: Vec::with_capacity(files.len()),
        text: String::new(),
    };
    tree.push_level(&files, 0..files.len(), 0, 0);
    tree
}

impl Tree {
    /// Adds the rows of `files[run]`, which share their first `depth` parts,
    /// `level` levels in.
    fn push_level(&mut self, files: &[Vec<&OsStr>], run: Range<usize>, depth: usize, level: usize) {
        let mut next = run.start;
        while next < run.end {
            let file = &files[next];
            if file.len() <= depth + 1 {
                let name = file.last().map(|name| name.to_string_lossy());
                self.push_row(Entry::File { file: next }, level, &name.unwrap_or_default());
                next += 1;
                continue;
            }
            let in_dir = |index: &usize, part: usize| {
                let other = &files[*index];
                other.len() > part + 1 && other[part] == file[part]
            };
            let end = (next..run.end)
                .find(|index| !in_dir(index, depth))
                .unwrap_or(run.end);
            // WHY: a directory with no file of its own and one directory in
            // it shares its row with that one, so a deep path costs one level.
            let mut last = depth + 1;
            while (next..end).all(|index| in_dir(&index, last)) {
                last += 1;
            }
            let dir: PathBuf = file[depth..last].iter().collect();
            let mut name = dir.display().to_string();
            if !name.ends_with(std::path::MAIN_SEPARATOR) {
                name.push('/');
            }
            self.push_row(Entry::Dir { first: next }, level, &name);
            self.push_level(files, next..end, last, level + 1);
            next = end;
        }
    }

    fn push_row(&mut self, entry: Entry, level: usize, name: &str) {
        self.rows.push(entry);
        for _ in 0..level {
            self.text.push_str("  ");
        }
        self.text.push_str(name);
        self.text.push('\n');
    }
}

/// Lays the list out for the stream's files, which took the place of `old`,
/// and keeps each view's cursor on its file, by path.
pub(super) fn rebuild(editor: &mut Editor, old: &[File]) {
    let Some(stream) = editor.features.review().stream.as_ref() else {
        return;
    };
    let path = |file: &File| file.change.path().to_path_buf();
    if old.iter().map(path).eq(stream.files.iter().map(path)) {
        return;
    }
    // INVARIANT: the cursors are read off the old rows, so the buffer has to
    // hold the old text.
    if hold(editor) {
        editor.set_status(PUT_BACK);
    }
    let Some(stream) = editor.features.review_mut().stream.as_mut() else {
        return;
    };
    let new = tree(stream.files.iter().map(|file| &file.change));
    let text = Rope::from(new.text.as_str());
    let list = &mut stream.list;
    let Some(doc) = editor.documents.get(&list.doc) else {
        return;
    };
    list.selections.clear();
    for (view, _) in editor.tree.views() {
        if view.doc != list.doc {
            continue;
        }
        let shown = doc.text().slice(..);
        let row = shown.char_to_line(doc.selection(view.id).primary().cursor(shown));
        let kept = list.rows.get(row).and_then(|entry| {
            let path = old[entry.file()].change.path();
            let rows_of = |dir: bool| -> Vec<usize> {
                let of_file = |new: &Entry| {
                    matches!(new, Entry::Dir { .. }) == dir
                        && stream.files[new.file()].change.path() == path
                };
                (0..new.rows.len())
                    .filter(|&row| of_file(&new.rows[row]))
                    .collect()
            };
            let same = rows_of(matches!(entry, Entry::Dir { .. }));
            // WHY: nested directories share a first file, so a directory's
            // row is the one as deep as it was, or the deepest.
            let above = list.rows[..row].iter().filter(|above| *above == entry);
            same.get(above.count())
                .or(same.last())
                .copied()
                .or_else(|| rows_of(false).first().copied())
        });
        let row = kept.unwrap_or(row.min(new.rows.len().saturating_sub(1)));
        list.selections
            .insert(view.id, Selection::point(text.line_to_char(row)));
    }
    list.rows = new.rows;
    list.text = text;
    list.stale = true;
    hold(editor);
}

/// Makes the list's buffer hold the list's text, through a view that shows
/// it. A hidden list waits for one. Returns whether that put an edit back.
fn hold(editor: &mut Editor) -> bool {
    let Some((list, doc)) = editor
        .features
        .review()
        .stream
        .as_ref()
        .and_then(|stream| Some((&stream.list, editor.documents.get(&stream.list.doc)?)))
    else {
        return false;
    };
    let edited = doc.version() != list.version;
    if !edited && !list.stale {
        return false;
    }
    let doc_id = list.doc;
    // INVARIANT: the buffer's history is committed through a view of it.
    let Some(view) = view_of(editor, doc_id) else {
        return false;
    };
    if editor.mode == Mode::Insert && editor.tree.get(editor.tree.focus).doc == doc_id {
        // INVARIANT: before the text is restored. Leaving insert mode can
        // edit the buffer, to drop the indent of an empty line.
        editor.enter_normal_mode();
    }

    let (Some(stream), Some(doc)) = (
        editor.features.review_mut().stream.as_mut(),
        editor.documents.get_mut(&doc_id),
    ) else {
        return false;
    };
    let list = &mut stream.list;
    let transaction = compare_ropes(doc.text(), &list.text);
    doc.apply(&transaction, view);
    for (view, _) in editor.tree.views() {
        if view.doc == doc_id {
            // WHY: a view with none is new. Its cursor went to the end with
            // the text that came in before it.
            let selection = list.selections.get(&view.id).cloned();
            doc.set_selection(view.id, selection.unwrap_or_else(|| Selection::point(0)));
        }
    }
    settle(doc, &mut editor.tree, view);
    list.version = doc.version();
    list.stale = false;

    let scrolloff = editor.config().scrolloff;
    let doc = editor.documents.get_mut(&doc_id).expect("checked above");
    for (view, _) in editor.tree.views() {
        if view.doc == doc_id {
            view.ensure_cursor_in_view(doc, scrolloff);
        }
    }
    edited
}

/// Holds the list's text and notes each view's selection in it. Runs with
/// the stream's sync.
pub(super) fn sync(editor: &mut Editor) {
    if hold(editor) {
        editor.set_status(PUT_BACK);
    }
    follow(editor);
    let Some(stream) = editor.features.review_mut().stream.as_mut() else {
        return;
    };
    let list = &mut stream.list;
    let Some(doc) = editor.documents.get(&list.doc) else {
        return;
    };
    list.selections.clear();
    for (view, _) in editor.tree.views() {
        if view.doc == list.doc {
            list.selections
                .insert(view.id, doc.selection(view.id).clone());
        }
    }
}

/// Keeps the list and the review on one file. With the focus in the list, the
/// review's view shows the file under the list's cursor. With the focus
/// anywhere else, the list's cursors go to the file under the review's.
// INVARIANT: which way it goes depends only on the focus, so the two can't
// chase each other.
fn follow(editor: &mut Editor) {
    let Some(stream) = editor.features.review().stream.as_ref() else {
        return;
    };
    let views = views_of(editor, stream.doc);
    let view = [editor.tree.tab_focus(REVIEW_TAB), stream.list.review_view]
        .into_iter()
        .flatten()
        .find(|view| views.contains(view))
        .or(views.first().copied());
    let cursor_row = |doc: &Document, view: ViewId| {
        let text = doc.text().slice(..);
        text.char_to_line(doc.selection(view).primary().cursor(text))
    };
    let mut current = view.and_then(|view| {
        let row = cursor_row(editor.documents.get(&stream.doc)?, view);
        // WHY: the line below the last row is the last file's too.
        let row = stream.rows.get(row).or(stream.rows.last())?;
        Some(row.file())
    });

    let scrolloff = editor.config().scrolloff;
    let focus = editor.tree.get(editor.tree.focus);
    let Some(stream) = editor.features.review_mut().stream.as_mut() else {
        return;
    };
    let list = &mut stream.list;
    list.review_view = view;
    list.current = current;
    let Some(doc) = editor.documents.get(&list.doc) else {
        return;
    };

    if focus.doc == list.doc {
        let row = cursor_row(doc, focus.id);
        let file = list
            .rows
            .get(row)
            .or(list.rows.last())
            .map(|row| row.file());
        let header = file.filter(|_| file != current).and_then(|file| {
            let header = |row: &Row| *row == Row::Header { file };
            stream.rows.iter().position(header)
        });
        let (Some(header), Some(view), Some(doc)) =
            (header, view, editor.documents.get_mut(&stream.doc))
        else {
            return;
        };
        let view = editor.tree.get_mut(view);
        // WHY: one jump for a visit to the list, however far it goes, so
        // `C-o` in the review goes back to where the visit started.
        if !std::mem::replace(&mut list.visited, true) {
            let jump = (doc.id(), doc.selection(view.id).clone());
            view.push_jump(doc, jump);
        }
        let start = doc.text().line_to_char(header);
        doc.set_selection(view.id, Selection::point(start));
        align_view(doc, view, Align::Top);
        current = file;
        list.current = current;
        return;
    }

    list.visited = false;
    let row = current.and_then(|file| {
        let row = |entry: &Entry| *entry == Entry::File { file };
        list.rows.iter().position(row)
    });
    let list = list.doc;
    let (Some(row), Some(doc)) = (row, editor.documents.get_mut(&list)) else {
        return;
    };
    for (view, _) in editor.tree.views() {
        if view.doc != list || row >= doc.text().len_lines() {
            continue;
        }
        if cursor_row(doc, view.id) != row {
            doc.set_selection(view.id, Selection::point(doc.text().line_to_char(row)));
            view.ensure_cursor_in_view(doc, scrolloff);
        }
    }
}

/// The view of the tab that last had focus while it showed the review.
pub fn review_view(editor: &Editor) -> Option<ViewId> {
    let stream = editor.features.review().stream.as_ref()?;
    stream.list.review_view.filter(|&view| {
        let view = editor.tree.try_get(view);
        view.is_some_and(|view| view.doc == stream.doc)
    })
}

/// The review tab's views and what each shows.
fn tab_views(editor: &Editor) -> Vec<(ViewId, DocumentId)> {
    editor
        .tree
        .views()
        .filter(|(view, _)| editor.tree.tab_of(view.id) == REVIEW_TAB)
        .map(|(view, _)| (view.id, view.doc))
        .collect()
}

/// The views of the review's tab that show `doc`, with the tab's focused one
/// first.
pub(super) fn views_of(editor: &Editor, doc: DocumentId) -> Vec<ViewId> {
    let focus = editor.tree.tab_focus(REVIEW_TAB);
    let mut views: Vec<_> = tab_views(editor)
        .into_iter()
        .filter(|&(_, shown)| shown == doc)
        .map(|(view, _)| view)
        .collect();
    views.sort_by_key(|&view| Some(view) != focus);
    views
}

/// Keeps the list's views what they have to be: side views with no gutter,
/// and only beside the review. Runs before the stream's sync.
pub(super) fn keep_views(editor: &mut Editor) {
    let Some(stream) = editor.features.review().stream.as_ref() else {
        return;
    };
    let (review, list) = (stream.doc, stream.list.doc);
    if views_of(editor, review).is_empty() {
        let mut lists = views_of(editor, list);
        // WHY: a list alone would hold the tab open with nothing beside it.
        // The tab's last view shows the review instead, so `C-w o` in the
        // list leaves the review. A hidden tab has no focus to switch in.
        let alone = lists.len() == tab_views(editor).len();
        if alone && editor.tree.tab() == REVIEW_TAB && !lists.is_empty() {
            editor.focus(lists.remove(0));
            editor.switch(review, Action::Replace);
        }
        for view in lists {
            editor.close(view);
        }
    }
    // WHY: set each frame, so a reload of the config resizes an open list.
    let width = editor.features.review().config().sidebar_width.get();
    for (id, doc) in tab_views(editor) {
        let side = editor.tree.get(id).side;
        if doc == list {
            editor.tree.set_side(id, Some(width));
            // WHY: every refresh gives the view the configured gutters back.
            editor.tree.get_mut(id).gutters.layout.clear();
        } else if side.is_some() {
            editor.tree.set_side(id, None);
            editor.tree.get_mut(id).gutters = editor.config().gutters.clone();
        }
    }
}

/// Whether a view of the review's tab shows the list.
pub fn shown(editor: &Editor) -> bool {
    let list = editor.features.review().stream.as_ref();
    list.is_some_and(|stream| !views_of(editor, stream.list.doc).is_empty())
}

/// Shows the list in a side view of the review's tab, or closes the views
/// that show it. `false` when there's no view of the review to show it
/// beside.
pub fn set_shown(editor: &mut Editor, show: bool) -> bool {
    let Some(stream) = editor.features.review().stream.as_ref() else {
        return false;
    };
    let (review, list) = (stream.doc, stream.list.doc);
    let views = views_of(editor, list);
    if !show {
        for view in views {
            editor.close(view);
        }
        return true;
    }
    if views_of(editor, review).is_empty() {
        return false;
    }
    if views.is_empty() {
        let width = editor.features.review().config().sidebar_width.get();
        editor.open_side(REVIEW_TAB, list, width);
    }
    true
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn tree_rows_follow_the_files_order_under_their_directories() {
        use Entry::{Dir, File};

        let modified = |path: &str| FileChange::Modified {
            path: PathBuf::from(path),
        };
        let renamed = |from: &str, to: &str| FileChange::Renamed {
            from_path: PathBuf::from(from),
            to_path: PathBuf::from(to),
        };
        let cases = [
            (
                "nested directories",
                vec![
                    modified("a/b/one.rs"),
                    modified("a/b/two.rs"),
                    modified("a/three.rs"),
                ],
                "a/\n  b/\n    one.rs\n    two.rs\n  three.rs\n",
                vec![
                    Dir { first: 0 },
                    Dir { first: 0 },
                    File { file: 0 },
                    File { file: 1 },
                    File { file: 2 },
                ],
            ),
            (
                "a chain of directories shares a row, up to one with a file",
                vec![modified("a/b/c/one.rs"), modified("a/b/c/d/e/two.rs")],
                "a/b/c/\n  one.rs\n  d/e/\n    two.rs\n",
                vec![
                    Dir { first: 0 },
                    File { file: 0 },
                    Dir { first: 1 },
                    File { file: 1 },
                ],
            ),
            (
                "a file at the root",
                vec![modified("a/one.rs"), modified("README.md")],
                "a/\n  one.rs\nREADME.md\n",
                vec![Dir { first: 0 }, File { file: 0 }, File { file: 1 }],
            ),
            (
                "a rename shows under its new path",
                vec![modified("new/a.rs"), renamed("old/b.rs", "new/b.rs")],
                "new/\n  a.rs\n  b.rs\n",
                vec![Dir { first: 0 }, File { file: 0 }, File { file: 1 }],
            ),
            (
                "one file",
                vec![modified("one.rs")],
                "one.rs\n",
                vec![File { file: 0 }],
            ),
            (
                "an order that isn't a tree's shows a directory once per run",
                vec![
                    modified("a/one.rs"),
                    modified("b/two.rs"),
                    modified("a/three.rs"),
                ],
                "a/\n  one.rs\nb/\n  two.rs\na/\n  three.rs\n",
                vec![
                    Dir { first: 0 },
                    File { file: 0 },
                    Dir { first: 1 },
                    File { file: 1 },
                    Dir { first: 2 },
                    File { file: 2 },
                ],
            ),
        ];
        for (case, changes, text, rows) in cases {
            let tree = tree(changes.iter());
            assert_eq!(tree.text, text, "{case}");
            assert_eq!(tree.rows, rows, "{case}");
        }
    }
}
