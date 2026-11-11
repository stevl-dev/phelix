//! `:stage-hunk` and `:unstage-hunk` write the hunks the selection touches in
//! the review to the index, whole, through `git`. Ported from diff review,
//! which took the lines from a file's buffer instead of the review's rows.
//!
//! `:stage-line` and `:unstage-line` write only the changed rows the selection
//! covers. Their diffs are the index against the source and `HEAD` against the
//! index, not the review's own, so each row's line is carried to that diff
//! first. The rules are in `fork/stage-lines.md`.

use std::collections::HashMap;
use std::ops::Range;

use crate::view::{layout, Picked};
use anyhow::{anyhow, bail, Context as _};
use helix_core::command_line::Args;
use helix_core::encoding::Encoding;
use helix_core::line_ending::{auto_detect_line_ending, get_line_ending};
use helix_core::{LineEnding, Rope, RopeSlice};
use helix_loader::workspace_trust::TrustQuery;
use helix_vcs::{index, FileChange, Hunk};
use helix_view::current_ref;
use helix_view::document::{from_reader, to_writer};

use super::{open_stream, read_changes};
use helix_term::compositor;
use helix_term::ui::PromptEvent;

pub(crate) fn stage_hunk(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let staged = write_picked(cx, "Staged", changes, |file, trust_full| {
        let path = file.change.path();
        let index_bytes =
            index::get_index_base(path, trust_full).map_err(|err| anyhow!("{err:#}"))?;
        let (index_text, bom) = decode(file.encoding, index_bytes.as_deref())?;
        let hunks = hunks(&index_text, file.source)?;
        let picked: Vec<&Hunk> = hunks
            .iter()
            .filter(|hunk| under(&hunk.after, &file.lines))
            .collect();
        if picked.is_empty() {
            return Ok(0);
        }
        let staged = splice(
            &index_text,
            picked
                .iter()
                .map(|hunk| (hunk.before.clone(), vec![slice(file.source, &hunk.after)])),
            ending(&index_text, file.source),
        );
        // WHY: staging all of a deleted file removes it from the index.
        let remove = staged.len_bytes() == 0 && !path.exists();
        write(file, (!remove).then_some((&staged, bom)), trust_full)?;
        Ok(picked.len())
    })?;
    if staged == 0 {
        bail!("There are no unstaged changes under any selection");
    }
    Ok(())
}

pub(crate) fn stage_line(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let staged = write_picked(cx, "Staged", line_count, |file, trust_full| {
        let path = file.change.path();
        let index_bytes =
            index::get_index_base(path, trust_full).map_err(|err| anyhow!("{err:#}"))?;
        let (index_text, bom) = decode(file.encoding, index_bytes.as_deref())?;
        let (staged, count) = staged_lines(file, &index_text)?;
        if count == 0 {
            return Ok(0);
        }
        // WHY: staging all of a deleted file removes it from the index.
        let remove = staged.len_bytes() == 0 && !path.exists();
        write(file, (!remove).then_some((&staged, bom)), trust_full)?;
        Ok(count)
    })?;
    if staged == 0 {
        bail!("There are no unstaged changes under any selection");
    }
    Ok(())
}

pub(crate) fn unstage_hunk(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let providers = cx.editor.diff_providers.clone();
    let unstaged = write_picked(cx, "Unstaged", changes, |file, trust_full| {
        let path = file.change.path();
        let head_bytes = providers.get_diff_base(path, trust_full);
        let index_bytes =
            index::get_index_base(path, trust_full).map_err(|err| anyhow!("{err:#}"))?;
        let (head, _) = decode(file.encoding, head_bytes.as_deref())?;
        let (index_text, bom) = decode(file.encoding, index_bytes.as_deref())?;
        // WHY: the rows show the source, so their lines go through the
        // `HEAD`-to-source diff to `HEAD`'s, where staged hunks start.
        let shown = hunks(&head, file.source)?;
        let lines: Vec<(usize, usize)> = file
            .lines
            .iter()
            .map(|&lines| map_to_before(&shown, lines))
            .collect();
        let staged = hunks(&head, &index_text)?;
        let picked: Vec<&Hunk> = staged
            .iter()
            .filter(|hunk| under(&hunk.before, &lines))
            .collect();
        if picked.is_empty() {
            return Ok(0);
        }
        let unstaged = splice(
            &index_text,
            picked
                .iter()
                .map(|hunk| (hunk.after.clone(), vec![slice(&head, &hunk.before)])),
            ending(&index_text, &head),
        );
        // WHY: unstaging all of a file that `HEAD` doesn't have removes it
        // from the index.
        let remove = unstaged.len_bytes() == 0 && head_bytes.is_none();
        write(file, (!remove).then_some((&unstaged, bom)), trust_full)?;
        Ok(picked.len())
    })?;
    if unstaged == 0 {
        bail!("There are no staged changes under any selection");
    }
    Ok(())
}

pub(crate) fn unstage_line(
    cx: &mut compositor::Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let providers = cx.editor.diff_providers.clone();
    let unstaged = write_picked(cx, "Unstaged", line_count, |file, trust_full| {
        // WHY: `HEAD` has a renamed file at its old path, where the review
        // read its base.
        let head_path = match file.change {
            FileChange::Renamed { from_path, .. } => from_path,
            change => change.path(),
        };
        let head_bytes = providers.get_diff_base(head_path, trust_full);
        let index_bytes = index::get_index_base(file.change.path(), trust_full)
            .map_err(|err| anyhow!("{err:#}"))?;
        let (head, _) = decode(file.encoding, head_bytes.as_deref())?;
        let (index_text, bom) = decode(file.encoding, index_bytes.as_deref())?;
        let (unstaged, count) = unstaged_lines(file, &head, &index_text)?;
        if count == 0 {
            return Ok(0);
        }
        // WHY: unstaging all of a file that `HEAD` doesn't have removes it
        // from the index.
        let remove = unstaged.len_bytes() == 0 && head_bytes.is_none();
        write(file, (!remove).then_some((&unstaged, bom)), trust_full)?;
        Ok(count)
    })?;
    if unstaged == 0 {
        bail!("There are no staged changes under any selection");
    }
    Ok(())
}

/// Calls `write` for each file with hunks under the focused review's
/// selection, then reads the review again and says what `verb` did to how
/// many, as `counted` words it. Returns the changes written.
fn write_picked(
    cx: &mut compositor::Context,
    verb: &str,
    counted: fn(usize) -> String,
    write: impl Fn(&Picked, bool) -> anyhow::Result<usize>,
) -> anyhow::Result<usize> {
    let (view, doc) = current_ref!(cx.editor);
    let stream = open_stream(cx.editor)
        .filter(|stream| stream.doc == doc.id())
        .context("This buffer isn't a review. Start one with :review")?;
    let trust_full = cx
        .editor
        .workspace_trust
        .query(
            &helix_loader::find_workspace_in(&stream.root).0,
            TrustQuery::Git,
        )
        .is_trusted();
    if !trust_full {
        bail!("Staging runs git, which needs a trusted workspace. See :workspace-trust");
    }
    let rows: Vec<_> = doc
        .selection(view.id)
        .line_ranges(doc.text().slice(..))
        .collect();
    let mut written = 0;
    for file in stream.picked(&rows) {
        // WHY: a typable command shows only the outermost message of its
        // error, so a context would hide the reason.
        written += write(&file, trust_full).map_err(|err| {
            let path = helix_stdx::path::get_relative_path(file.change.path());
            anyhow!("{}: {err:#}", path.display())
        })?;
    }
    if written > 0 {
        let scope = stream.scope;
        read_changes(
            cx,
            scope,
            Some(format!("{verb} {}", counted(written))),
            None,
        );
    }
    Ok(written)
}

fn hunks(from: &Rope, to: &Rope) -> anyhow::Result<Vec<Hunk>> {
    layout::hunks(from, to).context("The file is too large to diff")
}

/// The text of a version of the file, and whether it has a byte order mark.
/// A missing version is empty.
fn decode(encoding: &'static Encoding, bytes: Option<&[u8]>) -> anyhow::Result<(Rope, bool)> {
    let (text, _, bom) = from_reader(&mut bytes.unwrap_or_default(), Some(encoding))?;
    Ok((text, bom))
}

/// Makes `text` the index version of the file, or removes the file from the
/// index when `text` is `None`.
fn write(file: &Picked, text: Option<(&Rope, bool)>, trust_full: bool) -> anyhow::Result<()> {
    let bytes = match text {
        Some((text, bom)) => {
            let mut bytes = Vec::new();
            helix_lsp::block_on(to_writer(&mut bytes, (file.encoding, bom), text))?;
            Some(bytes)
        }
        None => None,
    };
    index::set_index_text(file.change.path(), bytes.as_deref(), trust_full)
        .map_err(|err| anyhow!("{err:#}"))
}

fn changes(count: usize) -> String {
    match count {
        1 => "1 change".to_string(),
        _ => format!("{count} changes"),
    }
}

fn line_count(count: usize) -> String {
    match count {
        1 => "1 line".to_string(),
        _ => format!("{count} lines"),
    }
}

/// Whether the hunk side `lines` touches one of `ranges`, first and last lines,
/// or ends just above it. A removal has no lines, so it's under the line below.
pub(super) fn under(lines: &Range<u32>, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| lines.end as usize >= start && lines.start as usize <= end)
}

/// The `before` lines that the `after` lines `start..=end` map to through
/// `hunks`. A line inside a hunk maps to all of the hunk's `before` lines, and
/// an added line to the line its hunk comes before.
fn map_to_before(hunks: &[Hunk], (start, end): (usize, usize)) -> (usize, usize) {
    let map = |line: usize| {
        let preceding = hunks.partition_point(|hunk| hunk.after.start as usize <= line);
        let Some(hunk) = preceding.checked_sub(1).map(|i| &hunks[i]) else {
            return (line, line);
        };
        let (before, after) = (
            hunk.before.start as usize..hunk.before.end as usize,
            hunk.after.start as usize..hunk.after.end as usize,
        );
        if after.contains(&line) {
            (before.start, before.end.max(before.start + 1) - 1)
        } else {
            let line = line - after.end + before.end;
            (line, line)
        }
    };
    (map(start).0, map(end).1)
}

/// The rows of one file as lines of a command's diff: each shown line of the
/// diff's two sides, and whether the selection covers its row.
#[derive(Default)]
struct Rows {
    before: HashMap<usize, bool>,
    after: HashMap<usize, bool>,
}

/// A side of a diff.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Before,
    After,
}

/// The line of `hunks`' other side that `line` of their `from` side is. `None`
/// when the other side doesn't have it.
fn carry(hunks: &[Hunk], from: Side, line: usize) -> Option<usize> {
    let sides = |hunk: &Hunk| match from {
        Side::Before => (hunk.before.clone(), hunk.after.clone()),
        Side::After => (hunk.after.clone(), hunk.before.clone()),
    };
    let preceding = hunks.partition_point(|hunk| sides(hunk).0.start as usize <= line);
    let Some(hunk) = preceding.checked_sub(1).map(|i| &hunks[i]) else {
        return Some(line);
    };
    let (from, to) = sides(hunk);
    (line >= from.end as usize).then(|| line - from.end as usize + to.end as usize)
}

/// `index` with the changed rows of `file` that the selection covers staged,
/// and how many took effect.
fn staged_lines(file: &Picked, index: &Rope) -> anyhow::Result<(Rope, usize)> {
    // WHY: a removed row is a line of the review's base, which is `HEAD` in
    // an `all` review, and staging removes lines of the index.
    let to_index = hunks(file.base, index)?;
    let rows = Rows {
        before: carried(&file.removed, &to_index, Side::Before),
        after: file.added.iter().copied().collect(),
    };
    let hunks = hunks(index, file.source)?;
    Ok(take_lines((index, file.source), &hunks, Side::After, &rows))
}

/// `index` with the changed rows of `file` that the selection covers
/// unstaged, back to `head`'s lines, and how many took effect.
fn unstaged_lines(file: &Picked, head: &Rope, index: &Rope) -> anyhow::Result<(Rope, usize)> {
    // WHY: an added row is a line of the review's source, which is the file
    // in an `all` review, and unstaging removes lines of the index.
    let to_source = hunks(index, file.source)?;
    let rows = Rows {
        before: carried(&file.removed, &hunks(file.base, head)?, Side::Before),
        after: carried(&file.added, &to_source, Side::After),
    };
    let hunks = hunks(head, index)?;
    Ok(take_lines((head, index), &hunks, Side::Before, &rows))
}

fn carried(rows: &[(usize, bool)], hunks: &[Hunk], from: Side) -> HashMap<usize, bool> {
    rows.iter()
        .filter_map(|&(line, covered)| Some((carry(hunks, from, line)?, covered)))
        .collect()
}

/// The index with the covered lines of `hunks` moved `toward` a side of the
/// diff, and how many those are. The index is the other side. A hunk with no
/// covered line stays. In the others only a line shown in a row the selection
/// doesn't cover stays as the index has it, so a line with no row goes with
/// its hunk.
fn take_lines(
    (before, after): (&Rope, &Rope),
    hunks: &[Hunk],
    toward: Side,
    rows: &Rows,
) -> (Rope, usize) {
    let covered = |shown: &HashMap<usize, bool>, lines: &Range<u32>| {
        lines
            .clone()
            .filter(|&line| shown.get(&(line as usize)) == Some(&true))
            .count()
    };
    // WHY: a held line of the index's side stays in it, and a held line of
    // the other side stays out.
    let stays = |side: Side, shown: &HashMap<usize, bool>, line: u32| {
        let held = shown.get(&(line as usize)) == Some(&false);
        (side == toward) != held
    };
    let (index, other) = match toward {
        Side::After => (before, after),
        Side::Before => (after, before),
    };
    let mut count = 0;
    let mut pairs = Vec::new();
    for hunk in hunks {
        let taken = covered(&rows.before, &hunk.before) + covered(&rows.after, &hunk.after);
        if taken == 0 {
            continue;
        }
        count += taken;
        // INVARIANT: the review's order, removed lines above added ones.
        let removed = hunk
            .before
            .clone()
            .filter(|&line| stays(Side::Before, &rows.before, line))
            .map(|line| slice(before, &(line..line + 1)));
        let added = hunk
            .after
            .clone()
            .filter(|&line| stays(Side::After, &rows.after, line))
            .map(|line| slice(after, &(line..line + 1)));
        let replaced = match toward {
            Side::After => hunk.before.clone(),
            Side::Before => hunk.after.clone(),
        };
        pairs.push((replaced, removed.chain(added).collect()));
    }
    let text = splice(index, pairs.into_iter(), ending(index, other));
    (text, count)
}

fn slice<'a>(text: &'a Rope, lines: &Range<u32>) -> RopeSlice<'a> {
    let start = text.line_to_char(lines.start as usize);
    let end = text.line_to_char(lines.end as usize);
    text.slice(start..end)
}

/// The line ending a line with none gets when another line lands below it.
fn ending(index: &Rope, other: &Rope) -> LineEnding {
    auto_detect_line_ending(index)
        .or_else(|| auto_detect_line_ending(other))
        .unwrap_or(LineEnding::LF)
}

/// `index`, with each `(lines, text)` pair's lines replaced by its text. The
/// pairs are in order and don't overlap.
fn splice<'a>(
    index: &'a Rope,
    pairs: impl Iterator<Item = (Range<u32>, Vec<RopeSlice<'a>>)>,
    ending: LineEnding,
) -> Rope {
    let mut out = String::new();
    let mut unended = false;
    let mut push = |out: &mut String, text: RopeSlice| {
        if text.len_chars() == 0 {
            return;
        }
        // WHY: a last line with no line ending can land above another line,
        // and would join it.
        if unended {
            out.push_str(ending.as_str());
        }
        out.extend(text.chunks());
        unended = get_line_ending(&text).is_none();
    };
    let mut cursor = 0;
    for (replaced, text) in pairs {
        push(&mut out, slice(index, &(cursor..replaced.start)));
        for text in text {
            push(&mut out, text);
        }
        cursor = replaced.end;
    }
    push(&mut out, slice(index, &(cursor..index.len_lines() as u32)));
    Rope::from(out)
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use helix_core::{LineEnding, Rope};
    use helix_vcs::{FileChange, Hunk};

    use super::{
        carry, hunks, map_to_before, slice, splice, staged_lines, under, unstaged_lines, Side,
    };
    use crate::view::Picked;

    fn hunk(before: std::ops::Range<u32>, after: std::ops::Range<u32>) -> Hunk {
        Hunk { before, after }
    }

    #[test]
    fn splice_takes_only_the_picked_hunks() {
        let index = Rope::from("a\nb\nc\nd\n");
        let buffer = Rope::from("a\nB\nc\nD\ne\n");
        let hunks = hunks(&index, &buffer).unwrap();
        assert_eq!(hunks, [hunk(1..2, 1..2), hunk(3..4, 3..5)]);

        let staged = splice(
            &index,
            hunks[1..]
                .iter()
                .map(|hunk| (hunk.before.clone(), vec![slice(&buffer, &hunk.after)])),
            LineEnding::LF,
        );
        assert_eq!(staged, "a\nb\nc\nD\ne\n");
    }

    #[test]
    fn splice_handles_a_last_line_without_a_newline() {
        let index = Rope::from("a\nb");
        let buffer = Rope::from("a\nb\nc");
        let hunks = hunks(&index, &buffer).unwrap();
        let staged = splice(
            &index,
            hunks
                .iter()
                .map(|hunk| (hunk.before.clone(), vec![slice(&buffer, &hunk.after)])),
            LineEnding::LF,
        );
        assert_eq!(staged, "a\nb\nc");
    }

    /// What `write` makes of the index for the rows of a review of `base`
    /// against `source`, with the selection over the removed rows of the base
    /// lines `removed` and the added rows of the source lines `added`.
    fn with_rows(
        [base, source]: [&str; 2],
        removed: &[usize],
        added: &[usize],
        write: impl Fn(&Picked) -> (Rope, usize),
    ) -> (String, usize) {
        let (base, source) = (Rope::from(base), Rope::from(source));
        let shown = hunks(&base, &source).unwrap();
        let rows = |side: fn(&Hunk) -> std::ops::Range<u32>, covered: &[usize]| {
            shown
                .iter()
                .flat_map(side)
                .map(|line| (line as usize, covered.contains(&(line as usize))))
                .collect()
        };
        let change = FileChange::Modified {
            path: PathBuf::from("a.txt"),
        };
        let (text, count) = write(&Picked {
            change: &change,
            base: &base,
            source: &source,
            encoding: helix_core::encoding::UTF_8,
            lines: Vec::new(),
            removed: rows(|hunk| hunk.before.clone(), removed),
            added: rows(|hunk| hunk.after.clone(), added),
        });
        (text.to_string(), count)
    }

    fn stage(
        [base, index, source]: [&str; 3],
        removed: &[usize],
        added: &[usize],
    ) -> (String, usize) {
        with_rows([base, source], removed, added, |file| {
            staged_lines(file, &Rope::from(index)).unwrap()
        })
    }

    #[test]
    fn staging_lines_takes_the_covered_rows_and_holds_back_the_other_rows() {
        let same = |text: &'static str, source: &'static str| [text, text, source];
        type Case<'a> = (
            &'a str,
            [&'a str; 3],
            &'a [usize],
            &'a [usize],
            &'a str,
            usize,
        );
        let cases: &[Case] = &[
            (
                "an added row alone leaves the line it replaces",
                same("a\nb\nc\n", "a\nB\nc\n"),
                &[],
                &[1],
                "a\nb\nB\nc\n",
                1,
            ),
            (
                "a removed row alone adds nothing",
                same("a\nb\nc\n", "a\nB\nc\n"),
                &[1],
                &[],
                "a\nc\n",
                1,
            ),
            (
                "every row of a hunk is the whole hunk",
                same("a\nb\nc\n", "a\nB\nc\n"),
                &[1],
                &[1],
                "a\nB\nc\n",
                2,
            ),
            (
                "a hunk with no covered row stays",
                same("a\nb\nc\nd\ne\n", "A\nb\nc\nd\nE\n"),
                &[4],
                &[4],
                "a\nb\nc\nd\nE\n",
                2,
            ),
            (
                "the second of two added lines",
                same("a\n", "a\nx\ny\n"),
                &[],
                &[2],
                "a\ny\n",
                1,
            ),
            (
                "a line only the index has goes with its hunk",
                ["a\nb\nc\n", "a\nB\nc\n", "a\nB2\nc\n"],
                &[1],
                &[1],
                "a\nB2\nc\n",
                1,
            ),
            (
                "a row of a staged change has nothing to do",
                ["a\nb\nc\n", "a\nB\nc\n", "a\nB\nc\n"],
                &[1],
                &[1],
                "a\nB\nc\n",
                0,
            ),
            (
                "a row beside a staged change holds back its own line",
                ["a\nb\nc\nd\n", "a\nB\nc\nd\n", "a\nB\nc\nD\n"],
                &[],
                &[3],
                "a\nB\nc\nd\nD\n",
                1,
            ),
            (
                "a removed row of an `all` review is carried past a staged line",
                ["a\nc\nd\n", "a\nb\nc\nd\n", "a\nb\nc\n"],
                &[2],
                &[],
                "a\nb\nc\n",
                1,
            ),
            (
                "a last line with no line ending gets one above an added line",
                same("a\nb", "a\nb\nc\n"),
                &[],
                &[2],
                "a\nb\nc\n",
                1,
            ),
            (
                "and takes the index's own line ending",
                same("a\r\nb", "a\r\nb\r\nc\r\n"),
                &[],
                &[2],
                "a\r\nb\r\nc\r\n",
                1,
            ),
        ];
        for &(name, texts, removed, added, staged, count) in cases {
            assert_eq!(
                stage(texts, removed, added),
                (staged.to_string(), count),
                "{name}"
            );
        }
    }

    #[test]
    fn unstaging_lines_takes_the_covered_rows_back_to_heads() {
        // `HEAD` and the index, then the review's base and source.
        type Case<'a> = (
            &'a str,
            [&'a str; 4],
            &'a [usize],
            &'a [usize],
            &'a str,
            usize,
        );
        let staged = |head: &'static str, index: &'static str| [head, index, head, index];
        let all = |[head, index, file]: [&'static str; 3]| [head, index, head, file];
        let unstaged = |[head, index, file]: [&'static str; 3]| [head, index, index, file];
        let cases: &[Case] = &[
            (
                "an added row alone leaves the line it replaced out",
                staged("a\nb\nc\n", "a\nB\nc\n"),
                &[],
                &[1],
                "a\nc\n",
                1,
            ),
            (
                "a removed row alone comes back above the line that replaced it",
                staged("a\nb\nc\n", "a\nB\nc\n"),
                &[1],
                &[],
                "a\nb\nB\nc\n",
                1,
            ),
            (
                "every row of a hunk is the whole hunk",
                staged("a\nb\nc\n", "a\nB\nc\n"),
                &[1],
                &[1],
                "a\nb\nc\n",
                2,
            ),
            (
                "an added row of an `all` review is carried to the index's line",
                all(["a\nb\nc\nd\n", "a\nB\nc\nd\n", "x\na\nB\nc\nD\n"]),
                &[],
                &[2],
                "a\nc\nd\n",
                1,
            ),
            (
                "a row of an unstaged change has nothing to do",
                all(["a\nb\nc\nd\n", "a\nB\nc\nd\n", "a\nB\nc\nD\n"]),
                &[3],
                &[3],
                "a\nB\nc\nd\n",
                0,
            ),
            (
                "a line only the index has goes with its hunk",
                all(["a\nb\nc\n", "a\nB\nc\n", "a\nc\n"]),
                &[1],
                &[],
                "a\nb\nc\n",
                1,
            ),
            (
                "an unstaged review shows no staged change",
                unstaged(["a\nb\nc\n", "a\nB\nc\n", "a\nB2\nc\n"]),
                &[1],
                &[1],
                "a\nB\nc\n",
                0,
            ),
            (
                "a last line with no line ending gets one when it comes back",
                staged("a\nb", "a\nb\nc\n"),
                &[1],
                &[],
                "a\nb\nb\nc\n",
                1,
            ),
        ];
        for &(name, [head, index, base, source], removed, added, unstaged, count) in cases {
            let written = with_rows([base, source], removed, added, |file| {
                unstaged_lines(file, &Rope::from(head), &Rope::from(index)).unwrap()
            });
            assert_eq!(written, (unstaged.to_string(), count), "{name}");
        }
    }

    #[test]
    fn a_line_is_carried_past_hunks_and_not_out_of_one() {
        let hunks = [hunk(1..2, 1..3), hunk(4..5, 5..5), hunk(7..7, 7..9)];
        let carried = |from: Side, lines: std::ops::Range<usize>| -> Vec<_> {
            lines.map(|line| carry(&hunks, from, line)).collect()
        };
        assert_eq!(
            carried(Side::Before, 0..8),
            [
                Some(0),
                None,
                Some(3),
                Some(4),
                None,
                Some(5),
                Some(6),
                Some(9)
            ]
        );
        assert_eq!(
            carried(Side::After, 0..10),
            [
                Some(0),
                None,
                None,
                Some(2),
                Some(3),
                Some(5),
                Some(6),
                None,
                None,
                Some(7)
            ]
        );
    }

    #[test]
    fn a_removal_is_under_the_line_below_it() {
        assert!(under(&(3..3), &[(3, 3)]));
        assert!(under(&(1..3), &[(2, 2)]));
        assert!(under(&(1..3), &[(3, 3)]));
        assert!(!under(&(4..6), &[(2, 2)]));
        assert!(under(&(4..6), &[(0, 1), (5, 5)]));
    }

    #[test]
    fn lines_map_to_the_before_side() {
        let hunks = [hunk(1..2, 1..3), hunk(4..5, 5..5)];
        assert_eq!(
            map_to_before(&hunks, (0, 0)),
            (0, 0),
            "unchanged lines stay"
        );
        assert_eq!(
            map_to_before(&hunks, (3, 3)),
            (2, 2),
            "and shift past hunks"
        );
        assert_eq!(
            map_to_before(&hunks, (2, 2)),
            (1, 1),
            "a changed line maps to what it replaced"
        );
        assert_eq!(
            map_to_before(&hunks, (5, 5)),
            (5, 5),
            "the line below a removal maps past it"
        );
        assert_eq!(map_to_before(&hunks, (0, 3)), (0, 2));
        assert_eq!(
            map_to_before(&[hunk(2..2, 2..4)], (3, 3)),
            (2, 2),
            "an added line maps to the line it comes before"
        );
    }
}
