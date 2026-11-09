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

//! The check an edit passes before a document keeps it: that the edited
//! text's tree is the old one with exactly this edit made. Both walks keep
//! their own list, so neither recurses, however deep the tree.

use super::super::{Table, ValueKind};
use super::Step;

/// Whether two floats are the same data: the same bits, or two NaNs of the
/// same sign, whatever their payloads.
fn same_float(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        a.is_sign_negative() == b.is_sign_negative()
    } else {
        a.to_bits() == b.to_bits()
    }
}

/// Whether `a` and `b` hold the same data: the same kind and value,
/// ignoring spans and how tables were defined; floats by [`same_float`];
/// arrays in order, ignoring whether they are arrays of tables; and tables
/// entry by entry, in order.
pub(super) fn same_data(a: &ValueKind, b: &ValueKind) -> bool {
    let mut work = vec![(a, b)];
    while let Some((a, b)) = work.pop() {
        let same = match (a, b) {
            (ValueKind::String(a), ValueKind::String(b)) => a == b,
            (ValueKind::Integer(a), ValueKind::Integer(b)) => a == b,
            (ValueKind::Float(a), ValueKind::Float(b)) => same_float(*a, *b),
            (ValueKind::Boolean(a), ValueKind::Boolean(b)) => a == b,
            (ValueKind::Datetime(a), ValueKind::Datetime(b)) => a == b,
            (ValueKind::Array(a), ValueKind::Array(b)) => {
                work.extend(
                    a.items()
                        .iter()
                        .zip(b.items())
                        .map(|(a, b)| (&a.kind, &b.kind)),
                );
                a.len() == b.len()
            }
            (ValueKind::Table(a), ValueKind::Table(b)) => same_entries(a, b, None, &mut work),
            _ => false,
        };
        if !same {
            return false;
        }
    }
    true
}

/// Whether `a` and `b` have entries of the same names in the same order,
/// leaving out the one named `except` wherever it is in either; adds the
/// pairs of their values to `work`, for [`same_data`] to compare.
fn same_entries<'t>(
    a: &'t Table,
    b: &'t Table,
    except: Option<&str>,
    work: &mut Vec<(&'t ValueKind, &'t ValueKind)>,
) -> bool {
    let kept = |table: &'t Table| {
        table
            .entries()
            .iter()
            .filter(move |(key, _)| Some(key.name.as_str()) != except)
    };
    if kept(a).count() != kept(b).count() {
        return false;
    }
    for ((a_key, a_value), (b_key, b_value)) in kept(a).zip(kept(b)) {
        if a_key.name != b_key.name {
            return false;
        }
        work.push((&a_value.kind, &b_value.kind));
    }
    true
}

/// Where the walk of [`verify`] is in one of the trees.
#[derive(Clone, Copy)]
enum At<'t> {
    Root(&'t Table),
    Value(&'t ValueKind),
}

impl<'t> At<'t> {
    fn table(self) -> Option<&'t Table> {
        match self {
            Self::Root(table) | Self::Value(ValueKind::Table(table)) => Some(table),
            Self::Value(_) => None,
        }
    }
}

/// Checks that `new` is `old` with the value at `path` set to `value` and
/// nothing else changed, walking both trees along the path:
///
/// - every table on the path has the same other entries, in the same order
///   and with the same data, whatever the place of the entry the path goes
///   through, which may be new;
/// - every array on the path has the same length, is an array of tables
///   or not as before, and has the same other elements;
/// - every table the edit made holds only the next step;
/// - and the value at the path holds the same data as `value`.
///
/// The error says which failed.
pub(super) fn verify(
    old: &Table,
    new: &Table,
    path: &[Step<'_>],
    value: &ValueKind,
) -> Result<(), &'static str> {
    // `None` once the path is past what the old tree had.
    let mut old = Some(At::Root(old));
    let mut new = At::Root(new);
    let mut work = Vec::new();
    for step in path {
        match *step {
            Step::Key(name) => {
                let new_table = new.table().ok_or("a table on the path is no longer one")?;
                let next = new_table
                    .get(name)
                    .ok_or("the edited text does not have the path")?;
                match old {
                    Some(old_at) => {
                        let old_table = old_at
                            .table()
                            .ok_or("the old text had no table on the path")?;
                        if !same_entries(old_table, new_table, Some(name), &mut work) {
                            return Err("an entry beside the path changed");
                        }
                        old = old_table.get(name).map(|value| At::Value(&value.kind));
                    }
                    None if new_table.len() != 1 => {
                        return Err("a table the edit made holds more than the path");
                    }
                    None => {}
                }
                new = At::Value(&next.kind);
            }
            Step::Index(index) => {
                let (
                    Some(At::Value(ValueKind::Array(old_array))),
                    At::Value(ValueKind::Array(new_array)),
                ) = (old, new)
                else {
                    return Err("an array on the path is no longer one");
                };
                if old_array.len() != new_array.len()
                    || old_array.is_array_of_tables() != new_array.is_array_of_tables()
                {
                    return Err("an array on the path changed its length or kind");
                }
                for (at, (a, b)) in old_array.items().iter().zip(new_array.items()).enumerate() {
                    if at != index {
                        work.push((&a.kind, &b.kind));
                    }
                }
                let (Some(old_item), Some(new_item)) = (old_array.get(index), new_array.get(index))
                else {
                    return Err("the edited text does not have the path");
                };
                old = Some(At::Value(&old_item.kind));
                new = At::Value(&new_item.kind);
            }
        }
    }
    let At::Value(found) = new else {
        return Err("the path is empty");
    };
    if !same_data(found, value) {
        return Err("the value at the path is not the value set");
    }
    // What is beside the path, compared last since it is the bulk.
    while let Some((a, b)) = work.pop() {
        if !same_data(a, b) {
            return Err("an entry beside the path changed");
        }
    }
    Ok(())
}
