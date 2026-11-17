//! `:review-log`: a picker of the commits `HEAD` reaches, which opens the one
//! picked in the review.

use helix_core::command_line::Args;
use helix_term::compositor::{self, Compositor};
use helix_term::job::Callback;
use helix_term::ui::overlay::overlaid;
use helix_term::ui::{Picker, PickerColumn, PromptEvent};
use helix_tui::text::Span;
use helix_vcs::commit::Commit;
use helix_view::graphics::Style;
use helix_view::{Editor, Theme};

use super::{read_commit, repository};

/// The theme scope of a commit's hash, in this picker and in the message
/// atop a commit's review, and the scope it falls back to.
pub(super) const HASH: (&str, &str) = ("ui.review.commit.hash", "constant.numeric");
pub(super) const DATE: (&str, &str) = ("ui.review.commit.date", "comment");
pub(super) const AUTHOR: (&str, &str) = ("ui.review.commit.author", "string");

struct Styles {
    hash: Style,
    date: Style,
    author: Style,
}

impl Styles {
    fn new(theme: &Theme) -> Styles {
        let style = |(scope, fallback)| {
            theme
                .try_get_exact(scope)
                .unwrap_or_else(|| theme.get(fallback))
        };
        Styles {
            hash: style(HASH),
            date: style(DATE),
            author: style(AUTHOR),
        }
    }
}

pub(super) fn review_log(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let (root, trust_full) = repository(cx.editor);
    // WHY: the picker is made where the compositor is, and the walk's error
    // goes to this editor through its jobs, so the injector crosses over.
    let (tx, rx) = tokio::sync::oneshot::channel();
    cx.jobs.callback(async move {
        let open = move |editor: &mut Editor, compositor: &mut Compositor| {
            let columns = [
                PickerColumn::new("hash", |commit: &Commit, styles: &Styles| {
                    Span::styled(commit.short_id.as_str(), styles.hash).into()
                }),
                PickerColumn::new("date", |commit: &Commit, styles: &Styles| {
                    Span::styled(commit.date.get(..10).unwrap_or(&commit.date), styles.date).into()
                }),
                PickerColumn::new("author", |commit: &Commit, styles: &Styles| {
                    Span::styled(commit.author.as_str(), styles.author).into()
                }),
                PickerColumn::new("subject", |commit: &Commit, _: &Styles| {
                    commit.subject().into()
                }),
            ];
            // WHY: every action opens the commit the same way. The review has
            // one tab, so there is no split to open it in.
            let picker = Picker::new(
                columns,
                3,
                [],
                Styles::new(&editor.theme),
                |cx, commit: &Commit, _action| read_commit(cx, commit.id.clone()),
            );
            let _ = tx.send(picker.injector());
            compositor.push(Box::new(overlaid(picker)));
        };
        Ok(Callback::EditorCompositor(Box::new(open)))
    });
    cx.jobs.callback(async move {
        let injector = rx.await?;
        let walked = tokio::task::spawn_blocking(move || {
            helix_vcs::commit::walk(&root, trust_full, |commit| injector.push(commit).is_ok())
        })
        .await?;
        let report = move |editor: &mut Editor| {
            if let Err(err) = walked {
                editor.set_error(format!("{err:#}"));
            }
        };
        Ok(Callback::Editor(Box::new(report)))
    });
    Ok(())
}
