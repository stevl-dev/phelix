//! Reads a commit and the files it changed, for the review stream.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use gix::bstr::{BStr, ByteSlice};
use gix::filter::plumbing::driver::apply::Delay;
use gix::object::tree::diff::ChangeDetached;
use gix::objs::tree::EntryMode;
use gix::{ObjectId, Repository};

use super::{absolute_path, open_repo, renames};
use crate::commit::{Commit, CommitFile};
use crate::FileChange;

/// The commit `rev` names in the repository that holds `dir`, and its changed
/// files in path order.
pub fn read(dir: &Path, rev: &str, trust_full: bool) -> Result<(Commit, Vec<CommitFile>)> {
    let repo = open_repo(dir, trust_full)
        .context("failed to open git repo")?
        .to_thread_local();
    let work_dir = repo
        .workdir()
        .context("the repository has no working tree")?
        .to_path_buf();
    let commit = repo
        .rev_parse_single(rev)
        .ok()
        .and_then(|id| id.object().ok()?.peel_to_commit().ok())
        .with_context(|| format!("{rev} names no commit"))?;
    let short_id = commit.id().shorten_or_id().to_string();

    let parents: Vec<_> = commit.parent_ids().collect();
    let base = match parents.first() {
        Some(parent) => {
            let parent = parent
                .object()
                .ok()
                .and_then(|object| object.try_into_commit().ok())
                .with_context(|| format!("{short_id}'s parent isn't in this clone"))?;
            Some(parent.tree()?)
        }
        None => None,
    };
    let options = gix::diff::Options::default().with_rewrites(Some(renames()));
    let changes = repo.diff_tree_to_tree(base.as_ref(), &commit.tree()?, options)?;

    let mut blobs = Blobs {
        repo: &repo,
        pipeline: repo.filter_pipeline(None)?.0,
    };
    let mut files = Vec::new();
    for change in changes {
        let path = |location: &BStr| absolute_path(&work_dir, location);
        let file = match change {
            ChangeDetached::Addition {
                location,
                entry_mode,
                id,
                ..
            } if !entry_mode.is_tree() => CommitFile {
                change: FileChange::Added {
                    path: path(location.as_bstr())?,
                },
                base: Vec::new(),
                source: blobs.text(id, entry_mode, location.as_bstr())?,
            },
            ChangeDetached::Deletion {
                location,
                entry_mode,
                id,
                ..
            } if !entry_mode.is_tree() => CommitFile {
                change: FileChange::Deleted {
                    path: path(location.as_bstr())?,
                },
                base: blobs.text(id, entry_mode, location.as_bstr())?,
                source: Vec::new(),
            },
            ChangeDetached::Modification {
                location,
                previous_entry_mode,
                previous_id,
                entry_mode,
                id,
            } if !(previous_entry_mode.is_tree() && entry_mode.is_tree()) => CommitFile {
                change: FileChange::Modified {
                    path: path(location.as_bstr())?,
                },
                base: blobs.text(previous_id, previous_entry_mode, location.as_bstr())?,
                source: blobs.text(id, entry_mode, location.as_bstr())?,
            },
            ChangeDetached::Rewrite {
                source_location,
                source_entry_mode,
                source_id,
                entry_mode,
                id,
                location,
                copy,
                ..
            } => CommitFile {
                change: match copy {
                    true => FileChange::Added {
                        path: path(location.as_bstr())?,
                    },
                    false => FileChange::Renamed {
                        from_path: path(source_location.as_bstr())?,
                        to_path: path(location.as_bstr())?,
                    },
                },
                base: match copy {
                    true => Vec::new(),
                    false => blobs.text(source_id, source_entry_mode, source_location.as_bstr())?,
                },
                source: blobs.text(id, entry_mode, location.as_bstr())?,
            },
            _ => continue,
        };
        files.push(file);
    }
    // INVARIANT: path order, component by component, as `visible_changes`
    // sorts. The review's file list walks the files as a tree.
    files.sort_by(|a, b| a.change.path().cmp(b.change.path()));

    let author = commit.author()?;
    let date = author
        .time()
        .map(|time| time.format_or_unix(gix::date::time::format::ISO8601))
        .unwrap_or_default();
    let commit = Commit {
        id: commit.id().to_string(),
        short_id,
        parents: parents
            .iter()
            .map(|parent| parent.shorten_or_id().to_string())
            .collect(),
        author: author.name.to_str_lossy().trim().to_string(),
        email: author.email.to_str_lossy().trim().to_string(),
        date,
        message: commit
            .message_raw_sloppy()
            .to_str_lossy()
            .trim_end()
            .to_string(),
    };
    Ok((commit, files))
}

/// Reads the text of a tree's entries.
struct Blobs<'repo> {
    repo: &'repo Repository,
    pipeline: gix::filter::Pipeline<'repo>,
}

impl Blobs<'_> {
    /// The text of a file, converted as a checkout would, or a symlink's
    /// target. Nothing for any other entry.
    fn text(&mut self, id: ObjectId, mode: EntryMode, location: &BStr) -> Result<Vec<u8>> {
        if !mode.is_blob_or_symlink() {
            return Ok(Vec::new());
        }
        let data = self.repo.find_object(id)?.detach().data;
        if mode.is_link() {
            return Ok(data);
        }
        let mut converted = self
            .pipeline
            .convert_to_worktree(&data, location, Delay::Forbid)?;
        let mut text = Vec::with_capacity(data.len());
        converted.read_to_end(&mut text)?;
        Ok(text)
    }
}
