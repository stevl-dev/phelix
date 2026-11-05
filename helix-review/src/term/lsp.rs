//! The review's rename. Goto, references, hover, and code actions are
//! upstream's own, which ask the review's buffer kind for the file of the row
//! under the cursor.

use crate::view as review;
use helix_core::syntax::config::LanguageServerFeature;
use helix_core::textobject::{textobject_word, TextObject};
use helix_lsp::{lsp, OffsetEncoding};
use helix_view::{doc, Editor};

use helix_term::commands::{still_at_version, Context, MappableCommand};
use helix_term::compositor::Compositor;
use helix_term::job::Callback;
use helix_term::ui::{self, PromptEvent};

/// The review's version of `command`, swapped into the review's keymap.
/// Built here rather than in the command table: it exists only as that swap.
pub(crate) fn replacement(command: &MappableCommand) -> Option<MappableCommand> {
    match command {
        MappableCommand::Static {
            name: "rename_symbol",
            doc,
            ..
        } => Some(MappableCommand::Static {
            name: "review_rename_symbol",
            fun: rename,
            doc,
        }),
        _ => None,
    }
}

// WHY: upstream's rename reads the focused buffer again when you confirm its
// prompt, and that's the review, which isn't the source. This one keeps the
// file and the place, and leaves out `prepareRename`.
fn rename(cx: &mut Context) {
    let view = cx.editor.tree.focus;
    let file = match review::source(cx.editor, view) {
        Ok(file) => file,
        Err(refusal) => {
            cx.editor.set_error(refusal);
            return;
        }
    };
    let doc = doc!(cx.editor, &file);
    let Some(server) = doc
        .language_servers_with_feature(LanguageServerFeature::RenameSymbol)
        .next()
    else {
        cx.editor
            .set_error("No configured language server supports symbol renaming");
        return;
    };
    let (server, encoding) = (server.id(), server.offset_encoding());
    let position = doc.position(view, encoding);
    let version = doc.version();
    let text = doc.text().slice(..);
    let selection = doc.selection(view).primary();
    let word = match selection.len() > 1 {
        true => selection,
        false => textobject_word(text, selection, TextObject::Inside, 1, false),
    };
    let prefill = word.fragment(text).into_owned();

    let prompt = ui::Prompt::new(
        "rename-to:".into(),
        cx.register,
        ui::completers::none,
        move |cx, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate {
                return;
            }
            if !still_at_version(cx.editor, file, version, "nothing was renamed") {
                return;
            }
            let Some(server) = cx.editor.language_server_by_id(server) else {
                cx.editor.set_error("The language server exited");
                return;
            };
            let future = server
                .rename_symbol(
                    doc!(cx.editor, &file).identifier(),
                    position,
                    input.to_string(),
                )
                .expect("the server supports renaming");
            cx.jobs.callback(async move {
                let answer = future.await;
                let call = move |editor: &mut Editor, _: &mut Compositor| {
                    let edit = match answer {
                        Ok(edit) => edit.unwrap_or_default(),
                        Err(err) => return editor.set_error(err.to_string()),
                    };
                    if still_at_version(editor, file, version, "nothing was renamed") {
                        apply(editor, encoding, &edit);
                    }
                };
                Ok(Callback::EditorCompositor(Box::new(call)))
            });
        },
    )
    .with_line(prefill, cx.editor);
    cx.push_layer(Box::new(prompt));
}

fn apply(editor: &mut Editor, encoding: OffsetEncoding, edit: &lsp::WorkspaceEdit) {
    // WHY: the rename is one step of the review's undo log, as an edit is. A
    // file it opens starts at revision 0. The revisions are taken here, not
    // when the request goes out, so an edit in another file meanwhile isn't
    // part of the step.
    let mut before: Vec<_> = editor
        .documents
        .iter_mut()
        .map(|(&id, doc)| (id, doc.get_current_revision()))
        .collect();
    let _ = editor.apply_workspace_edit(encoding, edit);
    let opened: Vec<_> = editor
        .documents
        .keys()
        .filter(|id| !before.iter().any(|(known, _)| known == *id))
        .map(|&id| (id, 0))
        .collect();
    before.extend(opened);
    review::log_step(editor, &before);
}
