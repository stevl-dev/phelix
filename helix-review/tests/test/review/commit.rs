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

/// The second commit's rows.
fn seventh_line(repo: &TempDir) -> String {
    let path = |name| shown(repo.path().join(name));
    format!(
        "{}  modified\n\
         ⋯ 3 unchanged lines\n\
         4\n5\n6\n7\nseven\n8\n9\n10\n\
         {}  new file\n\
         fresh\n\
         {}  deleted\n\
         old\n",
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
    assert_eq!(stream_text(&app), seventh_line(&repo));
    assert_eq!(cursor(&app), (0, 0));
    assert!(doc!(app.editor).readonly);
    let Scope::Commit(commit) = &stream(&app).scope else {
        panic!("the review shows {:?}", stream(&app).scope);
    };
    assert_eq!(commit.message, "Change the seventh line.");

    // WHY: `:reload` reads the scope it shows again. A read of the working
    // tree's changes there would replace the commit.
    run_until(&mut app, "/^seven<ret>:reload<ret>", |app| {
        status(app).ends_with(": Change the seventh line.")
    })
    .await?;
    assert_eq!(stream_text(&app), seventh_line(&repo));
    assert_eq!(cursor(&app).0, 6, "a reload keeps the cursor's row");
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
                    assert_eq!(stream_text(app), seventh_line(&repo));
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
    assert_eq!(stream_text(&app), seventh_line(&repo));
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
