use super::*;

use helix_lsp::lsp::Url;
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
