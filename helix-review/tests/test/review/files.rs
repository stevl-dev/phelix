//! The review's file list, beside it in a side view.

use helix_view::{View, ViewId};

use super::*;

/// `a.txt` and `src/b.txt` changed, and `other.txt` not.
fn two_changed() -> TempDir {
    repo(
        &[("a.txt", TEN), ("src/b.txt", TEN), ("other.txt", "same\n")],
        &[
            ("a.txt", Some("1\n2\nthree\n4\n5\n6\n7\n8\n9\n10\n")),
            ("src/b.txt", Some("1\n2\n3\n4\n5\n6\nseven\n8\n9\n10\n")),
        ],
    )
}

/// The list's rows for `names` under the repository's own row.
fn rows(repo: &TempDir, names: &[&str]) -> String {
    let mut rows = format!("{}/\n", shown(repo.path().to_path_buf()));
    for name in names {
        rows.push_str(&format!("  {name}\n"));
    }
    rows
}

fn list_text(app: &Application) -> String {
    let list = stream(app).list().doc;
    app.editor.document(list).unwrap().text().to_string()
}

/// The views on screen that show the list.
fn list_views(app: &Application) -> Vec<&View> {
    let list = stream(app).list().doc;
    let views = app.editor.tree.traverse();
    views
        .map(|(_, view)| view)
        .filter(|view| view.doc == list)
        .collect()
}

/// The row of the list's cursor, in its one view.
fn list_row(app: &Application) -> usize {
    let [view] = list_views(app)[..] else {
        panic!("one view shows the list");
    };
    let doc = app.editor.document(view.doc).unwrap();
    let text = doc.text().slice(..);
    text.char_to_line(doc.selection(view.id).primary().cursor(text))
}

fn in_list(app: &Application) -> bool {
    doc!(app.editor).id() == stream(app).list().doc
}

/// A review of `repo` in the test terminal's 120 columns, where the list
/// shows by default.
async fn review_with_list(repo: &TempDir) -> anyhow::Result<Application> {
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app).await?;
    run_until(&mut app, "", |app| !list_views(app).is_empty()).await?;
    Ok(app)
}

/// Makes the terminal `width` columns wide, and draws one frame at that
/// width.
async fn resize(app: &mut Application, width: u16) -> anyhow::Result<()> {
    app.resize_terminal(width, 150);
    // WHY: a key, since the editor finds the new size when it draws, and an
    // idle editor draws nothing.
    run_until(app, "<esc>", |app| app.editor.tree.area().width == width).await
}

/// Whether the screen shows the list's statusline.
fn list_drawn(app: &Application) -> bool {
    let buffer = app.terminal_buffer();
    let symbols = buffer.content().iter().map(|cell| cell.symbol.as_str());
    symbols.collect::<String>().contains("[files]")
}

#[tokio::test(flavor = "multi_thread")]
async fn review_sidebar_shows_the_files_in_a_side_view_and_hides_it() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;

    assert_eq!(list_text(&app), rows(&repo, &["a.txt", "src/", "  b.txt"]));
    let [list] = list_views(&app)[..] else {
        panic!("one view shows the list");
    };
    assert_eq!((list.area.x, list.area.width), (0, 30));
    assert!(list.gutters.layout.is_empty());
    assert!(in_stream(&app), "the focus stays in the review");
    assert_eq!(view!(app.editor).area.x, 31);

    for (keys, shown) in [
        (":review-sidebar<ret>", false),
        (":review-sidebar hide<ret>", false),
        (":review-sidebar show<ret>", true),
        (":review-sidebar show<ret>", true),
    ] {
        run_until(&mut app, keys, |app| {
            list_views(app).len() == shown as usize
        })
        .await?;
        assert_eq!(view!(app.editor).area.x, if shown { 31 } else { 0 });
    }

    run_until(&mut app, "<C-w>h:buffer-close<ret>", |app| {
        list_views(app).is_empty()
    })
    .await?;
    assert!(in_stream(&app), "closing the list's buffer only hides it");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_to_the_list_is_put_back() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    let before = list_text(&app);

    for keys in ["<C-w>hxd", "ihello<esc>", "u"] {
        run_until(&mut app, keys, |app| {
            in_list(app) && list_text(app) == before && app.editor.mode == Mode::Normal
        })
        .await?;
        assert!(!doc!(app.editor).is_modified(), "{keys}");
        if keys.ends_with("xd") {
            assert_eq!(selected(&app), "  a.txt\n", "the selection goes back too");
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_put_back_edit_is_named_in_the_status() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    run_until(&mut app, "<C-w>hxd", |app| {
        status(app) == "The edit was put back"
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn quit_in_the_review_quits_with_the_list_open() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    test_key_sequence(&mut app, Some(":q<ret>"), None, true).await
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_every_other_view_from_the_list_leaves_the_review() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    run_until(&mut app, "<C-w>h<C-w>o", in_stream).await?;

    let views: Vec<ViewId> = app.editor.tree.traverse().map(|(id, _)| id).collect();
    assert_eq!(views, [app.editor.tree.focus]);
    let view = view!(app.editor);
    assert_eq!((view.side, view.area.x), (None, 0));
    assert!(!view.gutters.layout.is_empty());
    assert_eq!(app.editor.tree.tab(), REVIEW_TAB);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_opened_in_the_list_goes_to_the_code_tab() -> anyhow::Result<()> {
    let repo = two_changed();
    let other = repo.path().join("other.txt");
    let mut app = review_with_list(&repo).await?;
    run_until(
        &mut app,
        &format!("<C-w>h:e {}<ret>", other.display()),
        |app| app.editor.tree.tab() == Tab::CODE,
    )
    .await?;

    assert_eq!(doc!(app.editor).path(), Some(other.as_path()));
    let list = stream(&app).list().doc;
    let shows_list = |view: &ViewId| app.editor.tree.get(*view).doc == list;
    assert_eq!(
        app.editor.tree.tab_focus(REVIEW_TAB).filter(shows_list),
        app.editor.tree.tab_focus(REVIEW_TAB),
        "the list's view shows the list again"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn goto_buffer_in_the_list_steps_between_its_file_rows() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    // Rows: 0 the repository, 1 a.txt, 2 src/, 3 b.txt.
    for (keys, row) in [("<C-w>hgn", 3), ("gn", 3), ("gp", 1), ("gp", 1)] {
        run_until(&mut app, keys, |app| in_list(app) && cursor(app).0 == row).await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reload_adds_a_new_changes_row_and_keeps_the_cursors_file() -> anyhow::Result<()> {
    let repo = repo(
        &[("a.txt", TEN), ("b.txt", TEN), ("c.txt", TEN)],
        &[
            ("a.txt", Some("1\n2\nthree\n4\n5\n6\n7\n8\n9\n10\n")),
            ("c.txt", Some("1\n2\n3\n4\n5\n6\nseven\n8\n9\n10\n")),
        ],
    );
    let mut app = review_with_list(&repo).await?;
    run_until(&mut app, "<C-w>hgnggjj", |app| {
        in_list(app) && cursor(app).0 == 2
    })
    .await?;
    std::fs::write(
        repo.path().join("b.txt"),
        "one\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
    )?;

    let reread = rows(&repo, &["a.txt", "b.txt", "c.txt"]);
    run_until(&mut app, ":reload<ret>", |app| list_text(app) == reread).await?;
    assert!(in_list(&app));
    assert_eq!(cursor(&app).0, 3, "still on c.txt");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_lists_cursor_follows_the_reviews_file_and_scrolls_to_it() -> anyhow::Result<()> {
    // WHY: more files than the test terminal has rows, so the last one's row
    // starts off screen.
    let names: Vec<String> = (0..200).map(|n| format!("f{n:03}.txt")).collect();
    let committed: Vec<_> = names.iter().map(|name| (name.as_str(), "old\n")).collect();
    let changes: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), Some("new\n")))
        .collect();
    let repo = repo(&committed, &changes);
    let mut app = app_on(repo.path().join("f000.txt"))?;
    start_review(&mut app).await?;
    run_until(&mut app, "", |app| {
        !list_views(app).is_empty() && list_row(app) == 1
    })
    .await?;

    run_until(&mut app, "ge", |app| list_row(app) == 200).await?;
    assert!(in_stream(&app));
    let view = list_views(&app)[0];
    let doc = app.editor.document(view.doc).unwrap();
    let top = doc.text().char_to_line(doc.view_offset(view.id).anchor);
    assert!(top > 0 && top + view.inner_height() > 200, "from row {top}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cursor_on_a_files_last_row_marks_that_file_in_the_list() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    // Rows: 0 the repository, 1 a.txt, 2 src/, 3 b.txt.
    run_until(&mut app, "gn", |app| list_row(app) == 3).await?;
    run_until(&mut app, "k", |app| list_row(app) == 1).await
}

/// The names on the list's rows that the screen tints from edge to edge.
pub(super) fn tinted(app: &Application) -> Vec<String> {
    let buffer = app.terminal_buffer();
    let cell = |x, y| buffer.get(x, y).unwrap();
    (0..buffer.area.height)
        .filter(|&y| (0..30).all(|x| cell(x, y).bg == Color::Magenta))
        .map(|y| (0..30).map(|x| cell(x, y).symbol.as_str()).collect())
        .map(|row: String| {
            row.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reviews_file_has_the_tint_in_the_list() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = app_on(repo.path().join("a.txt"))?;
    app.editor.set_theme(toml::from_str(
        r#"
        "ui.selection" = { bg = "yellow" }
        "ui.review.current" = { bg = "magenta" }
        "#,
    )?)?;
    start_review(&mut app).await?;
    run_until(&mut app, "", |app| tinted(app) == ["a.txt"]).await?;
    run_until(&mut app, "gn", |app| tinted(app) == ["b.txt"]).await
}

/// The cursor's row in the review's view, and the row at the view's top.
fn review_rows(app: &Application) -> (usize, usize) {
    let doc = app.editor.document(stream(app).doc).unwrap();
    let mut views = app.editor.tree.traverse().map(|(_, view)| view);
    let view = views.find(|view| view.doc == doc.id()).unwrap();
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    let top = doc.view_offset(view.id).anchor;
    (text.char_to_line(cursor), text.char_to_line(top))
}

/// The row of `b.txt`'s header in `two_changed`'s review, below `a.txt`'s.
fn second_header(app: &Application) -> usize {
    stream(app).headers().nth(1).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_review_shows_the_file_under_the_lists_cursor() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    let b = second_header(&app);
    // Rows of the list: 0 the repository, 1 a.txt, 2 src/, 3 b.txt.
    let steps = [
        ("3j<C-w>h", 1, (3, 0), "entering the list moves nothing"),
        (
            "j",
            2,
            (b, b),
            "a directory shows its first file, at the top",
        ),
        ("j", 3, (b, b), "the file's own row shows the same file"),
        ("kk", 1, (0, 0), "back to the first file's header"),
    ];
    for (keys, row, review, step) in steps {
        run_until(&mut app, keys, |app| {
            in_list(app) && cursor(app).0 == row && review_rows(app).0 == review.0
        })
        .await?;
        assert_eq!(review_rows(&app), review, "{step}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_visit_to_the_list_that_moves_nothing_leaves_the_reviews_cursor() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    run_until(&mut app, "3j<C-w>h", in_list).await?;
    run_until(&mut app, "<C-w>l", in_stream).await?;
    assert_eq!(cursor(&app), (3, 0));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn jump_backward_in_the_review_goes_back_to_before_the_visit() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    let b = second_header(&app);
    run_until(&mut app, "3j<C-w>hjj", |app| review_rows(app).0 == b).await?;
    run_until(&mut app, "kkjj<C-w>l", in_stream).await?;
    assert_eq!(cursor(&app).0, b);
    run_until(&mut app, "<C-o>", |app| cursor(app).0 == 3).await
}

#[tokio::test(flavor = "multi_thread")]
async fn enter_in_the_list_focuses_the_review_on_the_cursors_file() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    let b = second_header(&app);
    run_until(&mut app, "<C-w>hjj", |app| review_rows(app).0 == b).await?;
    run_until(&mut app, "<ret>", in_stream).await?;
    assert_eq!(cursor(&app).0, b);
    Ok(())
}

/// The list's first 30 columns of `row` on screen.
fn drawn_row(app: &Application, row: u16) -> String {
    let buffer = app.terminal_buffer();
    let y = list_views(app)[0].area.y + row;
    (0..30)
        .map(|x| buffer.get(x, y).unwrap().symbol.as_str())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn file_rows_end_with_their_status_and_counts_over_a_long_names_tail() -> anyhow::Result<()> {
    let long = "long_name_of_a_file_that_is_cut.txt";
    let repo = repo(
        &[("a.txt", TEN), ("gone.txt", "old\n"), (long, "x\n")],
        &[
            ("a.txt", Some("1\n2\nthree\nmore\n4\n5\n6\n7\n8\n9\n10\n")),
            ("gone.txt", None),
            (long, Some("y\n")),
            ("new.txt", Some("x\ny\nz\n")),
        ],
    );
    let mut app = review_with_list(&repo).await?;
    let before = list_text(&app);
    // Rows: 0 the repository, then the files in path order.
    let drawn =
        |app: &Application| -> Vec<String> { (1..5).map(|row| drawn_row(app, row)).collect() };
    run_until(&mut app, "", |app| drawn(app)[0].ends_with("-1")).await?;
    assert_eq!(
        drawn(&app),
        [
            "  a.txt                M +2 -1",
            "  gone.txt             D    -1",
            "  long_name_of_a_file_ M +1 -1",
            "  new.txt              ?    +3",
        ]
    );

    run_until(&mut app, "/three<ret>oxx<esc>", |app| {
        drawn(app)[0].ends_with("M +3 -1")
    })
    .await?;
    assert_eq!(
        list_text(&app),
        before,
        "the counts aren't the buffer's text"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_list_is_as_wide_as_the_review_table_says_while_its_open() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = app_with_review(repo.path().join("a.txt"), "sidebar-width = 40")?;
    start_review(&mut app).await?;
    let width = |app: &Application| list_views(app).first().map(|view| view.area.width);
    run_until(&mut app, "", |app| width(app) == Some(40)).await?;

    // As `:config-reload` does, after a change to the file.
    let mut narrower = app.editor.features.review().config();
    narrower.sidebar_width = 24.try_into()?;
    app.editor.features.review_mut().set_config(narrower);
    run_until(&mut app, "", |app| width(app) == Some(24)).await?;
    assert_eq!(view!(app.editor).area.x, 25, "the review takes the rest");
    Ok(())
}

/// Whether a review opens with the list, with `table` as `[review]` in a
/// terminal `width` columns wide.
async fn opens_with_list(table: &str, width: u16) -> anyhow::Result<bool> {
    let repo = two_changed();
    let mut app = app_with_review(repo.path().join("a.txt"), table)?;
    resize(&mut app, width).await?;
    start_review(&mut app).await?;
    run_until(&mut app, "", |_| true).await?;
    assert!(in_stream(&app), "the focus is in the review");
    Ok(!list_views(&app).is_empty())
}

#[test]
fn a_review_opens_with_the_list_as_the_sidebar_key_and_the_tabs_width_say() -> anyhow::Result<()> {
    let cases = [
        ("", 120, true),
        ("", 119, false),
        ("sidebar = \"auto\"", 119, false),
        ("sidebar = false", 120, false),
        ("sidebar = true", 119, true),
    ];
    for (table, width, shown) in cases {
        // WHY: an `Application` registers its event hooks for the tokio
        // runtime it runs in, so each case needs a runtime of its own.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let opened = runtime.block_on(opens_with_list(table, width))?;
        assert_eq!(opened, shown, "[review] {table} at {width} columns");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn auto_closes_the_list_in_a_narrow_tab_and_brings_it_back() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    for (width, shown) in [(119, false), (120, true), (90, false), (200, true)] {
        resize(&mut app, width).await?;
        assert_eq!(!list_views(&app).is_empty(), shown, "{width} columns");
        assert_eq!(list_drawn(&app), shown, "in the frame that's {width} wide");
        assert!(in_stream(&app), "{width} columns");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_by_hand_goes_before_the_config_through_a_resize() -> anyhow::Result<()> {
    let repo = two_changed();
    let mut app = review_with_list(&repo).await?;
    let steps = [
        (":review-sidebar hide<ret>", false, "hidden by the command"),
        (":review-sidebar show<ret>", true, "shown by the command"),
        ("<C-w>h<C-w>q", false, "hidden by closing its view"),
    ];
    for (keys, shown, step) in steps {
        run_until(&mut app, keys, |app| list_views(app).is_empty() != shown).await?;
        for width in [119, 120] {
            resize(&mut app, width).await?;
            assert_eq!(!list_views(&app).is_empty(), shown, "{step}, at {width}");
        }
    }
    Ok(())
}
