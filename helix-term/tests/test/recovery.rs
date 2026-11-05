use super::*;

use std::{fs, panic::AssertUnwindSafe};

use futures_util::FutureExt;
use helix_core::Transaction;

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_on_the_event_loop_writes_the_unsaved_buffers() -> anyhow::Result<()> {
    let file = tempfile::NamedTempFile::new()?;
    fs::write(file.path(), "one\n")?;
    let data_dir = tempfile::tempdir()?;
    let recovery_dir = data_dir.path().join("recovery");

    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .with_recovery_dir(&recovery_dir)
        .build()?;

    let (view, doc) = helix_view::current!(app.editor);
    let insert = Transaction::change(doc.text(), [(0, 0, Some("edited ".into()))].into_iter());
    doc.apply(&insert, view.id);
    helix_term::job::dispatch(|_, _| panic!("a job panics")).await;

    let looped = AssertUnwindSafe(helpers::run_event_loop_until_idle(&mut app))
        .catch_unwind()
        .await;

    assert!(looped.is_err(), "the panic should reach the caller");
    let crash_dirs: Vec<_> = fs::read_dir(&recovery_dir)?.collect::<Result<_, _>>()?;
    assert_eq!(crash_dirs.len(), 1);
    let copies: Vec<_> = fs::read_dir(crash_dirs[0].path())?.collect::<Result<_, _>>()?;
    assert_eq!(copies.len(), 1);
    assert_eq!(fs::read_to_string(copies[0].path())?, "edited one\n");
    Ok(())
}
