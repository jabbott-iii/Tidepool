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

//! A glob of an ignore file's pattern, compiled to a few instructions and
//! matched by stepping a set of states through the text.
//!
//! New in Tidepool (M4.7, ADR-0067). It reads globs as Git's `wildmatch`
//! does with `WM_PATHNAME` (Git's `wildmatch.c`), byte by byte, with case
//! kept:
//!
//! - `\` makes the next byte literal; a glob that ends in it never
//!   matches.
//! - `?` is one byte, `*` a run of bytes (maybe none), neither of them `/`.
//! - `[...]` is one byte of a set, never `/`: `!` or `^` first negates it;
//!   a `]` first is a member; `a-z` is a range of bytes, unless the `-` is
//!   first or last; `\` escapes; `[:alpha:]` and its eleven siblings are
//!   ASCII classes. A set that is not closed, or names a class that is not
//!   one, makes the whole glob never match, as `wildmatch` aborts there.
//! - `**` between the start or a `/` and the end or a `/` crosses `/`:
//!   `**/` is nothing or any run that ends in `/`, and a final `**` is any
//!   run at all. Anywhere else, a run of `*` is one `*`. `**/**/` is
//!   compiled as one `**/`, which matches the same, so a pattern of many
//!   costs no more than one.
//!
//! `wildmatch` backtracks, which takes time exponential in the number of
//! `*` for some globs; this steps every live state at once, in time
//! proportional to the text's length times the program's, which has at
//! most one instruction for each byte of the glob.

/// One instruction, 12 bytes. Each one that reads a byte, on a match,
/// goes on to the next; the end of the program is the match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Inst {
    /// This byte.
    Byte(u8),
    /// Any byte but `/`.
    One,
    /// A byte of the set at this index of [`Glob::sets`], never `/`.
    Set(u32),
    /// A run of bytes but `/`, maybe empty: it stays here on such a byte,
    /// and can go on to the next instruction without reading one.
    Star,
    /// A run of any bytes, maybe empty, as [`Inst::Star`] runs.
    AnyRun,
    /// Both of these instructions, without reading a byte.
    Split(u32, u32),
}

/// A compiled glob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Glob {
    /// The instructions, or, for a glob of literal bytes only, nothing:
    /// [`Glob::literal`] says what to compare instead.
    program: Vec<Inst>,
    /// The bytes of a glob with no wildcard, compared as they are.
    literal: Option<Box<[u8]>>,
    /// The sets of `[...]`, each 256 bits, one for each byte.
    sets: Vec<[u64; 4]>,
}

/// Room for the states of one match, kept between matches so that a match
/// allocates nothing once it has grown to the longest program.
#[derive(Debug, Default)]
pub(crate) struct Scratch {
    current: States,
    next: States,
}

/// A set of program positions: the list of those that read a byte, and
/// whether each position, splits included, has been reached.
#[derive(Debug, Default)]
struct States {
    list: Vec<usize>,
    member: Vec<bool>,
    /// Every position marked in `member`, splits too, so that a reset
    /// clears exactly those.
    marked: Vec<usize>,
    /// Positions still to follow through splits and runs.
    stack: Vec<usize>,
}

impl States {
    /// Empties the set, sized for a program of `len` instructions.
    fn reset(&mut self, len: usize) {
        for &pc in &self.marked {
            if let Some(member) = self.member.get_mut(pc) {
                *member = false;
            }
        }
        self.marked.clear();
        self.list.clear();
        if self.member.len() < len + 1 {
            self.member.resize(len + 1, false);
        }
    }

    /// Adds `pc` and every position reached from it without reading a
    /// byte: through splits, and past runs. The position of the end
    /// (`program.len()`) is the match.
    fn add(&mut self, program: &[Inst], pc: usize) {
        self.stack.push(pc);
        while let Some(pc) = self.stack.pop() {
            match self.member.get(pc) {
                Some(false) => {}
                _ => continue,
            }
            self.member[pc] = true;
            self.marked.push(pc);
            match program.get(pc) {
                Some(&Inst::Split(first, second)) => {
                    self.stack.push(second as usize);
                    self.stack.push(first as usize);
                }
                Some(Inst::Star | Inst::AnyRun) => {
                    self.list.push(pc);
                    self.stack.push(pc + 1);
                }
                _ => self.list.push(pc),
            }
        }
    }

    /// Whether `pc` is in the set.
    fn has(&self, pc: usize) -> bool {
        self.member.get(pc).copied().unwrap_or(false)
    }
}

impl Glob {
    /// Compiles `glob`, or `None` for one that never matches: one that ends
    /// in a `\`, or has a set that is not closed or names a class that is
    /// not one.
    pub(crate) fn compile(glob: &[u8]) -> Option<Glob> {
        let mut program = Vec::new();
        let mut sets = Vec::new();
        let mut wild = false;
        // Where the last `**/` ended in the program, so that the next one,
        // right after it, adds nothing.
        let mut dirs_end = None;
        let mut at = 0;
        while let Some(&byte) = glob.get(at) {
            match byte {
                b'\\' => {
                    program.push(Inst::Byte(*glob.get(at + 1)?));
                    at += 2;
                }
                b'?' => {
                    wild = true;
                    program.push(Inst::One);
                    at += 1;
                }
                b'[' => {
                    wild = true;
                    let (set, end) = set(glob, at)?;
                    program.push(Inst::Set(u32::try_from(sets.len()).ok()?));
                    sets.push(set);
                    at = end + 1;
                }
                b'*' => {
                    wild = true;
                    let mut end = at;
                    while glob.get(end) == Some(&b'*') {
                        end += 1;
                    }
                    let after_slash = at == 0 || glob[at - 1] == b'/';
                    let next = glob.get(end).copied();
                    let escaped_slash = next == Some(b'\\') && glob.get(end + 1) == Some(&b'/');
                    let double = end - at >= 2 && after_slash;
                    if double && next == Some(b'/') {
                        // `**/`: nothing, or any run that ends in `/`.
                        if dirs_end != Some(program.len()) {
                            let split = u32::try_from(program.len()).ok()?;
                            program.push(Inst::Split(split + 1, split + 3));
                            program.push(Inst::AnyRun);
                            program.push(Inst::Byte(b'/'));
                            dirs_end = Some(program.len());
                        }
                        at = end + 1;
                    } else if double && (next.is_none() || escaped_slash) {
                        // `wildmatch` tries "nothing" only before a plain
                        // `/`: before `\/` the `**` crosses `/`, and the
                        // escaped slash must follow.
                        program.push(Inst::AnyRun);
                        at = end;
                    } else {
                        program.push(Inst::Star);
                        at = end;
                    }
                }
                _ => {
                    program.push(Inst::Byte(byte));
                    at += 1;
                }
            }
        }
        let literal = (!wild).then(|| {
            program
                .iter()
                .filter_map(|inst| match inst {
                    Inst::Byte(byte) => Some(*byte),
                    _ => None,
                })
                .collect()
        });
        if literal.is_some() {
            program = Vec::new();
        }
        program.shrink_to_fit();
        sets.shrink_to_fit();
        Some(Glob {
            program,
            literal,
            sets,
        })
    }

    /// Whether the glob matches all of `text`.
    pub(crate) fn matches(&self, text: &[u8], scratch: &mut Scratch) -> bool {
        if let Some(literal) = &self.literal {
            return **literal == *text;
        }
        let program = &self.program;
        let end = program.len();
        let Scratch { current, next } = scratch;
        current.reset(end);
        next.reset(end);
        current.add(program, 0);
        for &byte in text {
            if current.list.is_empty() {
                return false;
            }
            for &pc in &current.list {
                let to = match program.get(pc) {
                    Some(Inst::Byte(want)) if byte == *want => pc + 1,
                    Some(Inst::One) if byte != b'/' => pc + 1,
                    Some(Inst::Set(index))
                        if byte != b'/'
                            && self
                                .sets
                                .get(*index as usize)
                                .is_some_and(|set| has(set, byte)) =>
                    {
                        pc + 1
                    }
                    Some(Inst::Star) if byte != b'/' => pc,
                    Some(Inst::AnyRun) => pc,
                    // No match, the end, or a split, which `add` never
                    // lists.
                    _ => continue,
                };
                next.add(program, to);
            }
            std::mem::swap(current, next);
            next.reset(end);
        }
        current.has(end)
    }
}

/// Whether `byte` is a member of `set`.
fn has(set: &[u64; 4], byte: u8) -> bool {
    (set[usize::from(byte >> 6)] >> (byte & 63)) & 1 == 1
}

/// The set of the `[` at `open`, and the index of its `]`; `None` if it is
/// not closed or names a class that is not one. Follows `wildmatch`'s
/// reading, step for step.
fn set(glob: &[u8], open: usize) -> Option<([u64; 4], usize)> {
    let mut members = [0u64; 4];
    let mut add = |byte: u8| members[usize::from(byte >> 6)] |= 1 << (byte & 63);
    let mut at = open + 1;
    let negated = matches!(glob.get(at), Some(b'!' | b'^'));
    if negated {
        at += 1;
    }
    // The byte before, for a range; none after a range or a class.
    let mut previous: Option<u8> = None;
    let mut first = true;
    loop {
        let byte = *glob.get(at)?;
        if byte == b']' && !first {
            break;
        }
        first = false;
        if byte == b'\\' {
            at += 1;
            let escaped = *glob.get(at)?;
            add(escaped);
            previous = Some(escaped);
        } else if let (b'-', Some(low)) = (byte, previous)
            && glob.get(at + 1).is_some_and(|&next| next != b']')
        {
            at += 1;
            let mut high = glob[at];
            if high == b'\\' {
                at += 1;
                high = *glob.get(at)?;
            }
            for member in low..=high {
                add(member);
            }
            previous = None;
        } else if byte == b'[' && glob.get(at + 1) == Some(&b':') {
            let start = at + 2;
            let close = start + glob.get(start..)?.iter().position(|&b| b == b']')?;
            if close > start && glob[close - 1] == b':' {
                for member in 0..=u8::MAX {
                    if in_class(&glob[start..close - 1], member)? {
                        add(member);
                    }
                }
                at = close;
                previous = None;
            } else {
                // No `:]`: the `[` is a member, and the `:` comes next.
                add(b'[');
                previous = Some(b'[');
            }
        } else {
            add(byte);
            previous = Some(byte);
        }
        at += 1;
    }
    if negated {
        for word in &mut members {
            *word = !*word;
        }
    }
    Some((members, at))
}

/// Whether `byte` is in the class named `name`, as Git's `sane_ctype`
/// classifies ASCII; `None` for a name that is not a class. Git's space is
/// tab, line feed, carriage return and space, not vertical tab or form
/// feed.
fn in_class(name: &[u8], byte: u8) -> Option<bool> {
    let space = matches!(byte, b'\t' | b'\n' | b'\r' | b' ');
    Some(match name {
        b"alnum" => byte.is_ascii_alphanumeric(),
        b"alpha" => byte.is_ascii_alphabetic(),
        b"blank" => matches!(byte, b' ' | b'\t'),
        b"cntrl" => byte.is_ascii_control(),
        b"digit" => byte.is_ascii_digit(),
        b"graph" => (0x20..0x7f).contains(&byte) && !space,
        b"lower" => byte.is_ascii_lowercase(),
        b"print" => (0x20..0x7f).contains(&byte),
        b"punct" => byte.is_ascii_punctuation(),
        b"space" => space,
        b"upper" => byte.is_ascii_uppercase(),
        b"xdigit" => byte.is_ascii_hexdigit(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(glob: &str, text: &str) -> bool {
        Glob::compile(glob.as_bytes())
            .is_some_and(|glob| glob.matches(text.as_bytes(), &mut Scratch::default()))
    }

    #[test]
    fn literals_and_wildcards() {
        assert!(matches("abc", "abc"));
        assert!(!matches("abc", "abcd"));
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "a/c"));
        assert!(matches("*.log", "x.log"));
        assert!(matches("*.log", ".log"));
        assert!(!matches("*.log", "a/x.log"));
        assert!(matches(r"\*", "*"));
        assert!(!matches(r"\*", "x"));
        assert!(!matches("bs\\", "bs\\"), "a final backslash never matches");
        assert!(!matches("bs\\", "bs"));
    }

    #[test]
    fn double_stars_cross_slashes_only_between_them() {
        assert!(matches("x**y", "xzy"));
        assert!(!matches("x**y", "x/y"));
        assert!(matches("**/deep", "deep"));
        assert!(matches("**/deep", "a/b/deep"));
        assert!(!matches("**/deep", "adeep"));
        assert!(matches("top/**", "top/a"));
        assert!(matches("top/**", "top/a/b"));
        assert!(!matches("top/**", "top"));
        assert!(matches("mid/**/end", "mid/end"));
        assert!(matches("mid/**/end", "mid/a/b/end"));
        assert!(!matches("mid/**/end", "mid/aend"));
        assert!(matches("**", "a/b/c"));
        assert!(matches("***/x", "a/x"));
        assert!(matches("a/**b", "a/xb"), "`**b` is `*b`");
        assert!(!matches("a/**b", "a/x/b"));
        assert!(matches(r"**\/x", "a/x"));
        assert!(
            !matches(r"**\/x", "x"),
            "nothing is tried before a plain `/` only"
        );
    }

    #[test]
    fn sets_follow_wildmatch() {
        assert!(!matches("a[/]b", "a/b"));
        assert!(matches("[!a]x", "bx"));
        assert!(!matches("[!a]x", "ax"));
        assert!(matches("[^b]y", "cy"));
        assert!(matches("[]]z", "]z"));
        assert!(matches("[a-]w", "-w"));
        assert!(matches("[a-]w", "aw"));
        assert!(!matches("[a-]w", "bw"));
        assert!(matches("[a-c]", "b"));
        assert!(!matches("[c-a]", "b"), "a reversed range is empty");
        assert!(matches(r"[\]]", "]"));
        assert!(matches("[[:digit:]]n", "5n"));
        assert!(matches("[[:space:]]", "\r"));
        assert!(!matches("[[:space:]]", "\u{b}"));
        assert!(
            !matches("[[:nope:]]", "n"),
            "an unknown class never matches"
        );
        assert!(!matches("un[closed", "un[closed"));
        assert!(matches("[[:a]", ":"), "no `:]`, so `[` and `:` are members");
        assert!(matches("[[:a]", "["));
        assert!(
            !matches("[!a]", "/"),
            "a negated set does not take `/` either"
        );
        assert!(matches("\u{e9}", "\u{e9}"));
        assert!(!matches("?", "\u{e9}"), "`?` is one byte, as in Git");
        assert!(matches("??", "\u{e9}"));
    }

    #[test]
    fn repeated_double_star_slashes_compile_as_one() {
        let one = Glob::compile(b"**/x").unwrap();
        let many = Glob::compile(
            &b"**/"
                .repeat(1000)
                .into_iter()
                .chain(*b"x")
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(many.program, one.program);
        assert!(matches("**/**/x", "x"));
        assert!(matches("**/**/x", "a/b/x"));
        assert!(matches("a/**/**/b", "a/b"));
        assert!(matches("a/**/**/b", "a/x/y/b"));
        assert!(!matches("a/**/**/b", "a/xb"));
    }

    #[test]
    fn a_literal_keeps_no_program() {
        let literal = Glob::compile(b"target").unwrap();
        assert_eq!(literal.program.capacity(), 0);
    }

    #[test]
    fn room_reused_across_globs_and_texts_keeps_no_state() {
        let mut scratch = Scratch::default();
        let deep = Glob::compile(b"**/deep").unwrap();
        let star = Glob::compile(b"*.log").unwrap();
        for _ in 0..3 {
            assert!(!deep.matches(b"a", &mut scratch));
            assert!(deep.matches(b"a/b/deep", &mut scratch));
            assert!(star.matches(b"x.log", &mut scratch));
            assert!(deep.matches(b"deep", &mut scratch));
        }
    }

    #[test]
    fn an_instruction_is_12_bytes() {
        assert_eq!(std::mem::size_of::<Inst>(), 12);
    }

    #[test]
    fn many_stars_take_linear_time() {
        let glob = "*a".repeat(200) + "b";
        let text = "a".repeat(4000);
        let start = std::time::Instant::now();
        assert!(!matches(&glob, &text));
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}
