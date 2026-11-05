//! Copies of unsaved buffers, written when the editor goes down.
//!
//! [`write()`] gives each crash a dir of its own under the recovery dir, with one
//! plain UTF-8 file per buffer that any program opens. [`take_unnamed`] finds
//! the crashes the next start should name. Nothing here reads the copies back
//! or deletes them.
//!
//! It runs after a panic, so it trusts as little as it can: plain `std::fs`,
//! since the async runtime may be going down, and no trust in a document's
//! history, which a panic can leave empty.

use std::{
    fmt, fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, Write as _},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use helix_core::Rope;

use crate::{document::from_reader, Document};

/// The longest file name most file systems take, in bytes.
const NAME_MAX: usize = 255;
/// How much of a too-long name is kept, from its end.
const TAIL_LEN: usize = 200;
/// The file in a crash's dir that says a start has named it. No copy's name
/// starts with a `.`: a path's starts with `%`, a drive letter, or its hash.
const SEEN: &str = ".seen";

/// What [`write()`] wrote into a crash's dir, and what it couldn't.
#[derive(Debug)]
pub struct Report {
    pub dir: PathBuf,
    pub written: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, io::Error)>,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unsaved buffers were written to {}", self.dir.display())?;
        for (path, err) in &self.failed {
            write!(f, "\nCouldn't write {}: {err}", path.display())?;
        }
        Ok(())
    }
}

/// Writes the buffers among `docs` that hold unsaved work into a new dir under
/// `recovery_dir`, made if it's missing. `None` when no buffer needs a copy,
/// and then no dir is made.
pub fn write<'a>(
    docs: impl IntoIterator<Item = &'a Document>,
    recovery_dir: &Path,
) -> io::Result<Option<Report>> {
    let docs: Vec<_> = docs.into_iter().filter(|doc| needs_copy(doc)).collect();
    if docs.is_empty() {
        return Ok(None);
    }

    fs::create_dir_all(recovery_dir)?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let dir = recovery_dir.join(format!("{secs}-{}", std::process::id()));
    create_private_dir(&dir)?;

    let mut report = Report {
        dir,
        written: Vec::new(),
        failed: Vec::new(),
    };
    for doc in docs {
        let path = report.dir.join(file_name(doc));
        match write_copy(&path, doc.text()) {
            Ok(()) => report.written.push(path),
            Err(err) => report.failed.push((path, err)),
        }
    }
    Ok(Some(report))
}

fn needs_copy(doc: &Document) -> bool {
    if doc.is_modified() {
        return true;
    }
    match doc.path() {
        Some(path) => !matches_file(doc, path),
        // WHY: a feature's own buffer, like a review's, holds text it made
        // itself, so only an edit makes it the human's work.
        None => doc.kind.is_none() && !is_blank(doc.text()),
    }
}

/// Whether `doc`'s text is what loading its file would give.
fn matches_file(doc: &Document, path: &Path) -> bool {
    match fs::File::open(path) {
        Ok(mut file) => {
            from_reader(&mut file, Some(doc.encoding())).is_ok_and(|(text, ..)| text == *doc.text())
        }
        // A missing file opens as a new buffer, holding a line ending.
        Err(err) if err.kind() == io::ErrorKind::NotFound => is_blank(doc.text()),
        Err(_) => false,
    }
}

/// Whether `text` is what a new buffer starts with: nothing, or a line ending.
fn is_blank(text: &Rope) -> bool {
    text.len_bytes() <= 2 && matches!(text.to_string().as_str(), "" | "\n" | "\r\n")
}

/// The buffer's path with each separator and `:` as `%`, as vim's `undodir`
/// names its files. A name too long for the file system keeps its end, behind
/// a hash of the whole path.
fn file_name(doc: &Document) -> String {
    match doc.path() {
        Some(path) => path_file_name(path),
        None => format!("scratch-{}", doc.id()),
    }
}

fn path_file_name(path: &Path) -> String {
    let name: String = path
        .to_string_lossy()
        .chars()
        .map(|c| {
            if std::path::is_separator(c) || c == ':' {
                '%'
            } else {
                c
            }
        })
        .collect();
    if name.len() <= NAME_MAX {
        return name;
    }

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    let mut start = name.len() - TAIL_LEN;
    while !name.is_char_boundary(start) {
        start += 1;
    }
    format!("{:016x}-{}", hasher.finish(), &name[start..])
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    fs::create_dir(dir)
}

fn write_copy(path: &Path, text: &Rope) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = io::BufWriter::new(options.open(path)?);
    for chunk in text.chunks() {
        file.write_all(chunk.as_bytes())?;
    }
    file.flush()
}

/// The crash dirs under `recovery_dir` that no start has named yet, oldest
/// first, each marked as named so the next call leaves it out. A dir whose mark
/// can't be written is named again next time.
pub fn take_unnamed(recovery_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(recovery_dir) else {
        return Vec::new();
    };
    let mut crashes: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|dir| dir.is_dir() && !dir.join(SEEN).exists())
        .collect();
    crashes.sort();
    for dir in &crashes {
        let _ = fs::File::create(dir.join(SEEN));
    }
    crashes
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arc_swap::ArcSwap;
    use helix_core::{syntax, Selection, Transaction};

    use super::*;
    use crate::{editor::Config, DocumentId, ViewId};

    fn config() -> Arc<ArcSwap<Config>> {
        Arc::new(ArcSwap::new(Arc::new(Config::default())))
    }

    fn loader() -> Arc<ArcSwap<syntax::Loader>> {
        Arc::new(ArcSwap::from_pointee(syntax::Loader::default()))
    }

    fn open(path: &Path, id: usize) -> Document {
        let mut doc = Document::open(path, None, false, config(), loader()).unwrap();
        doc.id = DocumentId::new(id);
        doc
    }

    fn scratch(text: &str, id: usize) -> Document {
        let mut doc = Document::from(Rope::from(text), None, config(), loader());
        doc.id = DocumentId::new(id);
        doc
    }

    fn insert_at_start(doc: &mut Document, text: &str) {
        let view = ViewId::default();
        doc.set_selection(view, Selection::point(0));
        let transaction = Transaction::change(doc.text(), [(0, 0, Some(text.into()))].into_iter());
        doc.apply(&transaction, view);
    }

    fn copies(report: &Report) -> Vec<(String, String)> {
        let mut copies: Vec<_> = report
            .written
            .iter()
            .map(|path| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, fs::read_to_string(path).unwrap())
            })
            .collect();
        copies.sort();
        copies
    }

    #[test]
    fn writes_only_the_buffers_that_differ_from_their_files() {
        let tmp = tempfile::tempdir().unwrap();
        let file = |rel: &str, text: &str| {
            let path = tmp.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        };

        let mut edited = open(&file("edited.txt", "one\n"), 1);
        insert_at_start(&mut edited, "new ");
        let untouched = open(&file("untouched.txt", "two\n"), 2);
        // Stands in for a history a panic left empty: nothing reads as
        // modified, yet the text isn't the file's.
        let mut lying = open(&file("lying.txt", "three\n"), 3);
        lying.set_path(Some(&file("other.txt", "four\n")));
        let mut same_a = open(&file("a/same.txt", "five\n"), 4);
        insert_at_start(&mut same_a, "a ");
        let mut same_b = open(&file("b/same.txt", "six\n"), 5);
        insert_at_start(&mut same_b, "b ");
        let new_file = open(&tmp.path().join("missing.txt"), 6);
        let notes = scratch("notes\r\n", 7);
        let blank = scratch("\n", 8);

        let recovery_dir = tmp.path().join("data/recovery");
        let docs = [
            &edited, &untouched, &lying, &same_a, &same_b, &new_file, &notes, &blank,
        ];
        let report = write(docs, &recovery_dir).unwrap().unwrap();

        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(report.dir.parent(), Some(recovery_dir.as_path()));
        let copies = copies(&report);
        let texts: Vec<_> = copies.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(
            texts,
            ["a five\n", "b six\n", "new one\n", "three\n", "notes\r\n"]
        );
        assert!(copies[0].0.ends_with("%a%same.txt"), "{}", copies[0].0);
        assert!(copies[1].0.ends_with("%b%same.txt"), "{}", copies[1].0);
        assert_eq!(copies[4].0, "scratch-7");
    }

    #[test]
    fn writes_nothing_and_makes_no_dir_when_no_buffer_needs_a_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("saved.txt");
        fs::write(&path, "saved\n").unwrap();
        let saved = open(&path, 1);
        let blank = scratch("", 2);

        let recovery_dir = tmp.path().join("recovery");
        assert!(write([&saved, &blank], &recovery_dir).unwrap().is_none());
        assert!(!recovery_dir.exists());
    }

    #[test]
    fn names_a_copy_after_the_path_and_cuts_a_name_that_is_too_long() {
        assert_eq!(
            path_file_name(Path::new("/home/u/src/main.rs")),
            "%home%u%src%main.rs"
        );

        let long = format!("/{}/main.rs", "é".repeat(150));
        let name = path_file_name(Path::new(&long));
        assert!(name.len() <= NAME_MAX, "{} bytes", name.len());
        let (hash, tail) = name.split_once('-').unwrap();
        assert_eq!(hash.len(), 16);
        assert!(tail.ends_with("éé%main.rs"), "{tail}");

        let other = format!("/x{}/main.rs", "é".repeat(150));
        assert_ne!(path_file_name(Path::new(&other)), name);
    }

    #[cfg(unix)]
    #[test]
    fn keeps_the_copies_private() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let report = write([&scratch("secret\n", 1)], tmp.path())
            .unwrap()
            .unwrap();

        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&report.dir), 0o700);
        assert_eq!(mode(&report.written[0]), 0o600);
    }

    #[test]
    fn take_unnamed_names_each_crash_once() {
        let tmp = tempfile::tempdir().unwrap();
        let recovery_dir = tmp.path().join("recovery");
        assert!(take_unnamed(&recovery_dir).is_empty(), "a missing dir");
        fs::create_dir(&recovery_dir).unwrap();
        assert!(take_unnamed(&recovery_dir).is_empty(), "an empty dir");

        let first = write([&scratch("one\n", 1)], &recovery_dir)
            .unwrap()
            .unwrap();
        assert_eq!(
            take_unnamed(&recovery_dir),
            std::slice::from_ref(&first.dir)
        );
        assert!(take_unnamed(&recovery_dir).is_empty(), "only seen crashes");

        let second = recovery_dir.join("0-1");
        fs::create_dir(&second).unwrap();
        fs::write(second.join("scratch-1"), "two\n").unwrap();
        assert_eq!(take_unnamed(&recovery_dir), [second]);
        assert!(first.dir.exists(), "naming deletes nothing");
    }
}
