// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! `ChangeSet::compose`: both change sets viewed as retain, insert and
//! delete ops, merged in one pass.

use super::{Builder, ChangeSet, Replacement};
use crate::{Rope, TextError};

/// One op of a change set's op view, or what is left of it.
#[derive(Clone, Copy)]
enum Op<'a> {
    /// Keep this many bytes.
    Retain(usize),
    /// Delete this many bytes.
    Delete(usize),
    /// Insert `text[from..]`.
    Insert { text: &'a Rope, from: usize },
}

impl Op<'_> {
    /// What is left after taking `k` bytes of it, or `None` if nothing.
    fn after(self, k: usize) -> Option<Self> {
        match self {
            Op::Retain(n) => (n > k).then(|| Op::Retain(n - k)),
            Op::Delete(n) => (n > k).then(|| Op::Delete(n - k)),
            Op::Insert { text, from } => (from + k < text.len()).then_some(Op::Insert {
                text,
                from: from + k,
            }),
        }
    }
}

/// The op view of a change set: for each replacement `Retain(start − pos)`,
/// `Insert(text)` and `Delete(end − start)`, then a final `Retain`, with
/// zero-length ops skipped.
struct Ops<'a> {
    replacements: std::slice::Iter<'a, Replacement>,
    /// The old offset reached.
    pos: usize,
    /// The old length.
    len: usize,
    /// The current replacement's text, not yet given out.
    insert: Option<&'a Rope>,
    /// The current replacement's deletion, not yet given out.
    delete: usize,
}

impl<'a> Ops<'a> {
    fn new(changes: &'a ChangeSet) -> Self {
        Self {
            replacements: changes.replacements.iter(),
            pos: 0,
            len: changes.len_before,
            insert: None,
            delete: 0,
        }
    }

    fn next(&mut self) -> Option<Op<'a>> {
        loop {
            if let Some(text) = self.insert.take() {
                return Some(Op::Insert { text, from: 0 });
            }
            if self.delete > 0 {
                return Some(Op::Delete(std::mem::take(&mut self.delete)));
            }
            let Some(r) = self.replacements.next() else {
                let rest = self.len.saturating_sub(self.pos);
                self.pos = self.len;
                return (rest > 0).then_some(Op::Retain(rest));
            };
            let keep = r.start.saturating_sub(self.pos);
            self.pos = r.end;
            self.insert = r.text.as_ref().filter(|text| !text.is_empty());
            self.delete = r.end.saturating_sub(r.start);
            if keep > 0 {
                return Some(Op::Retain(keep));
            }
        }
    }
}

/// `a`, then `b`.
pub(super) fn compose(a: &ChangeSet, b: &ChangeSet) -> Result<ChangeSet, TextError> {
    let mismatch = TextError::LengthMismatch {
        expected: b.len_before,
        actual: a.len_after,
    };
    if b.len_before != a.len_after {
        return Err(mismatch);
    }
    let (mut xs, mut ys) = (Ops::new(a), Ops::new(b));
    let (mut x, mut y) = (xs.next(), ys.next());
    let mut out = Builder::default();
    // The offset in `a`'s result, which is `b`'s text, where `x` starts.
    let mut mid = 0;
    loop {
        // The bytes both sides take this round, if they take any.
        let taken = match (x, y) {
            (Some(Op::Delete(n)), _) => {
                out.delete(n);
                x = xs.next();
                continue;
            }
            (_, Some(Op::Insert { text, from })) => {
                out.insert(if from == 0 {
                    text.clone()
                } else {
                    text.piece(from..text.len())
                });
                y = ys.next();
                continue;
            }
            (None, None) => break,
            (Some(Op::Retain(n)), Some(Op::Retain(m))) => {
                let k = n.min(m);
                out.retain(k);
                k
            }
            (Some(Op::Retain(n)), Some(Op::Delete(m))) => {
                let k = n.min(m);
                out.delete(k);
                k
            }
            (Some(Op::Insert { text, from }), Some(Op::Retain(m) | Op::Delete(m))) => {
                let k = (text.len() - from).min(m);
                let end = from + k;
                if end < text.len() && !text.is_char_boundary(end) {
                    return Err(TextError::NotCharBoundary { byte: mid + k });
                }
                if matches!(y, Some(Op::Retain(_))) {
                    out.insert(if from == 0 && end == text.len() {
                        text.clone()
                    } else {
                        text.piece(from..end)
                    });
                }
                // Text `a` inserts and `b` deletes is never in the result.
                k
            }
            // A side ran out before the other: impossible once the lengths
            // agree, but never a panic.
            _ => return Err(mismatch),
        };
        mid += taken;
        x = x.and_then(|op| op.after(taken)).or_else(|| xs.next());
        y = y.and_then(|op| op.after(taken)).or_else(|| ys.next());
    }
    Ok(out.finish(a.len_before))
}
