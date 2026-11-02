//! Tabs: switching between them, and what closing views and buffers does with
//! a second tab open. The tests open a tab of their own through the editor, on
//! a scratch buffer, as a feature would.

use helix_core::diagnostic::Severity;
use helix_stdx::path;
use helix_term::application::Application;
use helix_term::feature::Feature;
use helix_view::{
    doc,
    editor::{Action, TabOptions},
    tree::Tab,
};

use super::*;

const OTHER: Tab = Tab("other");

/// A feature that opens `OTHER`, so `:tab` knows its name.
const TABS: Feature = Feature {
    name: "tabs",
    commands: &[],
    typable: &[],
    tabs: &[OTHER],
    config: None,
    picker_actions: &[],
    register: || {},
};

/// An app builder, with `OTHER` among the features' tabs.
fn builder() -> AppBuilder {
    helix_term::feature::init(&[TABS]);
    AppBuilder::new()
}

/// Opens `OTHER` on a new scratch buffer.
fn open_other_tab(app: &mut Application) {
    let doc = app.editor.new_file(Action::Load);
    app.editor.open_tab(OTHER, doc, TabOptions::default());
}

#[tokio::test(flavor = "multi_thread")]
async fn quit_on_either_tabs_last_view_checks_for_unsaved_buffers_then_quits() -> anyhow::Result<()>
{
    for tab in [Tab::CODE, OTHER] {
        let mut app = builder().build()?;
        open_other_tab(&mut app);
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
    let mut app = builder().build()?;
    open_other_tab(&mut app);
    test_key_sequence(
        &mut app,
        Some(":vsplit<ret>:q<ret>"),
        Some(&|app| {
            assert_eq!(OTHER, app.editor.tree.tab());
            assert_eq!(1, app.editor.tree.traverse().count());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_a_tabs_only_buffer_closes_the_tab_and_keeps_helix_running() -> anyhow::Result<()> {
    let file = tempfile::NamedTempFile::new()?;
    let mut app = builder().with_file(file.path(), None).build()?;
    open_other_tab(&mut app);
    test_key_sequence(
        &mut app,
        Some(":buffer-close<ret>"),
        Some(&|app| {
            assert_eq!(Tab::CODE, app.editor.tree.tab());
            assert_eq!(None, app.editor.tree.tab_focus(OTHER));
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
    let mut app = builder().with_file(file.path(), None).build()?;
    open_other_tab(&mut app);
    test_key_sequence(
        &mut app,
        Some(&format!(
            ":buffer-close {}<ret>",
            path::normalize(file.path()).display()
        )),
        Some(&|app| {
            assert_eq!(OTHER, app.editor.tree.tab());
            let code = app.editor.tree.tab_focus(Tab::CODE).unwrap();
            assert!(app.editor.tree.try_get(code).is_some());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn tab_switches_to_an_open_tab_and_names_the_active_one() -> anyhow::Result<()> {
    let mut app = builder().build()?;
    open_other_tab(&mut app);
    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":tab code<ret>"),
                Some(&|app| assert_eq!(Tab::CODE, app.editor.tree.tab())),
            ),
            (
                Some(":tab other<ret>"),
                Some(&|app| assert_eq!(OTHER, app.editor.tree.tab())),
            ),
            (
                Some(":tab<ret>"),
                Some(&|app| {
                    let (message, _) = app.editor.get_status().unwrap();
                    assert_eq!("In the other tab", message);
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn tab_refuses_a_tab_that_isnt_open_or_doesnt_exist() -> anyhow::Result<()> {
    for (keys, refusal) in [
        (":tab other<ret>", "'tab': The other tab isn't open"),
        (
            ":tab git<ret>",
            "'tab': 'git' is not a tab. The tabs are code and other",
        ),
    ] {
        test_key_sequence(
            &mut builder().build()?,
            Some(keys),
            Some(&|app| {
                let (message, severity) = app.editor.get_status().unwrap();
                assert_eq!(Severity::Error, *severity);
                assert_eq!(refusal, message);
                assert_eq!(Tab::CODE, app.editor.tree.tab());
            }),
            false,
        )
        .await?;
    }
    Ok(())
}
