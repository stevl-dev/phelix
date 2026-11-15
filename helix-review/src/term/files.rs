//! The file list's command, and the `helix-term` side of its buffer kind:
//! how the list looks.
//!
//! The row of the file the review's cursor is in has a tint, from the theme's
//! `ui.review.current`, or else `ui.selection`.

use anyhow::{bail, Context as _};
use helix_core::command_line::Args;
use helix_core::fuzzy::fuzzy_match;
use helix_core::syntax::OverlayHighlights;
use helix_view::graphics::Rect;
use helix_view::{Document, DocumentId, Editor, Theme, View};

use helix_term::commands::WriteOptions;
use helix_term::compositor;
use helix_term::kind::BufferKindUi;
use helix_term::ui::document::{LinePos, TextRenderer};
use helix_term::ui::prompt::Completion;
use helix_term::ui::text_decorations::DecorationManager;
use helix_term::ui::PromptEvent;

use crate::view::files::{self, Entry};
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

    fn decorate<'a>(
        &self,
        editor: &'a Editor,
        doc: &'a Document,
        _view: &View,
        theme: &Theme,
        decorations: &mut DecorationManager<'a>,
        _overlays: &mut Vec<OverlayHighlights>,
    ) {
        let list = editor
            .features
            .review()
            .stream()
            .map(|stream| stream.list());
        let Some(list) = list.filter(|list| list.doc == doc.id()) else {
            return;
        };
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

pub(super) fn complete_state(_editor: &Editor, input: &str) -> Vec<Completion> {
    fuzzy_match(input, STATES, false)
        .into_iter()
        .map(|(state, _)| ((0..), state.into()))
        .collect()
}
