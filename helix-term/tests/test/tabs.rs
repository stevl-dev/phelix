//! What closing views and buffers does with a second tab open. No command
//! opens one yet, so the tests open it through the editor.

use helix_core::diagnostic::Severity;
use helix_stdx::path;
use helix_term::application::Application;
use helix_view::{doc, editor::Action, tree::Tab};

use super::*;

/// Opens the review tab on a new scratch buffer.
fn open_review_tab(app: &mut Application) {
    let doc = app.editor.new_file(Action::Load);
    app.editor.open_tab(Tab::Review, doc);
}

#[tokio::test(flavor = "multi_thread")]
async fn quit_on_either_tabs_last_view_checks_for_unsaved_buffers_then_quits() -> anyhow::Result<()>
{
    for tab in [Tab::Code, Tab::Review] {
        let mut app = AppBuilder::new().build()?;
        open_review_tab(&mut app);
        app.editor.switch_tab(tab);
        test_key_sequences(
            &mut app,
            vec![
                (
                    Some("ihello<esc>:q<ret>"),
                    Some(&|app| {
                        let (message, severity) = app.editor.get_status().unwrap();
                        assert_eq!(Severity::Error, *severity);
                        assert_eq!(
                            "'quit': 1 unsaved buffer remaining: [\"[scratch]\"]",
                            message
                        );
                    }),
                ),
                (Some(":q!<ret>"), None),
            ],
            true,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn quit_on_a_split_closes_only_the_split() -> anyhow::Result<()> {
    let mut app = AppBuilder::new().build()?;
    open_review_tab(&mut app);
    test_key_sequence(
        &mut app,
        Some(":vsplit<ret>:q<ret>"),
        Some(&|app| {
            assert_eq!(Tab::Review, app.editor.tree.tab());
            assert_eq!(1, app.editor.tree.traverse().count());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_a_tabs_only_buffer_closes_the_tab_and_keeps_helix_running() -> anyhow::Result<()> {
    let file = tempfile::NamedTempFile::new()?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    open_review_tab(&mut app);
    test_key_sequence(
        &mut app,
        Some(":buffer-close<ret>"),
        Some(&|app| {
            assert_eq!(Tab::Code, app.editor.tree.tab());
            assert_eq!(None, app.editor.tree.tab_focus(Tab::Review));
            assert_eq!(
                Some(path::normalize(file.path()).as_path()),
                doc!(app.editor).path()
            );
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_the_hidden_code_tabs_last_buffer_leaves_it_a_view() -> anyhow::Result<()> {
    let file = tempfile::NamedTempFile::new()?;
    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;
    open_review_tab(&mut app);
    test_key_sequence(
        &mut app,
        Some(&format!(
            ":buffer-close {}<ret>",
            path::normalize(file.path()).display()
        )),
        Some(&|app| {
            assert_eq!(Tab::Review, app.editor.tree.tab());
            let code = app.editor.tree.tab_focus(Tab::Code).unwrap();
            assert!(app.editor.tree.try_get(code).is_some());
        }),
        false,
    )
    .await
}
