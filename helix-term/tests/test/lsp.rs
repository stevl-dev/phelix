use super::*;

use helix_core::diagnostic::Severity;
use helix_lsp::lsp::Url;
use helix_view::doc;
use serde_json::json;

/// A `.fake` file with `text`, and its URI.
fn fake_file(text: &str) -> anyhow::Result<(tempfile::NamedTempFile, Url)> {
    let file = tempfile::Builder::new().suffix(".fake").tempfile()?;
    std::fs::write(file.path(), text)?;
    let path = helix_stdx::path::canonicalize(file.path());
    let uri = Url::from_file_path(path).expect("the path is absolute");
    Ok((file, uri))
}

#[tokio::test(flavor = "multi_thread")]
async fn goto_definition_jumps_where_the_server_says() -> anyhow::Result<()> {
    let (file, uri) = fake_file("one\ntwo\nthree\n")?;
    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .with_fake_lsp(json!({
            "capabilities": { "definitionProvider": true },
            "answers": {
                "textDocument/definition": { "result": {
                    "uri": uri,
                    "range": {
                        "start": { "line": 2, "character": 0 },
                        "end": { "line": 2, "character": 5 },
                    },
                }},
            },
        }))
        .build()?;
    helpers::wait_for_language_servers(&mut app).await;

    test_key_sequence(
        &mut app,
        Some("gd"),
        Some(&|app| {
            helpers::assert_status_not_error(&app.editor);
            let (view, doc) = helix_view::current_ref!(app.editor);
            let cursor = doc
                .selection(view.id)
                .primary()
                .cursor(doc.text().slice(..));
            assert_eq!(doc.text().line_to_char(2), cursor);
        }),
        false,
    )
    .await
}

/// A server with one code action, "Fix it", that replaces the first line's
/// `one` with `fixed` once it's resolved. `resolve` is added to how it
/// answers `codeAction/resolve`.
fn code_action_server(uri: &Url, resolve: serde_json::Value) -> serde_json::Value {
    let mut resolve = resolve;
    resolve["result"] = json!({
        "title": "Fix it",
        "edit": { "changes": { uri.as_str(): [{
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 3 },
            },
            "newText": "fixed",
        }]}},
    });
    json!({
        "capabilities": {
            "codeActionProvider": { "resolveProvider": true },
            "textDocumentSync": 2,
        },
        "answers": {
            "textDocument/codeAction": { "result": [{ "title": "Fix it", "data": 1 }] },
            "codeAction/resolve": resolve,
        },
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn code_action_applies_when_the_server_asks_the_editor_first() -> anyhow::Result<()> {
    let (file, uri) = fake_file("one\n")?;
    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .with_fake_lsp(code_action_server(
            &uri,
            json!({ "ask": {
                "method": "workspace/configuration",
                "params": { "items": [{ "section": "fake" }] },
            }}),
        ))
        .build()?;
    helpers::wait_for_language_servers(&mut app).await;

    test_key_sequences(
        &mut app,
        vec![
            (Some("<space>a"), None),
            (
                Some("<ret>"),
                Some(&|app| {
                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!("fixed\n", doc!(app.editor).text().to_string());
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn code_action_is_dropped_when_the_buffer_changes_first() -> anyhow::Result<()> {
    let (file, uri) = fake_file("one\n")?;
    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .with_fake_lsp(code_action_server(
            &uri,
            json!({ "after": "textDocument/didChange" }),
        ))
        .build()?;
    helpers::wait_for_language_servers(&mut app).await;
    let name = doc!(app.editor).display_name().into_owned();

    test_key_sequences(
        &mut app,
        vec![
            (Some("<space>a"), None),
            (Some("<ret>"), None),
            (
                Some("ix<esc>"),
                Some(&|app| {
                    assert_eq!("xone\n", doc!(app.editor).text().to_string());
                    let (status, severity) = app.editor.get_status().expect("a status");
                    assert_eq!(
                        format!("{name} changed, so the code action wasn't applied"),
                        *status
                    );
                    assert_eq!(&Severity::Error, severity);
                }),
            ),
        ],
        false,
    )
    .await
}

/// A server that renames both words of `one one` to `two`. `rename` is
/// added to how it answers `textDocument/rename`.
fn rename_server(uri: &Url, rename: serde_json::Value) -> serde_json::Value {
    let mut rename = rename;
    let word = |start: u32| {
        json!({
            "range": {
                "start": { "line": 0, "character": start },
                "end": { "line": 0, "character": start + 3 },
            },
            "newText": "two",
        })
    };
    rename["result"] = json!({ "changes": { uri.as_str(): [word(0), word(4)] } });
    json!({
        "capabilities": { "renameProvider": true, "textDocumentSync": 2 },
        "answers": { "textDocument/rename": rename },
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn rename_applies_when_the_server_asks_the_editor_first() -> anyhow::Result<()> {
    let (file, uri) = fake_file("one one\n")?;
    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .with_fake_lsp(rename_server(
            &uri,
            json!({ "ask": {
                "method": "workspace/configuration",
                "params": { "items": [{ "section": "fake" }] },
            }}),
        ))
        .build()?;
    helpers::wait_for_language_servers(&mut app).await;

    test_key_sequence(
        &mut app,
        Some("<space>r<ret>"),
        Some(&|app| {
            helpers::assert_status_not_error(&app.editor);
            assert_eq!("two two\n", doc!(app.editor).text().to_string());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn rename_is_dropped_when_the_buffer_changes_first() -> anyhow::Result<()> {
    let (file, uri) = fake_file("one one\n")?;
    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .with_fake_lsp(rename_server(
            &uri,
            json!({ "after": "textDocument/didChange" }),
        ))
        .build()?;
    helpers::wait_for_language_servers(&mut app).await;
    let name = doc!(app.editor).display_name().into_owned();

    test_key_sequences(
        &mut app,
        vec![
            (Some("<space>r<ret>"), None),
            (
                Some("ix<esc>"),
                Some(&|app| {
                    assert_eq!("xone one\n", doc!(app.editor).text().to_string());
                    let (status, severity) = app.editor.get_status().expect("a status");
                    assert_eq!(format!("{name} changed, so nothing was renamed"), *status);
                    assert_eq!(&Severity::Error, severity);
                }),
            ),
        ],
        false,
    )
    .await
}

/// A server that completes `o` to `one`, with an extra edit that comments
/// out the line once the item is resolved. `ask` is the request it sends
/// before it answers `completionItem/resolve`.
fn completion_server(ask: serde_json::Value) -> serde_json::Value {
    json!({
        "capabilities": {
            "completionProvider": { "resolveProvider": true },
            "textDocumentSync": 2,
        },
        "answers": {
            "textDocument/completion": { "result": [{ "label": "one", "data": 1 }] },
            "completionItem/resolve": {
                "ask": ask,
                "result": {
                    "label": "one",
                    "data": 1,
                    "additionalTextEdits": [{
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 0 },
                        },
                        "newText": "// ",
                    }],
                },
            },
        },
    })
}

/// Accepts the completion before the editor resolves it on idle, so the
/// accept resolves it, from a server that sends `ask` first.
async fn accept_resolves_when_the_server_asks(ask: serde_json::Value) -> anyhow::Result<()> {
    let (file, _) = fake_file("o\n")?;
    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .with_fake_lsp(completion_server(ask))
        .build()?;
    helpers::wait_for_language_servers(&mut app).await;

    test_key_sequences(
        &mut app,
        vec![
            (Some("A<C-x>"), None),
            (
                Some("<C-n><ret><esc>"),
                Some(&|app| {
                    helpers::assert_status_not_error(&app.editor);
                    assert_eq!("// one\n", doc!(app.editor).text().to_string());
                }),
            ),
        ],
        false,
    )
    .await
}

// WHY: two tests, not a table: a second app in one test's runtime registers
// the editor's hooks again, and its completions never open.
#[tokio::test(flavor = "multi_thread")]
async fn completion_resolves_on_accept_when_the_server_asks_for_its_configuration(
) -> anyhow::Result<()> {
    accept_resolves_when_the_server_asks(json!({
        "method": "workspace/configuration",
        "params": { "items": [{ "section": "fake.answer" }] },
        "expect": [42],
    }))
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_resolves_on_accept_when_the_server_asks_for_its_workspace_folders(
) -> anyhow::Result<()> {
    accept_resolves_when_the_server_asks(json!({
        "method": "workspace/workspaceFolders",
        "params": null,
        "expect": [],
    }))
    .await
}
