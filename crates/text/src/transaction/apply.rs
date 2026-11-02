//! `ChangeSet::apply`: each range replaced in place, from the last to the
//! first, or, for many small edits, the whole text rebuilt in one pass.

use std::ops::Range;

use super::{ChangeSet, Replacement};
use crate::{Rope, RopeBuilder, TextError};

/// The fewest replacements for which `apply` may rebuild the text.
pub(super) const REBUILD_MIN: usize = 64;

/// The most bytes per replacement, of the text before and the new texts,
/// that `apply` copies to rebuild the text instead of replacing each range.
// Below the measured break-even (5.5-6 KB), as a rebuild copies what replacing shares.
pub(super) const REBUILD_BYTES: usize = 4_096;

/// Which way `apply` went.
#[derive(Clone, Copy)]
enum Path {
    /// Replaced each range in place, last first.
    Replace,
    /// Rebuilt the whole text in one pass.
    Rebuild,
}

/// See [`ChangeSet::apply`].
pub(super) fn apply(changes: &ChangeSet, text: &mut Rope) -> Result<(), TextError> {
    changes.check_text(text)?;
    match path(changes) {
        Path::Replace => {
            for replacement in changes.replacements.iter().rev() {
                text.replace_with(
                    replacement.start,
                    replacement.end,
                    replacement.text.as_ref(),
                );
            }
        }
        Path::Rebuild => *text = rebuilt(changes, text),
    }
    Ok(())
}

/// The path for `changes`: rebuild for at least [`REBUILD_MIN`]
/// replacements with at most [`REBUILD_BYTES`] bytes to copy for each,
/// replace otherwise. Counted in unit tests.
fn path(changes: &ChangeSet) -> Path {
    let count = changes.replacements.len();
    let rebuild = count >= REBUILD_MIN && {
        let added = changes
            .replacements
            .iter()
            .map(Replacement::text_len)
            .fold(0, usize::saturating_add);
        changes.len_before.saturating_add(added) <= count.saturating_mul(REBUILD_BYTES)
    };
    let path = if rebuild {
        Path::Rebuild
    } else {
        Path::Replace
    };
    #[cfg(test)]
    let path = paths::pick(path);
    #[cfg(test)]
    paths::bump(path);
    path
}

/// The text after `changes`, built in one pass over the chunks of `text`,
/// which fits them, and the new texts. O(n + ΣL). It shares nothing with
/// `text`.
fn rebuilt(changes: &ChangeSet, text: &Rope) -> Rope {
    let mut builder = RopeBuilder::new();
    let mut replacements = changes.replacements.iter().peekable();
    // The old offset where the chunk starts, and the first old byte that a
    // replacement has not taken.
    let (mut start, mut kept) = (0, 0);
    for chunk in text.chunks() {
        let end = start + chunk.len();
        while let Some(r) = replacements.next_if(|r| r.start < end) {
            push_kept(&mut builder, chunk, start, kept..r.start);
            push_new(&mut builder, r.text.as_ref());
            kept = r.end;
        }
        push_kept(&mut builder, chunk, start, kept..end);
        start = end;
    }
    // Inserts at the end of the text.
    for r in replacements {
        push_new(&mut builder, r.text.as_ref());
    }
    builder.finish()
}

/// Pushes the old bytes of `range` that lie in `chunk`, which starts at old
/// offset `start`.
fn push_kept(builder: &mut RopeBuilder, chunk: &str, start: usize, range: Range<usize>) {
    let from = range.start.max(start);
    if from < range.end {
        // Checked offsets are char boundaries, and chunks hold whole chars.
        builder.push_str(
            chunk
                .get(from - start..range.end - start)
                .expect("the range is in the chunk, at checked char boundaries (invariant)"),
        );
    }
}

/// Pushes a replacement's new text.
fn push_new(builder: &mut RopeBuilder, text: Option<&Rope>) {
    for chunk in text.into_iter().flat_map(Rope::chunks) {
        builder.push_str(chunk);
    }
}

/// Forces `apply`'s path and counts the paths taken, on this thread, in
/// unit tests, so that they can run both paths on the same inputs.
#[cfg(test)]
pub(crate) mod paths {
    use std::cell::Cell;

    /// Which path `apply` takes.
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Force {
        /// The one the size rule picks.
        Auto,
        /// Always replace each range.
        Replace,
        /// Always rebuild the text.
        Rebuild,
    }

    thread_local! {
        static FORCE: Cell<Force> = const { Cell::new(Force::Auto) };
        static COUNTS: Cell<[u64; 2]> = const { Cell::new([0; 2]) };
    }

    /// Makes `apply` on this thread take `force`'s path from now on.
    pub(crate) fn force(force: Force) {
        FORCE.with(|cell| cell.set(force));
    }

    /// The counts on this thread so far: applies that replaced each range,
    /// and applies that rebuilt the text.
    pub(crate) fn read() -> [u64; 2] {
        COUNTS.with(Cell::get)
    }

    /// `auto`, unless a path is forced.
    pub(super) fn pick(auto: super::Path) -> super::Path {
        match FORCE.with(Cell::get) {
            Force::Auto => auto,
            Force::Replace => super::Path::Replace,
            Force::Rebuild => super::Path::Rebuild,
        }
    }

    pub(super) fn bump(path: super::Path) {
        let index = match path {
            super::Path::Replace => 0,
            super::Path::Rebuild => 1,
        };
        COUNTS.with(|counts| {
            let mut all = counts.get();
            all[index] += 1;
            counts.set(all);
        });
    }
}
