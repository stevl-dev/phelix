//! The file list's command, and the `helix-term` side of its buffer kind:
//! how the list looks.
//!
//! - The row of the file the review's cursor is in has a tint, from the
//!   theme's `ui.review.current`, or else `ui.selection`.
//! - Directory rows take `ui.text.directory`.
//! - A file's row ends with how it changed and the lines its hunks add and
//!   remove, like `M +12 -3`, at the view's right edge. They're drawn, not
//!   text, so a search and a yank see only names. The letter takes
//!   `diff.plus`, `diff.delta`, or `diff.minus`, a conflict's `error`, and the
//!   counts `diff.plus` and `diff.minus`.

use std::collections::HashMap;

use anyhow::{bail, Context as _};
use helix_core::command_line::Args;
use helix_core::fuzzy::fuzzy_match;
use helix_core::syntax::OverlayHighlights;
use helix_core::Position;
use helix_vcs::FileChange;
use helix_view::document::Mode;
use helix_view::graphics::{Rect, Style};
use helix_view::{doc, Document, DocumentId, Editor, Theme, View};

use helix_term::commands::{Context, WriteOptions};
use helix_term::compositor;
use helix_term::keymap::KeyTrie;
use helix_term::kind::BufferKindUi;
use helix_term::ui::document::{LinePos, TextRenderer};
use helix_term::ui::prompt::Completion;
use helix_term::ui::text_decorations::{Decoration, DecorationManager};
use helix_term::ui::PromptEvent;

use crate::view::files::{self, Entry, List};
use crate::view::ReviewState;

pub(super) struct Ui;

pub(super) static UI: Ui = Ui;

impl BufferKindUi for Ui {
    fn reload(&self, cx: &mut compositor::Context, _doc: DocumentId) {
        super::reload(cx);
    }

    fn write(
        &self,
        cx: &mut compositor::Context,
        _doc: DocumentId,
        path: Option<&str>,
        options: WriteOptions,
    ) -> anyhow::Result<()> {
        super::write::write(cx, path, options)
    }

    fn keymap(&self, user: &HashMap<Mode, KeyTrie>) -> Option<HashMap<Mode, KeyTrie>> {
        Some(super::keymap::files_keymap(user))
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
        let stream = editor.features.review().stream();
        let Some(stream) = stream.filter(|stream| stream.list().doc == doc.id()) else {
            return;
        };
        let list = stream.list();

        let text = doc.text();
        let first = text.char_to_line(doc.view_offset(view.id).anchor.min(text.len_chars()));
        let last = (first + view.inner_height())
            .min(list.rows.len())
            .min(text.len_lines() - 1);
        let directories: Vec<_> = (first..last)
            .filter(|&row| matches!(list.rows[row], Entry::Dir { .. }))
            .map(|row| text.line_to_char(row)..text.line_to_char(row + 1) - 1)
            .collect();
        if let Some(highlight) = theme.find_highlight("ui.text.directory") {
            overlays.push(OverlayHighlights::Homogeneous {
                highlight,
                ranges: directories,
            });
        }

        let current = list.current().map(|file| Entry::File { file });
        let tint = theme
            .try_get_exact("ui.review.current")
            .unwrap_or_else(|| theme.get("ui.selection"));
        decorations.add_decoration(move |renderer: &mut TextRenderer, pos: LinePos| {
            if current.is_some() && list.rows.get(pos.doc_line) == current.as_ref() {
                let viewport = renderer.viewport;
                let row = Rect::new(viewport.x, pos.visual_line, viewport.width, 1);
                renderer.set_style(row, tint);
            }
        });

        let marks: Vec<_> = stream
            .changes()
            .map(|(change, counts)| Mark::new(change, counts, theme))
            .collect();
        decorations.add_decoration(Marks {
            list,
            width: marks.iter().map(Mark::width).max().unwrap_or(0),
            marks,
            added: theme.get("diff.plus"),
            removed: theme.get("diff.minus"),
        });
    }
}

/// What a file's row ends with.
struct Mark {
    letter: char,
    style: Style,
    added: String,
    removed: String,
}

impl Mark {
    fn new(change: &FileChange, counts: Option<(usize, usize)>, theme: &Theme) -> Mark {
        let (letter, scope) = match change {
            FileChange::Modified { .. } => ('M', "diff.delta"),
            FileChange::Added { .. } => ('A', "diff.plus"),
            FileChange::Untracked { .. } => ('?', "diff.plus"),
            FileChange::Deleted { .. } => ('D', "diff.minus"),
            FileChange::Renamed { .. } => ('R', "diff.delta"),
            FileChange::Conflict { .. } => ('U', "error"),
        };
        let (added, removed) = counts_text(counts);
        Mark {
            letter,
            style: theme.get(scope),
            added,
            removed,
        }
    }

    /// The columns its counts take.
    fn width(&self) -> usize {
        let between = !self.added.is_empty() && !self.removed.is_empty();
        self.added.len() + between as usize + self.removed.len()
    }
}

/// A file's counts as its row shows them. A side with no lines is left out,
/// and so are both for a file with no counts.
fn counts_text(counts: Option<(usize, usize)>) -> (String, String) {
    let side = |sign: char, lines: usize| match lines {
        0 => String::new(),
        lines => format!("{sign}{lines}"),
    };
    let (added, removed) = counts.unwrap_or((0, 0));
    (side('+', added), side('-', removed))
}

/// Draws each file row's mark at the view's right edge, with the letters in
/// one column.
struct Marks<'a> {
    list: &'a List,
    /// One per file of the stream.
    marks: Vec<Mark>,
    /// The widest counts.
    width: usize,
    added: Style,
    removed: Style,
}

impl Decoration for Marks<'_> {
    // WHY: not `decorate_line`, which runs before the row's text. The mark
    // covers the tail of a name that's too long, so it's drawn after it.
    fn render_virt_lines(
        &mut self,
        renderer: &mut TextRenderer,
        pos: LinePos,
        _virt_off: Position,
    ) -> Position {
        let none = Position::new(0, 0);
        let Some(Entry::File { file }) = self.list.rows.get(pos.doc_line) else {
            return none;
        };
        let Some(mark) = self.marks.get(*file).filter(|_| pos.first_visual_line) else {
            return none;
        };
        // A blank, the letter, and with any counts, a blank and their column.
        let counts = if self.width == 0 { 0 } else { 1 + self.width };
        let viewport = renderer.viewport;
        let Some(start) = (viewport.width as usize).checked_sub(2 + counts) else {
            return none;
        };
        let (x, y) = (viewport.x + start as u16, pos.visual_line);
        let right = viewport.x + viewport.width;
        // WHY: a default style, so the blanks keep the row's tint.
        renderer.set_string(x, y, &" ".repeat(2 + counts), Style::default());
        renderer.set_string(x + 1, y, mark.letter.encode_utf8(&mut [0; 4]), mark.style);
        for (text, end, style) in [
            (&mark.added, mark.width(), self.added),
            (&mark.removed, mark.removed.len(), self.removed),
        ] {
            if !text.is_empty() {
                renderer.set_string(right - end as u16, y, text, style);
            }
        }
        none
    }
}

const STATES: [&str; 2] = ["show", "hide"];

pub(super) fn review_sidebar(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    super::open_stream(cx.editor).context("There is no review. Start one with :review")?;
    let show = match args.first() {
        None => !files::shown(cx.editor),
        Some("show") => true,
        Some("hide") => false,
        Some(other) => bail!("'{other}' is not one of show or hide"),
    };
    if !files::set_shown(cx.editor, show) {
        bail!("No view shows the review. :review switches to it");
    }
    Ok(())
}

/// `Enter` in the list. The review already shows the file under the list's
/// cursor, so it only takes the focus there.
pub(super) fn goto_file(cx: &mut Context) {
    let list = super::open_stream(cx.editor).map(|stream| stream.list().doc);
    if list != Some(doc!(cx.editor).id()) {
        cx.editor
            .set_error("This buffer isn't a review's file list. :review-sidebar shows it");
        return;
    }
    if let Some(view) = files::review_view(cx.editor) {
        cx.editor.focus(view);
    }
}

pub(super) fn complete_state(_editor: &Editor, input: &str) -> Vec<Completion> {
    fuzzy_match(input, STATES, false)
        .into_iter()
        .map(|(state, _)| ((0..), state.into()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::counts_text;

    #[test]
    fn counts_leave_out_a_side_with_no_lines() {
        let cases = [
            (Some((12, 3)), ("+12", "-3")),
            (Some((120, 0)), ("+120", "")),
            (Some((0, 5)), ("", "-5")),
            (Some((0, 0)), ("", "")),
            (None, ("", "")),
        ];
        for (counts, (added, removed)) in cases {
            assert_eq!(
                counts_text(counts),
                (added.to_string(), removed.to_string()),
                "{counts:?}"
            );
        }
    }
}
