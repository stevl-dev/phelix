//! The file list's command, and the `helix-term` side of its buffer kind.

use anyhow::{bail, Context as _};
use helix_core::command_line::Args;
use helix_core::fuzzy::fuzzy_match;
use helix_view::{DocumentId, Editor};

use helix_term::commands::WriteOptions;
use helix_term::compositor;
use helix_term::kind::BufferKindUi;
use helix_term::ui::prompt::Completion;
use helix_term::ui::PromptEvent;

use crate::view::files;

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
