use super::*;

use std::{fs, panic::AssertUnwindSafe, path::Path};

use futures_util::FutureExt;
use helix_core::Transaction;
use helix_term::application::Application;
use tempfile::{NamedTempFile, TempDir};

struct Setup {
    app: Application,
    recovery_dir: std::path::PathBuf,
    _file: NamedTempFile,
    _data_dir: TempDir,
}

/// An app on a file holding `one\n`, with `edited ` typed at its start if
/// `edit` says so, and a recovery dir that doesn't exist yet.
async fn setup(edit: bool) -> anyhow::Result<Setup> {
    let file = NamedTempFile::new()?;
    fs::write(file.path(), "one\n")?;
    let data_dir = tempfile::tempdir()?;
    let recovery_dir = data_dir.path().join("recovery");

    let mut app = helpers::AppBuilder::new()
        .with_file(file.path(), None)
        .with_recovery_dir(&recovery_dir)
        .build()?;
    if edit {
        let (view, doc) = helix_view::current!(app.editor);
        let insert = Transaction::change(doc.text(), [(0, 0, Some("edited ".into()))].into_iter());
        doc.apply(&insert, view.id);
    }

    Ok(Setup {
        app,
        recovery_dir,
        _file: file,
        _data_dir: data_dir,
    })
}

/// The text of each copy in the one crash's dir under `recovery_dir`.
fn recovered(recovery_dir: &Path) -> anyhow::Result<Vec<String>> {
    let crash_dirs: Vec<_> = fs::read_dir(recovery_dir)?.collect::<Result<_, _>>()?;
    assert_eq!(crash_dirs.len(), 1);
    fs::read_dir(crash_dirs[0].path())?
        .map(|copy| Ok(fs::read_to_string(copy?.path())?))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_on_the_event_loop_writes_the_unsaved_buffers() -> anyhow::Result<()> {
    let mut setup = setup(true).await?;
    helix_term::job::dispatch(|_, _| panic!("a job panics")).await;

    let looped = AssertUnwindSafe(helpers::run_event_loop_until_idle(&mut setup.app))
        .catch_unwind()
        .await;

    assert!(looped.is_err(), "the panic should reach the caller");
    assert_eq!(recovered(&setup.recovery_dir)?, ["edited one\n"]);
    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_signal_that_ends_the_editor_writes_the_unsaved_buffers() -> anyhow::Result<()> {
    use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};

    for (name, signal) in [("SIGTERM", SIGTERM), ("SIGINT", SIGINT), ("SIGHUP", SIGHUP)] {
        let mut setup = setup(true).await?;

        assert!(!setup.app.handle_signals(signal).await, "{name} should end");
        assert_eq!(recovered(&setup.recovery_dir)?, ["edited one\n"], "{name}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn losing_the_terminal_writes_the_unsaved_buffers_and_exits_129() -> anyhow::Result<()> {
    let mut setup = setup(true).await?;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut input = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    tx.send(Err(std::io::Error::other("the terminal is gone")))?;

    assert_eq!(setup.app.run(&mut input).await?, 129);
    assert_eq!(recovered(&setup.recovery_dir)?, ["edited one\n"]);
    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn ending_with_nothing_unsaved_makes_no_recovery_dir() -> anyhow::Result<()> {
    let mut setup = setup(false).await?;

    assert!(
        !setup
            .app
            .handle_signals(signal_hook::consts::signal::SIGTERM)
            .await
    );
    assert!(!setup.recovery_dir.exists());
    Ok(())
}
