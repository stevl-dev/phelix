use std::{collections::HashSet, fs::File, io::Write, path::Path, process::Command};

use tempfile::TempDir;

use crate::git;
use crate::{ChangeKind, FileChange};

fn exec_git_cmd(args: &str, git_dir: &Path) {
    exec_git(&args.split_whitespace().collect::<Vec<_>>(), git_dir);
}

fn exec_git(args: &[&str], git_dir: &Path) {
    let res = Command::new("git")
        .arg("-C")
        .arg(git_dir) // execute the git command in this directory
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
        .env("GIT_TERMINAL_PROMPT", "false")
        .env("GIT_AUTHOR_DATE", "2000-01-01 00:00:00 +0000")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_AUTHOR_NAME", "author")
        .env("GIT_COMMITTER_DATE", "2000-01-02 00:00:00 +0000")
        .env("GIT_COMMITTER_EMAIL", "committer@example.com")
        .env("GIT_COMMITTER_NAME", "committer")
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_0", "false")
        .env("GIT_CONFIG_KEY_1", "init.defaultBranch")
        .env("GIT_CONFIG_VALUE_1", "main")
        .output()
        .unwrap_or_else(|_| panic!("`git {args:?}` failed"));
    if !res.status.success() {
        println!("{}", String::from_utf8_lossy(&res.stdout));
        eprintln!("{}", String::from_utf8_lossy(&res.stderr));
        panic!("`git {args:?}` failed (see output above)")
    }
}

fn create_commit(repo: &Path, add_modified: bool) {
    if add_modified {
        exec_git_cmd("add -A", repo);
    }
    exec_git_cmd("commit -m message", repo);
}

fn empty_git_repo() -> TempDir {
    let tmp = tempfile::tempdir().expect("create temp dir for git testing");
    exec_git_cmd("init", tmp.path());
    exec_git_cmd("config user.email test@helix.org", tmp.path());
    exec_git_cmd("config user.name helix-test", tmp.path());
    tmp
}

#[test]
fn missing_file() {
    let temp_git = empty_git_repo();
    let file = temp_git.path().join("file.txt");
    File::create(&file).unwrap().write_all(b"foo").unwrap();

    assert!(git::get_diff_base(&file, true).is_err());
}

#[test]
fn unmodified_file() {
    let temp_git = empty_git_repo();
    let file = temp_git.path().join("file.txt");
    let contents = b"foo".as_slice();
    File::create(&file).unwrap().write_all(contents).unwrap();
    create_commit(temp_git.path(), true);
    assert_eq!(
        git::get_diff_base(&file, true).unwrap(),
        Vec::from(contents)
    );
}

#[test]
fn modified_file() {
    let temp_git = empty_git_repo();
    let file = temp_git.path().join("file.txt");
    let contents = b"foo".as_slice();
    File::create(&file).unwrap().write_all(contents).unwrap();
    create_commit(temp_git.path(), true);
    File::create(&file).unwrap().write_all(b"bar").unwrap();

    assert_eq!(
        git::get_diff_base(&file, true).unwrap(),
        Vec::from(contents)
    );
}

/// Test that `get_file_head` does not return content for a directory.
/// This is important to correctly cover cases where a directory is removed and replaced by a file.
/// If the contents of the directory object were returned a diff between a path and the directory children would be produced.
#[test]
fn directory() {
    let temp_git = empty_git_repo();
    let dir = temp_git.path().join("file.txt");
    std::fs::create_dir(&dir).expect("");
    let file = dir.join("file.txt");
    let contents = b"foo".as_slice();
    File::create(file).unwrap().write_all(contents).unwrap();

    create_commit(temp_git.path(), true);

    std::fs::remove_dir_all(&dir).unwrap();
    File::create(&dir).unwrap().write_all(b"bar").unwrap();
    assert!(git::get_diff_base(&dir, true).is_err());
}

/// Test that `get_diff_base` resolves symlinks so that the same diff base is
/// used as the target file.
///
/// This is important to correctly cover cases where a symlink is removed and
/// replaced by a file. If the contents of the symlink object were returned
/// a diff between a literal file path and the actual file content would be
/// produced (bad ui).
#[cfg(any(unix, windows))]
#[test]
fn symlink() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(not(unix))]
    use std::os::windows::fs::symlink_file as symlink;

    let temp_git = empty_git_repo();
    let file = temp_git.path().join("file.txt");
    let contents = Vec::from(b"foo");
    File::create(&file).unwrap().write_all(&contents).unwrap();
    let file_link = temp_git.path().join("file_link.txt");

    symlink("file.txt", &file_link).unwrap();
    create_commit(temp_git.path(), true);

    assert_eq!(git::get_diff_base(&file_link, true).unwrap(), contents);
    assert_eq!(git::get_diff_base(&file, true).unwrap(), contents);
}

/// Test that `get_diff_base` returns content when the file is a symlink to
/// another file that is in a git repo, but the symlink itself is not.
#[cfg(any(unix, windows))]
#[test]
fn symlink_to_git_repo() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(not(unix))]
    use std::os::windows::fs::symlink_file as symlink;

    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let temp_git = empty_git_repo();

    let file = temp_git.path().join("file.txt");
    let contents = Vec::from(b"foo");
    File::create(&file).unwrap().write_all(&contents).unwrap();
    create_commit(temp_git.path(), true);

    let file_link = temp_dir.path().join("file_link.txt");
    symlink(&file, &file_link).unwrap();

    assert_eq!(git::get_diff_base(&file_link, true).unwrap(), contents);
    assert_eq!(git::get_diff_base(&file, true).unwrap(), contents);
}

fn change_label(change: &FileChange) -> &'static str {
    match change {
        FileChange::Untracked { .. } => "untracked",
        FileChange::Added { .. } => "added",
        FileChange::Modified { .. } => "modified",
        FileChange::Conflict { .. } => "conflict",
        FileChange::Deleted { .. } => "deleted",
        FileChange::Renamed { .. } => "renamed",
    }
}

#[test]
fn changed_files_include_staged_and_unstaged() {
    let temp_git = empty_git_repo();
    let repo = temp_git.path();
    for name in ["staged.txt", "unstaged.txt", "both.txt"] {
        File::create(repo.join(name))
            .unwrap()
            .write_all(b"a")
            .unwrap();
    }
    create_commit(repo, true);

    File::create(repo.join("staged.txt"))
        .unwrap()
        .write_all(b"b")
        .unwrap();
    File::create(repo.join("unstaged.txt"))
        .unwrap()
        .write_all(b"b")
        .unwrap();
    File::create(repo.join("both.txt"))
        .unwrap()
        .write_all(b"b")
        .unwrap();
    File::create(repo.join("added.txt"))
        .unwrap()
        .write_all(b"a")
        .unwrap();
    exec_git_cmd("add staged.txt both.txt added.txt", repo);
    File::create(repo.join("both.txt"))
        .unwrap()
        .write_all(b"c")
        .unwrap();
    File::create(repo.join("new.txt"))
        .unwrap()
        .write_all(b"a")
        .unwrap();

    let mut got = HashSet::new();
    git::for_each_changed_file(repo, true, |change| {
        let (kind, change) = change.unwrap();
        let kind = match kind {
            ChangeKind::Staged => "staged",
            ChangeKind::Unstaged => "unstaged",
        };
        got.insert(format!(
            "{kind} {} {}",
            change_label(&change),
            change.path().file_name().unwrap().to_string_lossy()
        ));
        true
    })
    .unwrap();

    let expected: HashSet<_> = [
        "staged modified staged.txt",
        "staged modified both.txt",
        "staged added added.txt",
        "unstaged modified unstaged.txt",
        "unstaged modified both.txt",
        "unstaged untracked new.txt",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(got, expected);
}

fn write(repo: &Path, name: &str, text: &str) {
    std::fs::write(repo.join(name), text).unwrap();
}

/// Each file `rev` changed: how, its name, and both sides.
fn commit_files(repo: &Path, rev: &str) -> Vec<(String, String, String, String)> {
    let (_, files) = git::commit::read(repo, rev, true).unwrap();
    files
        .into_iter()
        .map(|file| {
            let name = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
            let how = match &file.change {
                FileChange::Renamed { from_path, .. } => {
                    format!("renamed from {}", name(from_path))
                }
                change => change_label(change).to_string(),
            };
            (
                how,
                name(file.change.path()),
                String::from_utf8(file.base).unwrap(),
                String::from_utf8(file.source).unwrap(),
            )
        })
        .collect()
}

fn file(how: &str, name: &str, base: &str, source: &str) -> (String, String, String, String) {
    (how.into(), name.into(), base.into(), source.into())
}

#[test]
fn a_commit_has_both_sides_of_each_file_it_changed_in_path_order() {
    let temp_git = empty_git_repo();
    let repo = temp_git.path();
    let long = "one\ntwo\nthree\nfour\nfive\nsix\n";
    write(repo, "kept.txt", "old\n");
    write(repo, "gone.txt", "gone\n");
    write(repo, "before.txt", long);
    write(repo, "same.txt", "same\n");
    create_commit(repo, true);

    write(repo, "kept.txt", "new\n");
    write(repo, "added.txt", "added\n");
    std::fs::remove_file(repo.join("gone.txt")).unwrap();
    std::fs::rename(repo.join("before.txt"), repo.join("zz-after.txt")).unwrap();
    exec_git_cmd("add -A", repo);
    exec_git(
        &["commit", "-m", "Change four files.\n\nAnd say why.\n"],
        repo,
    );
    // WHY: the commit's sides come from its trees, not from the files.
    write(repo, "kept.txt", "newer\n");

    assert_eq!(
        commit_files(repo, "HEAD"),
        [
            file("added", "added.txt", "", "added\n"),
            file("deleted", "gone.txt", "gone\n", ""),
            file("modified", "kept.txt", "old\n", "new\n"),
            file("renamed from before.txt", "zz-after.txt", long, long),
        ]
    );

    let (commit, _) = git::commit::read(repo, "HEAD", true).unwrap();
    assert_eq!(commit.id.len(), 40);
    assert!(commit.id.starts_with(&commit.short_id));
    assert_eq!(commit.parents.len(), 1);
    assert_eq!(
        (commit.author.as_str(), commit.email.as_str()),
        ("author", "author@example.com")
    );
    assert_eq!(commit.date, "2000-01-01 00:00:00 +0000");
    assert_eq!(commit.message, "Change four files.\n\nAnd say why.");
}

#[test]
fn a_commit_is_read_by_any_name_of_it() {
    let temp_git = empty_git_repo();
    let repo = temp_git.path();
    write(repo, "file.txt", "one\n");
    create_commit(repo, true);
    write(repo, "file.txt", "two\n");
    create_commit(repo, true);
    write(repo, "file.txt", "three\n");
    create_commit(repo, true);

    let second = [file("modified", "file.txt", "one\n", "two\n")];
    assert_eq!(commit_files(repo, "HEAD~1"), second);
    let (commit, _) = git::commit::read(repo, "HEAD~1", true).unwrap();
    assert_eq!(commit_files(repo, &commit.short_id), second);

    let err = git::commit::read(repo, "nowhere", true).err().unwrap();
    assert_eq!(err.to_string(), "nowhere names no commit");
}

#[test]
fn a_root_commit_adds_every_file() {
    let temp_git = empty_git_repo();
    let repo = temp_git.path();
    write(repo, "file.txt", "one\n");
    create_commit(repo, true);

    assert_eq!(
        commit_files(repo, "HEAD"),
        [file("added", "file.txt", "", "one\n")]
    );
    let (commit, _) = git::commit::read(repo, "HEAD", true).unwrap();
    assert!(commit.parents.is_empty());
}

#[test]
fn a_merge_has_its_changes_against_its_first_parent() {
    let temp_git = empty_git_repo();
    let repo = temp_git.path();
    write(repo, "ours.txt", "one\n");
    write(repo, "theirs.txt", "one\n");
    create_commit(repo, true);
    exec_git_cmd("branch side", repo);
    write(repo, "ours.txt", "two\n");
    create_commit(repo, true);
    exec_git_cmd("checkout -q side", repo);
    write(repo, "theirs.txt", "two\n");
    create_commit(repo, true);
    exec_git_cmd("checkout -q -", repo);
    exec_git_cmd("merge -q --no-edit side", repo);

    assert_eq!(
        commit_files(repo, "HEAD"),
        [file("modified", "theirs.txt", "one\n", "two\n")]
    );
    let (commit, _) = git::commit::read(repo, "HEAD", true).unwrap();
    assert_eq!(commit.parents.len(), 2);
}

#[test]
fn a_file_whose_mode_alone_changed_has_two_equal_sides() {
    let temp_git = empty_git_repo();
    let repo = temp_git.path();
    write(repo, "run.sh", "echo\n");
    create_commit(repo, true);
    exec_git_cmd("update-index --chmod=+x run.sh", repo);
    create_commit(repo, false);

    assert_eq!(
        commit_files(repo, "HEAD"),
        [file("modified", "run.sh", "echo\n", "echo\n")]
    );
}

#[test]
fn both_sides_of_a_commit_take_the_checkouts_line_endings() {
    let temp_git = empty_git_repo();
    let repo = temp_git.path();
    exec_git_cmd("config core.autocrlf true", repo);
    write(repo, "file.txt", "one\r\ntwo\r\n");
    create_commit(repo, true);
    write(repo, "file.txt", "one\r\nTWO\r\n");
    create_commit(repo, true);

    assert_eq!(
        commit_files(repo, "HEAD"),
        [file(
            "modified",
            "file.txt",
            "one\r\ntwo\r\n",
            "one\r\nTWO\r\n"
        )]
    );
}

#[test]
fn a_commit_whose_parent_a_shallow_clone_lacks_says_so() {
    let temp_git = empty_git_repo();
    let repo = temp_git.path();
    write(repo, "file.txt", "one\n");
    create_commit(repo, true);
    write(repo, "file.txt", "two\n");
    create_commit(repo, true);
    let clone = tempfile::tempdir().unwrap();
    let url = format!("file://{}", repo.display());
    exec_git(&["clone", "-q", "--depth", "1", &url, "."], clone.path());

    let err = git::commit::read(clone.path(), "HEAD", true).err().unwrap();
    let (commit, _) = git::commit::read(repo, "HEAD", true).unwrap();
    assert_eq!(
        err.to_string(),
        format!("{}'s parent isn't in this clone", commit.short_id)
    );
}
