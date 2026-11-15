//! `:review-log`: a picker of the commits `HEAD` reaches, which opens the one
//! picked in the review.

use helix_core::command_line::Args;
use helix_term::compositor::{self, Compositor};
use helix_term::job::Callback;
use helix_term::ui::overlay::overlaid;
use helix_term::ui::{Picker, PickerColumn, PromptEvent};
use helix_vcs::commit::Commit;
use helix_view::Editor;

use super::{read_commit, repository};

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
        let open = move |_: &mut Editor, compositor: &mut Compositor| {
            let columns = [
                PickerColumn::new("hash", |commit: &Commit, _: &()| {
                    commit.short_id.as_str().into()
                }),
                PickerColumn::new("date", |commit: &Commit, _: &()| {
                    commit.date.get(..10).unwrap_or(&commit.date).into()
                }),
                PickerColumn::new("author", |commit: &Commit, _: &()| {
                    commit.author.as_str().into()
                }),
                PickerColumn::new("subject", |commit: &Commit, _: &()| commit.subject().into()),
            ];
            // WHY: every action opens the commit the same way. The review has
            // one tab, so there is no split to open it in.
            let picker = Picker::new(columns, 3, [], (), |cx, commit: &Commit, _action| {
                read_commit(cx, commit.id.clone())
            });
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
