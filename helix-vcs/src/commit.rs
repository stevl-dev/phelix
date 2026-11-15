//! A commit and the files it changed, and the commits `HEAD` reaches, for the
//! review. Builds without the `git` feature get an error, like the diff
//! providers.

use crate::FileChange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The full hash.
    pub id: String,
    pub short_id: String,
    /// The parents' short hashes. More than one for a merge.
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// The author's date, as `2000-01-01 00:00:00 +0000`.
    pub date: String,
    /// The whole message, without the line endings after it.
    pub message: String,
}

impl Commit {
    /// The message's first line.
    pub fn subject(&self) -> &str {
        self.message.lines().next().unwrap_or_default()
    }
}

/// A file a commit changed. A merge's are against its first parent, and a
/// root commit's are all added.
pub struct CommitFile {
    /// With paths under the work tree, as the status walk's are.
    pub change: FileChange,
    /// The parent's text and the commit's, as worktree text like the `HEAD`
    /// base. Both are empty for an entry that isn't a file or a symlink, like
    /// a submodule.
    pub base: Vec<u8>,
    pub source: Vec<u8>,
}

#[cfg(feature = "git")]
pub use crate::git::commit::{read, walk};

#[cfg(not(feature = "git"))]
pub fn read(
    _dir: &std::path::Path,
    _rev: &str,
    _trust_full: bool,
) -> anyhow::Result<(Commit, Vec<CommitFile>)> {
    anyhow::bail!("No diff support compiled in")
}

#[cfg(not(feature = "git"))]
pub fn walk(
    _dir: &std::path::Path,
    _trust_full: bool,
    _f: impl FnMut(Commit) -> bool,
) -> anyhow::Result<()> {
    anyhow::bail!("No diff support compiled in")
}
