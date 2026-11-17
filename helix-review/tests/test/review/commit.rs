//! `:review-commit` over a repository with a history.

use helix_review::view::Scope;

use super::*;

const SEVEN: &str = "1\n2\n3\n4\n5\n6\nseven\n8\n9\n10\n";
const TWO: &str = "1\ntwo\n3\n4\n5\n6\nseven\n8\n9\n10\n";
const THREE: &str = "1\ntwo\nthree\n4\n5\n6\nseven\n8\n9\n10\n";

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "--all"]);
    git(
        dir,
        &["commit", "--quiet", "--allow-empty", "--message", message],
    );
}

/// Three commits. The second changes `a.txt`'s seventh line, adds `new.txt`,
/// and deletes `old.txt`. The third changes `a.txt`'s second line. The working
/// tree changes its third line, and `b.txt`'s fifth.
fn history() -> TempDir {
    let repo = repo(
        &[("a.txt", TEN), ("b.txt", TEN), ("old.txt", "old\n")],
        &[
            ("a.txt", Some(SEVEN)),
            ("old.txt", None),
            ("new.txt", Some("fresh\n")),
        ],
    );
    commit(repo.path(), "Change the seventh line.");
    std::fs::write(repo.path().join("a.txt"), TWO).unwrap();
    commit(repo.path(), "Change the second line.");
    std::fs::write(repo.path().join("a.txt"), THREE).unwrap();
    std::fs::write(repo.path().join("b.txt"), TEN.replace("5\n", "five\n")).unwrap();
    repo
}

/// Runs `:review-commit rev` until the status names the commit by `subject`.
async fn review_commit(app: &mut Application, rev: &str, subject: &str) -> anyhow::Result<()> {
    run_until(app, &format!(":review-commit {rev}<ret>"), |app| {
        let status = status(app);
        status.starts_with("Reviewing ") && status.ends_with(&format!(": {subject}"))
    })
    .await
}

/// The rows of the message of the commit the review shows, above its files.
fn message(app: &Application, subject: &str) -> String {
    let Scope::Commit(commit) = &stream(app).scope else {
        panic!("the review shows {:?}", stream(app).scope);
    };
    format!(
        "commit {}\n\
         Author: author <author@example.com>\n\
         Date:   {}\n\
         \n    {subject}\n\n",
        commit.id, commit.date
    )
}

/// The second commit's rows.
fn seventh_line(app: &Application, repo: &TempDir) -> String {
    let path = |name| shown(repo.path().join(name));
    format!(
        "{}{}  modified\n\
         ⋯ 3 unchanged lines\n\
         4\n5\n6\n7\nseven\n8\n9\n10\n\
         {}  new file\n\
         fresh\n\
         {}  deleted\n\
         old\n",
        message(app, "Change the seventh line."),
        path("a.txt"),
        path("new.txt"),
        path("old.txt"),
    )
}

/// The working tree's rows.
fn working_tree(repo: &TempDir) -> String {
    let path = |name| shown(repo.path().join(name));
    format!(
        "{}  modified\n\
         1\ntwo\n3\nthree\n4\n5\n6\n\
         ⋯ 4 unchanged lines\n\
         {}  modified\n\
         ⋯ 1 unchanged line\n\
         2\n3\n4\n5\nfive\n6\n7\n8\n\
         ⋯ 2 unchanged lines\n",
        path("a.txt"),
        path("b.txt"),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commits_review_shows_its_changes_and_not_the_working_trees() -> anyhow::Result<()> {
    let repo = history();
    let mut app = app_on(repo.path().join("a.txt"))?;
    review_commit(&mut app, "HEAD~1", "Change the seventh line.").await?;

    assert!(in_stream(&app));
    assert_eq!(stream_text(&app), seventh_line(&app, &repo));
    assert_eq!(cursor(&app), (0, 0));
    assert!(doc!(app.editor).readonly);
    let Scope::Commit(commit) = &stream(&app).scope else {
        panic!("the review shows {:?}", stream(&app).scope);
    };
    let head = Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["rev-parse", "HEAD~1"])
        .output()?;
    assert_eq!(commit.id, String::from_utf8(head.stdout)?.trim());

    // WHY: `:reload` reads the scope it shows again. A read of the working
    // tree's changes there would replace the commit.
    run_until(&mut app, "/^seven<ret>:reload<ret>", |app| {
        status(app).ends_with(": Change the seventh line.")
    })
    .await?;
    assert_eq!(stream_text(&app), seventh_line(&app, &repo));
    assert_eq!(cursor(&app).0, 12, "a reload keeps the cursor's row");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commits_review_takes_no_edit_and_asks_no_file() -> anyhow::Result<()> {
    let repo = history();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    review_commit(&mut app, "HEAD~1", "Change the seventh line.").await?;
    let read_only = "A commit's review is read-only. ret opens the file.";
    let not_the_files = "A commit's review shows the commit, not the files.";

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("/^seven<ret>ix"),
                Some(&|app| {
                    assert_eq!(stream_text(app), seventh_line(app, &repo));
                    assert_eq!(app.editor.mode, Mode::Normal);
                    assert_eq!(status(app), read_only);
                    assert_eq!(buffer_text(app, &file).as_deref(), Some(THREE));
                }),
            ),
            (
                Some(":echo cleared<ret>u"),
                Some(&|app| assert_eq!(status(app), read_only)),
            ),
            (
                Some("gd"),
                Some(&|app| assert_eq!(status(app), not_the_files)),
            ),
            (
                Some(":stage-hunk<ret>"),
                Some(&|app| assert_eq!(status(app), format!("'stage-hunk': {not_the_files}"))),
            ),
            (
                Some(":unstage-line<ret>"),
                Some(&|app| assert_eq!(status(app), format!("'unstage-line': {not_the_files}"))),
            ),
            (
                Some(":review-restore<ret>"),
                Some(&|app| {
                    assert_eq!(status(app), format!("'review-restore': {not_the_files}"));
                    assert_eq!(buffer_text(app, &file).as_deref(), Some(THREE));
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_waits_for_the_unsaved_files_of_the_review_it_replaces() -> anyhow::Result<()> {
    let repo = history();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    start_review(&mut app).await?;
    run_until(&mut app, "/^three<ret>ix<esc>:review-commit<ret>", |app| {
        status(app).starts_with("1 unsaved buffer remaining: ")
    })
    .await?;

    assert!(in_stream(&app));
    assert_eq!(
        stream(&app).scope,
        Scope::Changes(helix_vcs::ChangeView::All)
    );
    assert_eq!(
        stream_text(&app),
        working_tree(&repo).replace("three", "xthree")
    );
    assert_eq!(
        buffer_text(&app, &file),
        Some(THREE.replace("three", "xthree"))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_and_the_working_trees_changes_replace_each_other() -> anyhow::Result<()> {
    let repo = history();
    let b = repo.path().join("b.txt");
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app).await?;
    // WHY: an edit to a file that isn't open loads its buffer, which the
    // review closes with the rows it replaces.
    run_until(&mut app, "/^five<ret>ix<esc>:w<ret>", |_| {
        std::fs::read_to_string(&b).is_ok_and(|text| text.contains("xfive"))
    })
    .await?;
    assert!(app.editor.document_by_path(&b).is_some());

    review_commit(&mut app, "HEAD~1", "Change the seventh line.").await?;
    assert_eq!(stream_text(&app), seventh_line(&app, &repo));
    assert_eq!(cursor(&app), (0, 0));
    assert!(app.editor.document_by_path(&b).is_none());

    run_until(&mut app, "/^fresh<ret>:review all<ret>", |app| {
        status(app) == "Reviewing all changes"
    })
    .await?;
    assert_eq!(
        stream_text(&app),
        working_tree(&repo).replace("five", "xfive")
    );
    assert_eq!(cursor(&app), (0, 0));
    assert!(!doc!(app.editor).readonly);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn review_alone_switches_to_a_commits_review() -> anyhow::Result<()> {
    let repo = history();
    let mut app = app_on(repo.path().join("a.txt"))?;
    review_commit(&mut app, "HEAD", "Change the second line.").await?;
    let rows = stream_text(&app);

    test_key_sequence(
        &mut app,
        Some(":tab code<ret>:review<ret>"),
        Some(&|app| {
            assert!(in_stream(app));
            assert_eq!(stream_text(app), rows);
            assert!(status(app).ends_with(": Change the second line."));
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn enter_in_a_commits_review_opens_the_working_trees_file() -> anyhow::Result<()> {
    let repo = history();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    review_commit(&mut app, "HEAD~1", "Change the seventh line.").await?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("/^old$<ret><ret>"),
                Some(&|app| {
                    assert!(in_stream(app));
                    assert_eq!(
                        status(app),
                        format!(
                            "{} isn't in the working tree",
                            shown(repo.path().join("old.txt"))
                        )
                    );
                    assert!(app
                        .editor
                        .document_by_path(repo.path().join("old.txt"))
                        .is_none());
                }),
            ),
            (
                Some("/^seven<ret><ret>"),
                Some(&|app| {
                    assert_eq!(app.editor.tree.tab(), Tab::CODE);
                    assert_eq!(doc!(app.editor).path(), Some(file.as_path()));
                    assert_eq!(cursor(app), (6, 4));
                    assert_eq!(doc!(app.editor).text(), THREE);
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_with_no_changes_or_no_name_leaves_the_open_review() -> anyhow::Result<()> {
    let repo = history();
    let mut app = app_on(repo.path().join("a.txt"))?;
    review_commit(&mut app, "HEAD", "Change the second line.").await?;
    let rows = stream_text(&app);
    // WHY: `commit` adds the working tree's changes, so only a second one is
    // empty.
    commit(repo.path(), "Change the third line.");
    commit(repo.path(), "Change nothing.");

    run_until(&mut app, ":review-commit<ret>", |app| {
        status(app).ends_with(" changes no files")
    })
    .await?;
    assert!(in_stream(&app));
    assert_eq!(stream_text(&app), rows);

    run_until(&mut app, ":review-commit nowhere<ret>", |app| {
        status(app) == "nowhere names no commit"
    })
    .await?;
    assert_eq!(stream_text(&app), rows);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_rows_of_a_commits_message_belong_to_no_file() -> anyhow::Result<()> {
    let repo = history();
    let mut app = app_on(repo.path().join("a.txt"))?;
    app.editor.set_theme(toml::from_str(
        r#"
        "ui.selection" = { bg = "yellow" }
        "ui.review.current" = { bg = "magenta" }
        "#,
    )?)?;
    review_commit(&mut app, "HEAD~1", "Change the seventh line.").await?;
    let rows = seventh_line(&app, &repo);

    // Rows: 0 to 5 the message, 6 a.txt's header.
    run_until(&mut app, "gn", |app| files::tinted(app) == ["a.txt"]).await?;
    assert_eq!(cursor(&app), (6, 0));
    run_until(&mut app, "gp", |app| cursor(app) == (6, 0)).await?;
    run_until(&mut app, "4k", |app| files::tinted(app).is_empty()).await?;
    assert_eq!(cursor(&app), (2, 0));

    run_until(&mut app, "<ret>", in_stream).await?;
    assert_eq!(app.editor.tree.tab(), REVIEW_TAB);
    assert_eq!(cursor(&app), (2, 0));

    run_until(&mut app, "ix", |app| {
        status(app) == "A commit's review is read-only. ret opens the file."
    })
    .await?;
    assert_eq!(stream_text(&app), rows);

    run_until(&mut app, ":reload<ret>", |app| {
        status(app).ends_with(": Change the seventh line.")
    })
    .await?;
    assert_eq!(stream_text(&app), rows);
    assert_eq!(cursor(&app), (2, 0), "a reload keeps the cursor on its row");
    Ok(())
}

/// Whether the screen shows `text`.
fn on_screen(app: &Application, text: &str) -> bool {
    let buffer = app.terminal_buffer();
    let symbols = buffer.content().iter().map(|cell| cell.symbol.as_str());
    symbols.collect::<String>().contains(text)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_commit_picker_lists_head_first_and_opens_the_commit_picked() -> anyhow::Result<()> {
    let repo = history();
    let mut app = app_on(repo.path().join("a.txt"))?;
    // WHY: the walk adds the commits from another thread, and `Enter` in a
    // picker with none picks nothing.
    run_until(&mut app, ":review-log<ret>", |app| {
        on_screen(app, "Change the second line.") && on_screen(app, "base")
    })
    .await?;
    run_until(&mut app, "<ret>", |app| {
        status(app).ends_with(": Change the second line.")
    })
    .await?;

    assert!(in_stream(&app));
    assert!(stream_text(&app).starts_with(&message(&app, "Change the second line.")));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_commit_picker_filters_by_subject() -> anyhow::Result<()> {
    let repo = history();
    let mut app = app_on(repo.path().join("a.txt"))?;
    run_until(&mut app, ":review-log<ret>", |app| {
        on_screen(app, "Change the second line.")
    })
    .await?;
    // WHY: the filter runs off the main thread too. `Enter` before it has
    // run picks the first commit of them all.
    run_until(&mut app, "seventh", |app| {
        !on_screen(app, "Change the second line.")
    })
    .await?;
    run_until(&mut app, "<ret>", |app| {
        status(app).ends_with(": Change the seventh line.")
    })
    .await?;

    assert_eq!(stream_text(&app), seventh_line(&app, &repo));
    Ok(())
}

/// The text colors of the screen's row that shows `text`, left to right, once
/// for each run of a color, leaving out the terminal's own.
fn colors(app: &Application, text: &str) -> Vec<Color> {
    let buffer = app.terminal_buffer();
    let row = buffer
        .content()
        .chunks(buffer.area.width as usize)
        .find(|row| {
            let symbols = row.iter().map(|cell| cell.symbol.as_str());
            symbols.collect::<String>().contains(text)
        })
        .unwrap_or_else(|| panic!("the screen shows no {text:?}"));
    let mut colors: Vec<Color> = row
        .iter()
        .filter(|cell| cell.symbol.as_str() != " " && cell.fg != Color::Reset)
        .map(|cell| cell.fg)
        .collect();
    colors.dedup();
    colors
}

/// A theme that sets the hash's scope and the scope it falls back to, and
/// leaves the date to its fallback.
const COMMIT_THEME: &str = r#"
    "ui.selection" = { bg = "black" }
    "ui.review.commit.hash" = { fg = "red" }
    "constant.numeric" = { fg = "yellow" }
    "comment" = { fg = "green" }
    "ui.review.commit.author" = { fg = "blue" }
"#;

#[tokio::test(flavor = "multi_thread")]
async fn the_commit_picker_colors_the_hash_date_and_author() -> anyhow::Result<()> {
    let repo = history();
    let mut app = app_on(repo.path().join("a.txt"))?;
    app.editor.set_theme(toml::from_str(COMMIT_THEME)?)?;
    run_until(&mut app, ":review-log<ret>", |app| {
        on_screen(app, "Change the second line.")
    })
    .await?;

    assert_eq!(
        colors(&app, "Change the second line."),
        [Color::Red, Color::Green, Color::Blue]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commits_message_colors_its_hash_author_and_date() -> anyhow::Result<()> {
    let repo = history();
    let mut app = app_on(repo.path().join("a.txt"))?;
    app.editor.set_theme(toml::from_str(COMMIT_THEME)?)?;
    review_commit(&mut app, "HEAD~1", "Change the seventh line.").await?;

    assert_eq!(colors(&app, "commit "), [Color::Red]);
    assert_eq!(colors(&app, "Author: "), [Color::Blue]);
    assert_eq!(colors(&app, "Date: "), [Color::Green]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_commit_picker_says_when_the_repository_has_no_commit() -> anyhow::Result<()> {
    let repo = tempfile::tempdir()?;
    git(repo.path(), &["init", "--quiet"]);
    std::fs::write(repo.path().join("a.txt"), TEN)?;
    let mut app = app_on(repo.path().join("a.txt"))?;

    run_until(&mut app, ":review-log<ret>", |app| {
        status(app) == "the repository has no commit yet"
    })
    .await
}
