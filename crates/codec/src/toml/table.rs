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

//! [`Table`]: entries in document order, found by key in constant time.

use std::fmt;
use std::hash::{BuildHasher, RandomState};

use super::{Key, TableKind, Value, ValueKind};

/// A table with up to this many entries is searched in order; a larger one
/// keeps an [`Index`], so that finding a key never costs more than a few
/// comparisons, however many entries there are.
const INDEX_FROM: usize = 8;

/// The most entries an [`Index`] numbers, since it keeps positions in 32
/// bits. A table with more, which would take over 400 GB, is searched in
/// order.
const INDEX_UP_TO: usize = u32::MAX as usize;

/// Makes room in `items` for one more, if it has none, by a quarter of its
/// length (and at least one) rather than by doubling from four as a `Vec`
/// does. Room to spare stays under a quarter, which matters since most
/// tables are small and a table entry is 96 bytes, and each element is still
/// moved at most five times on average as the vector grows.
pub(super) fn reserve_one<T>(items: &mut Vec<T>) {
    if items.len() == items.capacity() {
        items.reserve_exact((items.len() / 4).max(1));
    }
}

/// Where each entry of a large table is, by the hash of its name.
///
/// Open addressing with linear probing, over a power-of-two number of
/// slots that is never more than half full, so that a search ends at an
/// empty slot after a few probes. A slot holds an entry's position plus
/// one, or zero when it is empty. A search compares the names in the
/// table's own entries, so the index holds no copy of them: a slot is 4
/// bytes, so the index takes 8 to 16 bytes an entry. When it would be more
/// than half full, it is built again twice as large.
///
/// The hasher is `std`'s SipHash with keys chosen at random for each table,
/// so a document cannot be written to make its keys collide.
#[derive(Clone)]
struct Index {
    hasher: RandomState,
    slots: Box<[u32]>,
}

impl Index {
    /// The number of slots for `len` entries: a power of two, at least 16
    /// and at least twice `len`.
    fn slots_for(len: usize) -> Option<usize> {
        let slots = len.checked_mul(2)?.checked_next_power_of_two()?;
        Some(slots.max(16))
    }

    /// An index of `entries` with `slots` slots, from [`Index::slots_for`].
    /// `None` if a position does not fit in a slot.
    fn build(hasher: RandomState, entries: &[(Key, Value)], slots: usize) -> Option<Self> {
        let mut index = Self {
            hasher,
            slots: vec![0; slots].into_boxed_slice(),
        };
        for (at, (key, _)) in entries.iter().enumerate() {
            index.place(&key.name, at)?;
        }
        Some(index)
    }

    /// The slot a search for `name` starts at.
    fn first_slot(&self, name: &str) -> usize {
        // Truncating the hash keeps its low bits, which are the ones used.
        (self.hasher.hash_one(name) as usize) & self.mask()
    }

    /// The slots' count less one: they are a power of two.
    fn mask(&self) -> usize {
        self.slots.len().wrapping_sub(1)
    }

    /// Records that the entry at `at` is named `name`, in the first empty
    /// slot from where its hash points. `None` if `at` does not fit in a
    /// slot. The caller keeps the index at most half full, so there is
    /// always an empty slot.
    fn place(&mut self, name: &str, at: usize) -> Option<()> {
        let stored = u32::try_from(at.checked_add(1)?).ok()?;
        let mask = self.mask();
        let mut slot = self.first_slot(name);
        loop {
            let cell = self.slots.get_mut(slot)?;
            if *cell == 0 {
                *cell = stored;
                return Some(());
            }
            slot = (slot + 1) & mask;
        }
    }

    /// The position in `entries` of the entry named `name`, if there is
    /// one: the search ends at the first empty slot.
    fn find(&self, entries: &[(Key, Value)], name: &str) -> Option<usize> {
        let mask = self.mask();
        let mut slot = self.first_slot(name);
        loop {
            let stored = usize::try_from(*self.slots.get(slot)?).ok()?;
            let at = stored.checked_sub(1)?;
            if entries.get(at)?.0.name == name {
                return Some(at);
            }
            slot = (slot + 1) & mask;
        }
    }
}

/// A TOML table: its entries in the order the document defines them, and
/// how the table was defined.
///
/// Keys are unique, since TOML forbids defining one twice, and are compared
/// exactly: decoded, case-sensitive and without Unicode normalization.
/// Finding a key takes constant time on average, however large the table:
/// a table of more than 8 entries keeps an index of their positions by the
/// hash of their names (with `std`'s randomly keyed hasher), which costs 8
/// to 16 bytes an entry and holds no copy of the names.
///
/// Equality compares the kind and the entries in order, spans included.
#[derive(Clone)]
pub struct Table {
    entries: Vec<(Key, Value)>,
    /// Where each entry is, once there are more than [`INDEX_FROM`] of them.
    /// Boxed, so that a table, and with it every `Value`, stays small.
    index: Option<Box<Index>>,
    kind: TableKind,
}

impl Table {
    /// An empty table of `kind`, which allocates nothing until an entry is
    /// added.
    pub(super) const fn new(kind: TableKind) -> Self {
        Self {
            entries: Vec::new(),
            index: None,
            kind,
        }
    }

    /// How the table was defined.
    pub fn kind(&self) -> TableKind {
        self.kind
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries, in the order the document defines them. For a table
    /// defined in pieces (`[a.b]` before `[a]`, or dotted keys spread over
    /// several lines), that is the order the keys first appear in.
    pub fn entries(&self) -> &[(Key, Value)] {
        &self.entries
    }

    /// The value of the entry named `name`, if there is one.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.get_key_value(name).map(|(_, value)| value)
    }

    /// The key and value of the entry named `name`, if there is one: the
    /// key carries the span of the name as the document writes it.
    pub fn get_key_value(&self, name: &str) -> Option<(&Key, &Value)> {
        let (key, value) = self.entries.get(self.position(name)?)?;
        Some((key, value))
    }

    /// Whether the table has an entry named `name`.
    pub fn contains_key(&self, name: &str) -> bool {
        self.position(name).is_some()
    }

    pub(super) fn set_kind(&mut self, kind: TableKind) {
        self.kind = kind;
    }

    /// Where the entry named `name` is in `entries`.
    pub(super) fn position(&self, name: &str) -> Option<usize> {
        match &self.index {
            Some(index) => index.find(&self.entries, name),
            None => self.entries.iter().position(|(key, _)| key.name == name),
        }
    }

    /// The entry at `at`, a position [`Table::position`] gave, or the one
    /// [`Table::push`] just added. Positions are never stale: entries are
    /// only ever added, at the end.
    pub(super) fn entry_mut(&mut self, at: usize) -> &mut (Key, Value) {
        self.entries
            .get_mut(at)
            .expect("a position `Table::position` gave is in range (invariant)")
    }

    /// The values of the entries, for walking a tree.
    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        self.entries.iter_mut().map(|(_, value)| value)
    }

    /// Adds an entry, whose key the table must not have yet, and returns
    /// it. The caller checks the limit on entries first. Room grows by a
    /// quarter (see [`reserve_one`]).
    pub(super) fn push(&mut self, key: Key, value: Value) -> &mut (Key, Value) {
        let at = self.entries.len();
        reserve_one(&mut self.entries);
        self.entries.push((key, value));
        self.index_entry(at);
        self.entry_mut(at)
    }

    /// Adds the entry at `at`, the last, to the index: building the index
    /// once the table has more than [`INDEX_FROM`] entries, and building it
    /// again twice as large when it would be more than half full.
    fn index_entry(&mut self, at: usize) {
        let len = self.entries.len();
        let room = self
            .index
            .as_ref()
            .is_some_and(|index| len.saturating_mul(2) <= index.slots.len());
        if room {
            if let (Some(index), Some((key, _))) = (self.index.as_mut(), self.entries.get(at))
                && index.place(&key.name, at).is_none()
            {
                self.index = None;
            }
        } else if len > INDEX_FROM && len <= INDEX_UP_TO {
            let hasher = self
                .index
                .take()
                .map_or_else(RandomState::new, |index| index.hasher);
            self.index = Index::slots_for(len)
                .and_then(|slots| Index::build(hasher, &self.entries, slots))
                .map(Box::new);
        } else {
            // Small, or too many to number: searched in order.
            self.index = None;
        }
    }

    /// Adds an entry holding `table`, as [`Table::push`] does, and returns
    /// the table.
    pub(super) fn push_table(&mut self, key: Key, table: Table, span: super::Span) -> &mut Table {
        let value = Value {
            kind: ValueKind::Table(table),
            span,
        };
        match &mut self.push(key, value).1.kind {
            ValueKind::Table(table) => table,
            _ => unreachable!("the value just added is a table"),
        }
    }

    /// The entries, for taking a table apart.
    pub(super) fn into_entries(self) -> Vec<(Key, Value)> {
        self.entries
    }

    /// Moves the entries to an allocation of exactly their size, for a
    /// table that is complete while the parse goes on (an inline table at
    /// its closing brace): moving, rather than shrinking in place, frees the
    /// old allocation whole for what comes next.
    pub(super) fn shrink(&mut self) {
        if self.entries.capacity() != self.entries.len() {
            let mut exact = Vec::with_capacity(self.entries.len());
            exact.append(&mut self.entries);
            self.entries = exact;
        }
    }

    /// Leaves the entries no room to spare, in place, for a table that can
    /// no longer grow because the parse is over. The index needs nothing:
    /// it is built for the number of entries it holds and kept no more than
    /// twice that.
    pub(super) fn settle(&mut self) {
        self.entries.shrink_to_fit();
    }

    /// The sizes in bytes of the allocations the table holds itself, at
    /// their capacity: its entries, and its index. Not what the entries
    /// hold.
    #[cfg(test)]
    pub(super) fn own_allocations(&self) -> Vec<usize> {
        let mut sizes = vec![self.entries.capacity() * std::mem::size_of::<(Key, Value)>()];
        if let Some(index) = &self.index {
            sizes.push(std::mem::size_of::<Index>());
            sizes.push(index.slots.len() * std::mem::size_of::<u32>());
        }
        sizes
    }
}

impl PartialEq for Table {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.entries == other.entries
    }
}

impl fmt::Debug for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Table")
            .field("kind", &self.kind)
            .field("entries", &self.entries)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toml::Span;

    fn key(name: &str, at: usize) -> Key {
        Key {
            name: name.to_string(),
            span: Span::new(at, at + name.len()),
        }
    }

    fn int(value: i64) -> Value {
        Value {
            kind: ValueKind::Integer(value),
            span: Span::new(0, 1),
        }
    }

    #[test]
    fn small_and_large_tables_find_every_key_in_order() {
        for size in [
            0,
            1,
            INDEX_FROM - 1,
            INDEX_FROM,
            INDEX_FROM + 1,
            16,
            17,
            1000,
        ] {
            let mut table = Table::new(TableKind::Header);
            for n in 0..size {
                table.push(key(&format!("k{n}"), n), int(n as i64));
            }
            assert_eq!(table.len(), size);
            assert_eq!(table.is_empty(), size == 0);
            assert_eq!(table.index.is_some(), size > INDEX_FROM, "size {size}");
            for n in 0..size {
                let name = format!("k{n}");
                assert_eq!(table.position(&name), Some(n));
                assert_eq!(table.get(&name), Some(&int(n as i64)));
                let (found, _) = table.get_key_value(&name).expect("present");
                assert_eq!(found.span, Span::new(n, n + name.len()));
                assert!(table.contains_key(&name));
            }
            assert!(!table.contains_key("missing"));
            assert!(!table.contains_key(""));
            assert_eq!(table.get("K0"), None);
            let names: Vec<&str> = table
                .entries()
                .iter()
                .map(|(k, _)| k.name.as_str())
                .collect();
            let expected: Vec<String> = (0..size).map(|n| format!("k{n}")).collect();
            assert_eq!(names, expected);
        }
    }

    #[test]
    fn the_index_is_never_more_than_half_full_nor_more_than_four_slots_an_entry() {
        let mut table = Table::new(TableKind::Header);
        for n in 0..5000 {
            table.push(key(&format!("k{n}"), 0), int(n));
            if let Some(index) = &table.index {
                let slots = index.slots.len();
                let len = table.len();
                assert!(slots.is_power_of_two() && slots >= 16);
                assert!(2 * len <= slots, "{len} entries in {slots} slots");
                assert!(slots <= (4 * len).max(32), "{len} entries in {slots} slots");
                assert_eq!(index.slots.iter().filter(|&&slot| slot != 0).count(), len);
            }
        }
        // A clone searches as well as the original.
        let clone = table.clone();
        assert_eq!(clone.position("k4999"), Some(4999));
        assert_eq!(clone.position("k5000"), None);
    }

    #[test]
    fn keys_that_start_in_the_same_slot_are_still_told_apart() {
        // Of any 17 names, two start in the same one of 16 slots; put them,
        // and six more, in an index of 16 slots, so that a search has to
        // probe past one to find the other.
        let mut index = Index {
            hasher: RandomState::new(),
            slots: vec![0; 16].into_boxed_slice(),
        };
        let names: Vec<String> = (0..17).map(|n| format!("k{n}")).collect();
        let mut by_slot: Vec<Vec<&str>> = vec![Vec::new(); 16];
        for name in &names {
            by_slot[index.first_slot(name)].push(name);
        }
        let pair = by_slot
            .iter()
            .find(|names| names.len() >= 2)
            .expect("pigeonhole");
        let mut chosen: Vec<&str> = pair.iter().take(2).copied().collect();
        for name in &names {
            if chosen.len() < 8 && !chosen.contains(&name.as_str()) {
                chosen.push(name);
            }
        }
        let entries: Vec<(Key, Value)> = chosen.iter().map(|name| (key(name, 0), int(0))).collect();
        for (at, name) in chosen.iter().enumerate() {
            index.place(name, at).expect("room");
        }
        for (at, name) in chosen.iter().enumerate() {
            assert_eq!(index.find(&entries, name), Some(at), "{name}");
        }
        for absent in ["k17", "k100", "", "K0"] {
            assert_eq!(index.find(&entries, absent), None, "{absent}");
        }
    }

    #[test]
    fn equality_ignores_the_index_but_not_the_kind() {
        let mut small = Table::new(TableKind::Dotted);
        let mut indexed = Table::new(TableKind::Dotted);
        for n in 0..20 {
            small.entries.push((key(&format!("k{n}"), 0), int(n)));
            indexed.push(key(&format!("k{n}"), 0), int(n));
        }
        assert!(small.index.is_none() && indexed.index.is_some());
        assert_eq!(small, indexed);
        indexed.set_kind(TableKind::Inline);
        assert_ne!(small, indexed);
        assert_eq!(indexed.kind(), TableKind::Inline);
    }

    #[test]
    fn debug_shows_the_kind_and_entries() {
        let mut table = Table::new(TableKind::Inline);
        table.push(key("a", 0), int(1));
        let debug = format!("{table:?}");
        assert!(debug.starts_with("Table { kind: Inline, entries: [(Key { name: \"a\""));
        assert!(!debug.contains("index"));
    }

    #[test]
    fn shrink_and_settle_leave_no_spare_room() {
        for settle in [false, true] {
            let mut table = Table::new(TableKind::Inline);
            for n in 0..11 {
                table.push(key(&format!("k{n}"), 0), int(n));
            }
            assert!(table.entries.capacity() > 11);
            if settle {
                table.settle();
            } else {
                table.shrink();
            }
            assert_eq!(table.entries.capacity(), 11);
            assert_eq!(table.position("k10"), Some(10));
        }
        assert_eq!(Table::new(TableKind::Inline).entries.capacity(), 0);
    }

    #[test]
    fn room_grows_by_a_quarter_from_one_entry() {
        let mut table = Table::new(TableKind::Header);
        let mut capacities = Vec::new();
        for n in 0..20 {
            table.push(key(&format!("k{n}"), 0), int(n));
            capacities.push(table.entries.capacity());
        }
        assert_eq!(
            capacities,
            [
                1, 2, 3, 4, 5, 6, 7, 8, 10, 10, 12, 12, 15, 15, 15, 18, 18, 18, 22, 22
            ]
        );
        let mut items: Vec<u8> = Vec::new();
        for _ in 0..1000 {
            reserve_one(&mut items);
            items.push(0);
            assert!(items.capacity() * 4 <= items.len() * 5 + 4);
        }
    }
}
