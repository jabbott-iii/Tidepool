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

//! [`Document`]: a TOML document's text and tree, edited one value at a
//! time with the rest of the text kept as it was.
//!
//! Written for Tidepool (ADR-0013, ADR-0031). An edit goes through four
//! steps: [`resolve`] walks the path; `place` plans one splice of the text
//! (`place::plan_replace` or `place::plan_insert`), with text from `write`;
//! [`Document::commit`] makes the new text and parses it; and `verify`
//! compares the new tree with the old along the path. The document changes
//! only if every step succeeds. The rules are in the
//! [module documentation](super#editing).

mod place;
mod verify;

use super::parse::{described, parse, parse_bytes};
use super::write::value_depth;
use super::{EditErrorKind, ErrorKind, Span, Table, TableKind, Value, ValueKind};
use crate::{CodecError, Limits};
use place::Splice;
use verify::same_data;

/// One step of a path from a document's root table: a key of a table, or
/// an index of an array. See the [module documentation](super#paths).
///
/// `"editor".into()` and `0usize.into()` make steps, so a path can be
/// written `&["editor".into(), "font-size".into()]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Step<'a> {
    /// The entry with this name: decoded, and compared exactly, as
    /// [`Table::get`] compares.
    Key(&'a str),
    /// The element at this index, from 0, of an array written as a value or
    /// of an array of tables.
    Index(usize),
}

impl<'a> From<&'a str> for Step<'a> {
    /// [`Step::Key`].
    fn from(name: &'a str) -> Self {
        Self::Key(name)
    }
}

impl From<usize> for Step<'_> {
    /// [`Step::Index`].
    fn from(index: usize) -> Self {
        Self::Index(index)
    }
}

/// A TOML document that can be edited: its text, its root [`Table`] and the
/// [`Limits`] it was read with, which bound every edit too.
///
/// Read values through [`Document::root`] or [`Document::get`], change or
/// add them with [`Document::set`], and take the text back with
/// [`Document::as_str`] or [`Document::into_string`]. Until an edit the
/// text is the input byte for byte; after one, only the bytes of the value
/// it changed, or of the entry or table it added, are new. The
/// [module documentation](super#editing) has the rules, and an example.
///
/// The tree is always what [`toml::parse`](fn@super::parse) gives for the text
/// with these limits, since an edit makes both anew and keeps them only
/// once they agree.
#[derive(Clone, Debug)]
pub struct Document {
    text: String,
    root: Table,
    limits: Limits,
}

/// Where a path leads.
enum Resolved<'d> {
    /// To a value the document has.
    Found(&'d Value),
    /// Past the end of the document: some of its steps do not exist.
    Missing(Missing<'d>),
}

/// A path whose steps from `first` on do not exist: the tables an edit
/// makes, then the key it adds. Every step from `first` on is a
/// [`Step::Key`].
struct Missing<'d> {
    /// N: the deepest table on the path that exists, which `path[..first]`
    /// reaches.
    anchor: &'d Table,
    /// N's span, or `None` for the root.
    anchor_span: Option<Span>,
    /// How many steps reach the nearest table at or above N that dotted
    /// keys did not make: the section's table for a table made by dotted
    /// keys in a section, the inline table for one in an inline table, and
    /// N itself otherwise.
    container: usize,
    /// That table's kind.
    container_kind: TableKind,
    /// The index of the first step that does not exist.
    first: usize,
}

/// Where a walk of a path is: at the root table, or at a value.
#[derive(Clone, Copy)]
enum Here<'d> {
    Root(&'d Table),
    Value(&'d Value),
}

/// Walks `path` from `root`. An error is the step that could not be taken,
/// and why: a key on something other than a table, an index on something
/// other than an array or past its end, or an index among the steps that
/// do not exist.
fn resolve<'d>(root: &'d Table, path: &[Step<'_>]) -> Result<Resolved<'d>, (usize, EditErrorKind)> {
    let mut here = Here::Root(root);
    let mut container = 0;
    let mut container_kind = TableKind::Root;
    for (at, step) in path.iter().enumerate() {
        let next = match *step {
            Step::Key(name) => {
                let (table, span) = match here {
                    Here::Root(table) => (table, None),
                    Here::Value(value) => match &value.kind {
                        ValueKind::Table(table) => (table, Some(value.span)),
                        other => {
                            let found = described(other);
                            return Err((at, EditErrorKind::NotATable { found }));
                        }
                    },
                };
                match table.get(name) {
                    Some(next) => next,
                    None => {
                        let rest = path.get(at + 1..).unwrap_or_default();
                        if let Some(index) =
                            rest.iter().position(|step| matches!(step, Step::Index(_)))
                        {
                            return Err((at + 1 + index, EditErrorKind::MissingArray));
                        }
                        return Ok(Resolved::Missing(Missing {
                            anchor: table,
                            anchor_span: span,
                            container,
                            container_kind,
                            first: at,
                        }));
                    }
                }
            }
            Step::Index(index) => {
                let array = match here {
                    Here::Root(_) => {
                        let found = "a table";
                        return Err((at, EditErrorKind::NotAnArray { found }));
                    }
                    Here::Value(value) => match &value.kind {
                        ValueKind::Array(array) => array,
                        other => {
                            let found = described(other);
                            return Err((at, EditErrorKind::NotAnArray { found }));
                        }
                    },
                };
                let len = array.len();
                array
                    .get(index)
                    .ok_or((at, EditErrorKind::IndexOutOfRange { len }))?
            }
        };
        if let ValueKind::Table(table) = &next.kind
            && table.kind() != TableKind::Dotted
        {
            container = at + 1;
            container_kind = table.kind();
        }
        here = Here::Value(next);
    }
    match here {
        Here::Value(value) => Ok(Resolved::Found(value)),
        Here::Root(_) => Err((0, EditErrorKind::EmptyPath)),
    }
}

/// What a value is, if [`Document::set`] cannot replace it whole because
/// its text is spread over lines or sections; `None` if it can.
fn not_replaceable(kind: &ValueKind) -> Option<&'static str> {
    match kind {
        ValueKind::Table(table) => match table.kind() {
            TableKind::Inline => None,
            TableKind::Header => Some("a table defined by a header"),
            TableKind::ArrayHeader => Some("an element of an array of tables"),
            TableKind::Implicit => Some("a table made by a header's prefix"),
            TableKind::Dotted => Some("a table made by dotted keys"),
            TableKind::Root => Some("the root table"),
        },
        ValueKind::Array(array) if array.is_array_of_tables() => Some("an array of tables"),
        _ => None,
    }
}

impl Document {
    /// Parses `text` as [`toml::parse`](fn@super::parse) does, and keeps a copy
    /// of it. `limits` also bound every later edit.
    ///
    /// # Errors
    ///
    /// Exactly those of [`toml::parse`](fn@super::parse), as
    /// [`CodecError::Toml`].
    pub fn parse(text: &str, limits: Limits) -> Result<Self, CodecError> {
        let root = parse(text, limits)?;
        Ok(Self {
            text: text.to_string(),
            root,
            limits,
        })
    }

    /// Parses bytes, which must be UTF-8, as
    /// [`toml::parse_bytes`](super::parse_bytes) does: the form a file arrives
    /// in. Otherwise the same as [`Document::parse`].
    ///
    /// # Errors
    ///
    /// Exactly those of [`toml::parse_bytes`](super::parse_bytes), as
    /// [`CodecError::Toml`].
    pub fn parse_bytes(bytes: &[u8], limits: Limits) -> Result<Self, CodecError> {
        let root = parse_bytes(bytes, limits)?;
        // `parse_bytes` accepts only UTF-8, which this borrows unchanged.
        let text = String::from_utf8_lossy(bytes).into_owned();
        Ok(Self { text, root, limits })
    }

    /// The text: the input until an edit, then the input with only the
    /// edited bytes changed.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The text, consuming the document.
    pub fn into_string(self) -> String {
        self.text
    }

    /// The root table. Its spans index [`Document::as_str`].
    pub fn root(&self) -> &Table {
        &self.root
    }

    /// The value at `path`, if there is one. `None` for an empty path, a
    /// step that does not exist or cannot be taken, and a [`Step::Key`] on
    /// an array of tables, whose elements need a [`Step::Index`].
    pub fn get(&self, path: &[Step<'_>]) -> Option<&Value> {
        match resolve(&self.root, path) {
            Ok(Resolved::Found(value)) => Some(value),
            _ => None,
        }
    }

    /// Makes the value at `path` hold `value`: replaces the value there, or
    /// adds it, with any tables missing on the way. Does nothing if it
    /// already holds the same data. The
    /// [module documentation](super#editing) says what is written where.
    ///
    /// Replacing a value written over several lines (a multi-line string,
    /// or an array or inline table laid over lines) writes it on one line
    /// of TOML 1.0, and the comments inside it go: set its elements or
    /// entries instead to keep them.
    ///
    /// # Errors
    ///
    /// [`CodecError::TomlEdit`], with the step of the path the problem
    /// concerns and an [`EditErrorKind`] saying what
    /// it is. The document is unchanged.
    pub fn set(&mut self, path: &[Step<'_>], value: &ValueKind) -> Result<(), CodecError> {
        self.edit(path, value)
            .map_err(|(step, kind)| CodecError::TomlEdit { step, kind })
    }

    /// [`Document::set`], with the error as a step and a kind.
    fn edit(&mut self, path: &[Step<'_>], value: &ValueKind) -> Result<(), (usize, EditErrorKind)> {
        match self.plan(path, value)? {
            Some(splice) => self.commit(path, value, splice),
            None => Ok(()),
        }
    }

    /// Plans the splice that sets `path` to `value`, or `None` if the value
    /// there already holds the same data.
    fn plan(
        &self,
        path: &[Step<'_>],
        value: &ValueKind,
    ) -> Result<Option<Splice>, (usize, EditErrorKind)> {
        let target = path
            .len()
            .checked_sub(1)
            .ok_or((0, EditErrorKind::EmptyPath))?;
        let planned = match resolve(&self.root, path)? {
            Resolved::Found(found) => {
                if same_data(&found.kind, value) {
                    return Ok(None);
                }
                if let Some(found) = not_replaceable(&found.kind) {
                    return Err((target, EditErrorKind::NotReplaceable { found }));
                }
                self.check_depth(target, value)?;
                place::plan_replace(&self.text, self.limits, found.span, value)
            }
            Resolved::Missing(missing) => {
                self.check_depth(target, value)?;
                place::plan_insert(&self.text, &self.root, self.limits, path, &missing, value)
            }
        };
        planned.map(Some).map_err(|kind| match kind {
            EditErrorKind::InvalidDatetime { .. } => (target, kind),
            _ => (path.len(), kind),
        })
    }

    /// Fails unless `value`, inside the `containers` tables and arrays that
    /// the path's last step is in, is within [`Limits::max_depth`]. Checked
    /// before writing, so that the writer, which recurses, never goes
    /// deeper than that.
    fn check_depth(
        &self,
        containers: usize,
        value: &ValueKind,
    ) -> Result<(), (usize, EditErrorKind)> {
        let limit = self.limits.max_depth;
        let too_deep = (
            containers,
            EditErrorKind::Limit(ErrorKind::TooDeep { limit }),
        );
        let room = limit.checked_sub(containers).ok_or(too_deep.clone())?;
        value_depth(value, room).map(drop).ok_or(too_deep)
    }

    /// Makes the edit `splice` plans, if the text it makes is the old data
    /// with `path` set to `value` and nothing else changed: builds the new
    /// text, parses it with the document's limits, and compares the trees
    /// along the path. Only then are the text and tree replaced.
    fn commit(
        &mut self,
        path: &[Step<'_>],
        value: &ValueKind,
        splice: Splice,
    ) -> Result<(), (usize, EditErrorKind)> {
        let whole = path.len();
        let unverified = |reason| (whole, EditErrorKind::Unverified { reason });
        let (start, end) = (splice.range.start, splice.range.end);
        let (Some(before), Some(after)) = (self.text.get(..start), self.text.get(end..)) else {
            return Err(unverified("the splice is not in the text"));
        };
        if start > end {
            return Err(unverified("the splice is not in the text"));
        }
        let limit = self.limits.max_input_bytes;
        let new_len = before
            .len()
            .checked_add(splice.text.len())
            .and_then(|len| len.checked_add(after.len()))
            .filter(|&len| len <= limit)
            .ok_or((whole, EditErrorKind::TooLarge { limit }))?;
        let mut text = String::with_capacity(new_len);
        text.push_str(before);
        text.push_str(&splice.text);
        text.push_str(after);
        let root = match parse(&text, self.limits) {
            Ok(root) => root,
            Err(CodecError::Toml { kind, .. }) => {
                let kind = match kind {
                    ErrorKind::InputTooLarge { limit, .. } => EditErrorKind::TooLarge { limit },
                    ErrorKind::TooDeep { .. }
                    | ErrorKind::StringTooLong { .. }
                    | ErrorKind::NumberTooLong { .. }
                    | ErrorKind::TooManyEntries { .. } => EditErrorKind::Limit(kind),
                    _ => EditErrorKind::Unverified {
                        reason: "the edited text does not parse",
                    },
                };
                return Err((whole, kind));
            }
            Err(_) => return Err(unverified("the edited text does not parse")),
        };
        verify::verify(&self.root, &root, path, value).map_err(unverified)?;
        self.text = text;
        self.root = root;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
