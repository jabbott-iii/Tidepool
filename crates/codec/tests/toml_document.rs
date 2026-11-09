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

//! `toml::Document` through its public API, with exact output: an edit
//! keeps everything else in the text (roadmap M1.5's acceptance), each rule
//! of where a value is replaced or added, what ADR-0031 says the writer may
//! create, and every refusal. The module documentation of `toml`
//! ("Editing") is the specification; toml-test's cases and generated
//! documents are edited in `tests/toml.rs`.

use tidepool_codec::toml::{
    self, Array, Date, Datetime, Document, EditErrorKind, ErrorKind, Offset, Step, Table, Time,
    Value, ValueKind,
};
use tidepool_codec::{CodecError, Limits};

fn int(value: i64) -> ValueKind {
    ValueKind::Integer(value)
}

fn string(text: &str) -> ValueKind {
    ValueKind::String(text.to_string())
}

/// `text` after setting `path` to `value`, which must succeed.
fn edited(text: &str, path: &[Step<'_>], value: &ValueKind) -> String {
    let mut document = Document::parse(text, Limits::DEFAULT)
        .unwrap_or_else(|error| panic!("{text:?} does not parse: {error}"));
    document
        .set(path, value)
        .unwrap_or_else(|error| panic!("{path:?} on {text:?}: {error}"));
    let result = document.into_string();
    // The tree a document keeps is always the parser's for its text.
    toml::parse(&result, Limits::DEFAULT)
        .unwrap_or_else(|error| panic!("{result:?} does not parse: {error}"));
    result
}

/// Checks each `(text, path, expected)`, setting `value`.
fn expect_all(value: &ValueKind, cases: &[(&str, &[Step<'_>], &str)]) {
    for (text, path, expected) in cases {
        assert_eq!(edited(text, path, value), *expected, "{path:?} on {text:?}");
    }
}

/// Whether two values hold the same data, ignoring spans and how tables
/// were written; floats by their bits.
fn same(a: &ValueKind, b: &ValueKind) -> bool {
    match (a, b) {
        (ValueKind::Table(a), ValueKind::Table(b)) => {
            a.len() == b.len()
                && a.entries()
                    .iter()
                    .zip(b.entries())
                    .all(|((a_key, a), (b_key, b))| {
                        a_key.name == b_key.name && same(&a.kind, &b.kind)
                    })
        }
        (ValueKind::Array(a), ValueKind::Array(b)) => {
            a.len() == b.len()
                && a.items()
                    .iter()
                    .zip(b.items())
                    .all(|(a, b)| same(&a.kind, &b.kind))
        }
        (ValueKind::Float(a), ValueKind::Float(b)) => a.to_bits() == b.to_bits(),
        _ => a == b,
    }
}

/// `text` with LF line breaks as CR LF.
fn crlf(text: &str) -> String {
    text.replace('\n', "\r\n")
}

// ---------------------------------------------------------- acceptance

/// A settings file written by hand, with comments everywhere, blank lines,
/// odd spacing, quoted keys, hex and underscored numbers, literal strings,
/// a multi-line array and an array of tables.
const SETTINGS: &str = r#"# Tidepool settings.
#   Edited by hand; comments everywhere.

version   =   1   # the format

[editor]
  font-size = 0x0E      # hex, on purpose
  "tab width" = 4
  rulers = [ 80,  # soft
             100, ]   # hard
  wrap = 'soft'
  scroll-off = 1_000

# Themes below.
[theme]
name = "tidepool-dark" # default
'path with spaces' = 'C:\themes\mine.toml'

[[keymap]]
keys = "ctrl+s"   # save
command = 'save'
"#;

#[test]
fn editing_a_setting_keeps_everything_else() {
    let replaced = SETTINGS.replace("\"tab width\" = 4", "\"tab width\" = 2");
    let added = SETTINGS.replace(
        "  scroll-off = 1_000\n",
        "  scroll-off = 1_000\n  line-numbers = true\n",
    );
    // The same file with CR LF, with a byte-order mark, and without a
    // final line break, which each edit keeps.
    let copies: [fn(&str) -> String; 4] = [
        |text| text.to_string(),
        crlf,
        |text| format!("\u{feff}{text}"),
        |text| text.strip_suffix('\n').unwrap_or(text).to_string(),
    ];
    for copy in copies {
        let text = copy(SETTINGS);
        // Unchanged, it writes back byte for byte.
        let document = Document::parse(&text, Limits::DEFAULT).expect("parses");
        assert_eq!(document.as_str(), text);
        assert_eq!(
            edited(&text, &["editor".into(), "tab width".into()], &int(2)),
            copy(&replaced)
        );
        assert_eq!(
            edited(
                &text,
                &["editor".into(), "line-numbers".into()],
                &ValueKind::Boolean(true)
            ),
            copy(&added)
        );
    }
}

#[test]
fn setting_an_equal_value_changes_nothing() {
    let text = "a = 0x10\nb = 1_000\nc = 'lit'\nd = 07:30\ne = -nan\nf = 0.0\n\
                [t] # c\nx = 1\n[[u]]\n";
    let time = Time {
        hour: 7,
        minute: 30,
        second: 0,
        nanosecond: 0,
    };
    let mut x = Table::new_inline();
    x.insert("x", int(1));
    let mut document = Document::parse(text, Limits::DEFAULT).expect("parses");
    for (name, value) in [
        ("a", int(16)),
        ("b", int(1000)),
        ("c", string("lit")),
        ("d", ValueKind::Datetime(Datetime::LocalTime(time))),
        ("e", ValueKind::Float(-f64::NAN)),
        // The same data in a header's table, and in an array of tables.
        ("t", ValueKind::Table(x)),
        (
            "u",
            ValueKind::Array(Array::new(vec![Value::from(ValueKind::Table(
                Table::new_inline(),
            ))])),
        ),
    ] {
        document.set(&[name.into()], &value).expect("the same data");
        assert_eq!(document.as_str(), text, "{name}");
    }
    // Every leaf set to its own value, as a "save every setting" pass does.
    let leaves: Vec<(String, ValueKind)> = document
        .root()
        .entries()
        .iter()
        .map(|(key, value)| (key.name.clone(), value.kind.clone()))
        .collect();
    for (name, value) in &leaves {
        document.set(&[name.as_str().into()], value).expect("set");
    }
    assert_eq!(document.as_str(), text);
    // Two NaNs of one sign are the same data, whatever their payloads: a
    // negative NaN with another payload than `-nan` reads as is the same.
    let other_payload = f64::from_bits(0xFFF8_0000_0000_0001);
    assert!(other_payload.is_nan() && other_payload.to_bits() != (-f64::NAN).to_bits());
    document
        .set(&["e".into()], &ValueKind::Float(other_payload))
        .expect("the same data");
    assert_eq!(document.as_str(), text);
    // So a NaN with any payload is written `nan` or `-nan`, and set.
    let signalling = f64::from_bits(0x7FF0_0000_0000_0001);
    assert!(signalling.is_nan() && signalling.to_bits() != f64::NAN.to_bits());
    assert_eq!(
        edited("e = 1.0\n", &["e".into()], &ValueKind::Float(signalling)),
        "e = nan\n"
    );
    // A NaN of the other sign, and -0.0 for 0.0, are other data.
    assert_eq!(
        edited(text, &["e".into()], &ValueKind::Float(f64::NAN)),
        text.replace("e = -nan", "e = nan")
    );
    assert_eq!(
        edited(text, &["f".into()], &ValueKind::Float(-0.0)),
        text.replace("f = 0.0", "f = -0.0")
    );
}

// ----------------------------------------------------------- replacing

#[test]
fn replacing_keeps_the_comment_after_the_value() {
    expect_all(
        &string("x"),
        &[
            (
                "a = 1   # one\nb = 2\n",
                &["a".into()],
                "a = \"x\"   # one\nb = 2\n",
            ),
            ("a=1#one", &["a".into()], "a=\"x\"#one"),
            (
                "[t]\n  a  =  'y'  # y\r\n",
                &["t".into(), "a".into()],
                "[t]\n  a  =  \"x\"  # y\r\n",
            ),
        ],
    );
}

#[test]
fn replacing_a_multi_line_value_writes_one_line() {
    let mut z = Table::new_inline();
    z.insert("z", int(3));
    for (text, value, expected) in [
        (
            "s = \"\"\"\nline one\nline two\n\"\"\"  # c\nt = 1\n",
            string("x"),
            "s = \"x\"  # c\nt = 1\n",
        ),
        (
            "s = [\n  1, # one\n  2, # two\n]\nt = 1\n",
            ValueKind::Array(Array::new(vec![Value::from(int(3))])),
            "s = [3]\nt = 1\n",
        ),
        (
            "s = {\n  x = 1, # c\n  y = 2,\n}\nt = 1\n",
            ValueKind::Table(z),
            "s = { z = 3 }\nt = 1\n",
        ),
    ] {
        assert_eq!(edited(text, &["s".into()], &value), expected);
    }
}

#[test]
fn replacing_inside_a_multi_line_inline_table_keeps_its_layout() {
    let text = "s = {\n  x = 1, # c\n  y = [\n    2,\n  ],\n}\n";
    assert_eq!(
        edited(text, &["s".into(), "x".into()], &int(5)),
        "s = {\n  x = 5, # c\n  y = [\n    2,\n  ],\n}\n"
    );
    assert_eq!(
        edited(text, &["s".into(), "y".into(), 0usize.into()], &int(5)),
        "s = {\n  x = 1, # c\n  y = [\n    5,\n  ],\n}\n"
    );
    assert_eq!(
        edited(text, &["s".into(), "z".into()], &int(5)),
        "s = {\n  x = 1, # c\n  y = [\n    2,\n  ], z = 5,\n}\n"
    );
}

#[test]
fn replacing_an_element_of_an_array() {
    let mut y = Table::new_inline();
    y.insert("y", int(3));
    assert_eq!(
        edited(
            "a = [1, 2, 3] # c\n",
            &["a".into(), 1usize.into()],
            &string("x")
        ),
        "a = [1, \"x\", 3] # c\n"
    );
    let text = "b = [{ x = 1 }, { x = 2 }]\n";
    assert_eq!(
        edited(text, &["b".into(), 1usize.into()], &ValueKind::Table(y)),
        "b = [{ x = 1 }, { y = 3 }]\n"
    );
    assert_eq!(
        edited(text, &["b".into(), 0usize.into(), "x".into()], &int(9)),
        "b = [{ x = 9 }, { x = 2 }]\n"
    );
}

#[test]
fn replacing_in_an_array_of_tables_element() {
    let text = "[[bind]]\nkeys = \"ctrl+s\"\ncommand = \"save\"\n\n\
                [[bind]]\nkeys = \"ctrl+q\"   # quit\ncommand = \"quit\"\n";
    assert_eq!(
        edited(
            text,
            &["bind".into(), 1usize.into(), "keys".into()],
            &string("ctrl+w")
        ),
        text.replace("\"ctrl+q\"", "\"ctrl+w\"")
    );
}

// -------------------------------------------------------------- adding

#[test]
fn root_keys_go_after_the_last_root_key() {
    expect_all(
        &int(7),
        &[
            (
                "# top\na = 1\nb = 2 # c\n\n# about t\n[t]\nx = 1\n",
                &["c".into()],
                "# top\na = 1\nb = 2 # c\nc = 7\n\n# about t\n[t]\nx = 1\n",
            ),
            ("  a = 1\n[t]\n", &["b".into()], "  a = 1\n  b = 7\n[t]\n"),
            ("a.b = 1\n", &["c".into()], "a.b = 1\nc = 7\n"),
            ("a = 1", &["b".into()], "a = 1\nb = 7"),
            (
                "a = \"\"\"\nx\n\"\"\"\n[t]\n",
                &["b".into()],
                "a = \"\"\"\nx\n\"\"\"\nb = 7\n[t]\n",
            ),
        ],
    );
}

#[test]
fn root_keys_go_above_the_first_header_and_its_comments() {
    expect_all(
        &int(7),
        &[
            // A blank line keeps the comment above it at the top.
            (
                "# file\n\n[t]\nx = 1\n",
                &["k".into()],
                "# file\n\nk = 7\n\n[t]\nx = 1\n",
            ),
            // A comment right above the header stays with it.
            (
                "# file\n\n# about t\n  # more\n[t]\n",
                &["k".into()],
                "# file\n\nk = 7\n\n# about t\n  # more\n[t]\n",
            ),
            ("[t]\n", &["k".into()], "k = 7\n\n[t]\n"),
            (
                "# a\n# b\n[[t]]\n",
                &["k".into()],
                "k = 7\n\n# a\n# b\n[[t]]\n",
            ),
            ("\n\n[t]\n", &["k".into()], "\n\nk = 7\n\n[t]\n"),
            (
                "\u{feff}# a\n[t]\n",
                &["k".into()],
                "\u{feff}k = 7\n\n# a\n[t]\n",
            ),
        ],
    );
}

#[test]
fn root_keys_in_empty_documents() {
    expect_all(
        &int(7),
        &[
            ("", &["k".into()], "k = 7\n"),
            ("\u{feff}", &["k".into()], "\u{feff}k = 7\n"),
            ("# c", &["k".into()], "# c\nk = 7"),
            ("# c\n", &["k".into()], "# c\nk = 7\n"),
            ("   ", &["k".into()], "   \nk = 7"),
            ("\r\n", &["k".into()], "\r\nk = 7\r\n"),
            // A new file takes a table the same way.
            ("", &["t".into(), "k".into()], "[t]\nk = 7\n"),
        ],
    );
}

#[test]
fn header_keys_go_after_the_last_key() {
    expect_all(
        &int(7),
        &[
            // The last value is a multi-line string.
            (
                "[t]\ns = '''\nx\n'''\n[u]\n",
                &["t".into(), "k".into()],
                "[t]\ns = '''\nx\n'''\nk = 7\n[u]\n",
            ),
            // A trailing comment stays below.
            (
                "[t]\na = 1\n# trailing\n\n[u]\n",
                &["t".into(), "k".into()],
                "[t]\na = 1\nk = 7\n# trailing\n\n[u]\n",
            ),
            // The indent is copied.
            (
                "[t]\n\ta = 1\n",
                &["t".into(), "k".into()],
                "[t]\n\ta = 1\n\tk = 7\n",
            ),
            // An empty section, and one whose header has a comment.
            (
                "[t]\n\n[u]\n",
                &["t".into(), "k".into()],
                "[t]\nk = 7\n\n[u]\n",
            ),
            (
                "[t] # the t\n[u]\n",
                &["t".into(), "k".into()],
                "[t] # the t\nk = 7\n[u]\n",
            ),
            // A header at the end, with no line break after it.
            ("[t]", &["t".into(), "k".into()], "[t]\nk = 7"),
            ("a = 1\n[t]", &["t".into(), "k".into()], "a = 1\n[t]\nk = 7"),
            // A header's table defined after its sub-table.
            (
                "[t.u]\nx = 1\n[t]\ny = 2\n",
                &["t".into(), "k".into()],
                "[t.u]\nx = 1\n[t]\ny = 2\nk = 7\n",
            ),
        ],
    );
}

#[test]
fn keys_in_an_array_of_tables_go_into_the_named_element() {
    let text = "[[a]]\nx = 0\n[a.sub]\ny = 0\n[[a]]\nx = 1\n[[a]]\n[a.sub]\ny = 2\n";
    for (index, expected) in [
        (
            0,
            "[[a]]\nx = 0\nk = 7\n[a.sub]\ny = 0\n[[a]]\nx = 1\n[[a]]\n[a.sub]\ny = 2\n",
        ),
        (
            1,
            "[[a]]\nx = 0\n[a.sub]\ny = 0\n[[a]]\nx = 1\nk = 7\n[[a]]\n[a.sub]\ny = 2\n",
        ),
        (
            2,
            "[[a]]\nx = 0\n[a.sub]\ny = 0\n[[a]]\nx = 1\n[[a]]\nk = 7\n[a.sub]\ny = 2\n",
        ),
    ] {
        let result = edited(text, &["a".into(), Step::Index(index), "k".into()], &int(7));
        assert_eq!(result, expected, "element {index}");
        let root = toml::parse(&result, Limits::DEFAULT).expect("parses");
        let elements = root.get("a").and_then(Value::as_array).expect("a");
        for (at, element) in elements.items().iter().enumerate() {
            assert_eq!(element.get("k").is_some(), at == index);
        }
    }
    // And into a sub-table of one.
    assert_eq!(
        edited(
            text,
            &["a".into(), 0usize.into(), "sub".into(), "k".into()],
            &int(7)
        ),
        "[[a]]\nx = 0\n[a.sub]\ny = 0\nk = 7\n[[a]]\nx = 1\n[[a]]\n[a.sub]\ny = 2\n"
    );
}

#[test]
fn dotted_keys_stay_with_their_siblings() {
    expect_all(
        &string("Mono"),
        &[
            (
                "[editor]\nfont.size = 13\ntab = 4\n",
                &["editor".into(), "font".into(), "family".into()],
                "[editor]\nfont.size = 13\nfont.family = \"Mono\"\ntab = 4\n",
            ),
            (
                "font.size = 13\ntab = 4\n[x]\n",
                &["font".into(), "family".into()],
                "font.size = 13\nfont.family = \"Mono\"\ntab = 4\n[x]\n",
            ),
            // A table made by dotted keys, with a header's table in it.
            (
                "[fruit]\napple.color = \"red\"\n\n[fruit.apple.texture]\nsmooth = true\n",
                &["fruit".into(), "apple".into(), "taste".into()],
                "[fruit]\napple.color = \"red\"\napple.taste = \"Mono\"\n\n\
                 [fruit.apple.texture]\nsmooth = true\n",
            ),
            (
                "a.b.c = 1\na.d = 2\n",
                &["a".into(), "b".into(), "e".into()],
                "a.b.c = 1\na.b.e = \"Mono\"\na.d = 2\n",
            ),
            // Quoted parts are written as they must be.
            (
                "[t]\n\"x y\".z = 1\n",
                &["t".into(), "x y".into(), "w".into()],
                "[t]\n\"x y\".z = 1\n\"x y\".w = \"Mono\"\n",
            ),
        ],
    );
}

#[test]
fn inline_tables_take_keys_inside_their_braces() {
    let k: &[Step<'_>] = &["t".into(), "k".into()];
    expect_all(
        &int(7),
        &[
            ("t = {}\n", k, "t = { k = 7 }\n"),
            ("t = { }\n", k, "t = { k = 7 }\n"),
            ("t = {\n}\n", k, "t = { k = 7\n}\n"),
            ("t = { # c\n}\n", k, "t = { k = 7 # c\n}\n"),
            ("t = { a = 1 }\n", k, "t = { a = 1, k = 7 }\n"),
            ("t = { a = 1, }\n", k, "t = { a = 1, k = 7, }\n"),
            (
                "t = { a = 1\n    , b = 2\n    }\n",
                k,
                "t = { a = 1\n    , b = 2, k = 7\n    }\n",
            ),
            (
                "t = { a.b = 1, c = 2 }\n",
                &["t".into(), "a".into(), "k".into()],
                "t = { a.b = 1, a.k = 7, c = 2 }\n",
            ),
            (
                "t = [{ a = 1 }, {}]\n",
                &["t".into(), 0usize.into(), "k".into()],
                "t = [{ a = 1, k = 7 }, {}]\n",
            ),
            (
                "t = [{ a = 1 }, {}]\n",
                &["t".into(), 1usize.into(), "k".into()],
                "t = [{ a = 1 }, { k = 7 }]\n",
            ),
            (
                "t = { u = { a = 1 } }\n",
                k,
                "t = { u = { a = 1 }, k = 7 }\n",
            ),
        ],
    );
}

#[test]
fn implicit_tables_get_their_header_after_their_subtree() {
    expect_all(
        &int(7),
        &[
            (
                "[a.b]\nx = 1\n",
                &["a".into(), "k".into()],
                "[a.b]\nx = 1\n\n[a]\nk = 7\n",
            ),
            // Inside an element of an array of tables.
            (
                "[[e]]\n[e.a.b]\nx = 1\n[[e]]\n",
                &["e".into(), 0usize.into(), "a".into(), "k".into()],
                "[[e]]\n[e.a.b]\nx = 1\n\n[e.a]\nk = 7\n\n[[e]]\n",
            ),
            // Under a table made by dotted keys.
            (
                "[x]\na.b = 1\n[x.a.c.d]\n",
                &["x".into(), "a".into(), "c".into(), "k".into()],
                "[x]\na.b = 1\n[x.a.c.d]\n\n[x.a.c]\nk = 7\n",
            ),
            // After the whole subtree, before what follows it.
            (
                "[a.b]\n[a.c.d]\ny = 1\n# end of a\n\n[z]\n",
                &["a".into(), "k".into()],
                "[a.b]\n[a.c.d]\ny = 1\n\n[a]\nk = 7\n\n# end of a\n\n[z]\n",
            ),
        ],
    );
}

#[test]
fn missing_tables_get_a_header_after_their_parent() {
    expect_all(
        &int(7),
        &[
            (
                "a = 1\n",
                &["t".into(), "k".into()],
                "a = 1\n\n[t]\nk = 7\n",
            ),
            // Trailing comment lines stay below.
            (
                "[x]\ny = 1\n\n# end\n",
                &["a".into(), "k".into()],
                "[x]\ny = 1\n\n[a]\nk = 7\n\n# end\n",
            ),
            (
                "[[e]]\nx = 0\n[[e]]\nx = 1\n[[e]]\nx = 2\n",
                &["e".into(), 1usize.into(), "t".into(), "k".into()],
                "[[e]]\nx = 0\n[[e]]\nx = 1\n\n[e.t]\nk = 7\n\n[[e]]\nx = 2\n",
            ),
            (
                "x = 1\n",
                &["a".into(), "b".into(), "c".into(), "k".into()],
                "x = 1\n\n[a.b.c]\nk = 7\n",
            ),
            (
                "[x]\ny = 1",
                &["t".into(), "k".into()],
                "[x]\ny = 1\n\n[t]\nk = 7",
            ),
            (
                "[x]\ny = 1\n[z]\n",
                &["x".into(), "t".into(), "k".into()],
                "[x]\ny = 1\n\n[x.t]\nk = 7\n\n[z]\n",
            ),
            (
                "[x]\n[x.y]\n[z]\n",
                &["x".into(), "t".into(), "k".into()],
                "[x]\n[x.y]\n\n[x.t]\nk = 7\n\n[z]\n",
            ),
        ],
    );
}

#[test]
fn missing_tables_under_dotted_and_inline_tables_are_dotted_keys() {
    expect_all(
        &int(7),
        &[
            (
                "[e]\nfont.size = 1\n",
                &["e".into(), "font".into(), "t".into(), "k".into()],
                "[e]\nfont.size = 1\nfont.t.k = 7\n",
            ),
            (
                "t = { a = 1 }\n",
                &["t".into(), "u".into(), "k".into()],
                "t = { a = 1, u.k = 7 }\n",
            ),
            (
                "t = {}\n",
                &["t".into(), "u".into(), "v".into(), "k".into()],
                "t = { u.v.k = 7 }\n",
            ),
        ],
    );
}

#[test]
fn crlf_documents_get_crlf_lines() {
    let text = "a = 1\r\n[t]\r\nx = 1\r\n";
    expect_all(
        &int(7),
        &[
            (text, &["b".into()], "a = 1\r\nb = 7\r\n[t]\r\nx = 1\r\n"),
            (
                text,
                &["t".into(), "y".into()],
                "a = 1\r\n[t]\r\nx = 1\r\ny = 7\r\n",
            ),
            (
                text,
                &["u".into(), "k".into()],
                "a = 1\r\n[t]\r\nx = 1\r\n\r\n[u]\r\nk = 7\r\n",
            ),
            ("a = 1\r\nb = 2", &["c".into()], "a = 1\r\nb = 2\r\nc = 7"),
            (
                "a = 1\r\nb = 2",
                &["t".into(), "k".into()],
                "a = 1\r\nb = 2\r\n\r\n[t]\r\nk = 7",
            ),
            (
                "\u{feff}[t]\r\n",
                &["k".into()],
                "\u{feff}k = 7\r\n\r\n[t]\r\n",
            ),
        ],
    );
}

#[test]
fn twenty_edits_in_a_row() {
    let mut document = Document::parse(SETTINGS, Limits::DEFAULT).expect("parses");
    let mut keys = Table::new_inline();
    keys.insert("keys", string("ctrl+q"));
    let edits: Vec<(Vec<Step<'_>>, ValueKind)> = vec![
        (vec!["version".into()], int(2)),
        (vec!["editor".into(), "font-size".into()], int(15)),
        (vec!["editor".into(), "tab width".into()], int(2)),
        (
            vec!["editor".into(), "rulers".into(), 1usize.into()],
            int(120),
        ),
        (vec!["editor".into(), "wrap".into()], string("none")),
        (
            vec!["editor".into(), "line-numbers".into()],
            ValueKind::Boolean(true),
        ),
        (
            vec!["editor".into(), "cursor".into(), "blink".into()],
            ValueKind::Boolean(false),
        ),
        (
            vec!["theme".into(), "name".into()],
            string("tidepool-light"),
        ),
        (
            vec!["theme".into(), "path with spaces".into()],
            string("D:\\themes"),
        ),
        (
            vec!["theme".into(), "contrast".into()],
            ValueKind::Float(1.5),
        ),
        (
            vec!["keymap".into(), 0usize.into(), "keys".into()],
            string("cmd+s"),
        ),
        (
            vec!["keymap".into(), 0usize.into(), "when".into()],
            keys_when(),
        ),
        (vec!["terminal".into(), "shell".into()], string("/bin/sh")),
        (
            vec!["terminal".into(), "font".into(), "size".into()],
            int(12),
        ),
        (vec!["schema".into()], int(1)),
        (
            vec!["editor".into(), "cursor".into(), "shape".into()],
            string("bar"),
        ),
        (
            vec!["keymap".into(), 0usize.into(), "when".into(), "mode".into()],
            string("insert"),
        ),
        (
            vec!["terminal".into(), "font".into(), "size".into()],
            int(13),
        ),
        (
            vec!["editor".into(), "rulers".into()],
            ValueKind::Array(Array::new(Vec::new())),
        ),
        (vec!["theme".into(), "extra".into()], ValueKind::Table(keys)),
    ];
    assert_eq!(edits.len(), 20);
    for (path, value) in &edits {
        document
            .set(path, value)
            .unwrap_or_else(|error| panic!("{path:?}: {error}"));
        // Each value reads back where it was set.
        let found = document.get(path).expect("the value set");
        assert!(same(&found.kind, value), "{path:?}: {found:?}");
    }
    assert_eq!(
        document.as_str(),
        r#"# Tidepool settings.
#   Edited by hand; comments everywhere.

version   =   2   # the format
schema = 1

[editor]
  font-size = 15      # hex, on purpose
  "tab width" = 2
  rulers = []   # hard
  wrap = "none"
  scroll-off = 1_000
  line-numbers = true

[editor.cursor]
blink = false
shape = "bar"

# Themes below.
[theme]
name = "tidepool-light" # default
'path with spaces' = 'D:\themes'
contrast = 1.5
extra = { keys = "ctrl+q" }

[[keymap]]
keys = "cmd+s"   # save
command = 'save'
when = { mode = "insert" }

[terminal]
shell = "/bin/sh"

[terminal.font]
size = 13
"#
    );
}

/// An inline table to add to a key binding.
fn keys_when() -> ValueKind {
    let mut when = Table::new_inline();
    when.insert("mode", string("normal"));
    ValueKind::Table(when)
}

// ------------------------------------------------------------ ADR-0031

#[test]
fn written_strings_never_use_1_1_escapes() {
    assert_eq!(
        edited("s = \"\\e\\x01\"\n", &["s".into()], &string("\u{1b}\u{1}x")),
        "s = \"\\u001B\\u0001x\"\n"
    );
    assert_eq!(
        edited("", &["t".into()], &string("\u{1b}")),
        "t = \"\\u001B\"\n"
    );
}

#[test]
fn written_times_have_seconds() {
    let text = "t = 07:32\nd = 1979-05-27T07:32\no = 1979-05-27 07:32z\n";
    let mut document = Document::parse(text, Limits::DEFAULT).expect("parses");
    for name in ["t", "d", "o"] {
        let mut value = *document
            .get(&[name.into()])
            .and_then(Value::as_datetime)
            .expect("a date-time");
        match &mut value {
            Datetime::LocalTime(time)
            | Datetime::LocalDatetime { time, .. }
            | Datetime::OffsetDatetime { time, .. } => time.minute = 33,
            Datetime::LocalDate(_) => panic!("a date"),
        }
        document
            .set(&[name.into()], &ValueKind::Datetime(value))
            .expect("set");
    }
    assert_eq!(
        document.as_str(),
        "t = 07:33:00\nd = 1979-05-27T07:33:00\no = 1979-05-27T07:33:00Z\n"
    );
}

#[test]
fn written_inline_tables_are_one_line() {
    let text = "src = {\n  a = 1, # c\n  b = [\n    2, # two\n  ],\n  c.d = 3,\n}\ndst = 0\n";
    let mut document = Document::parse(text, Limits::DEFAULT).expect("parses");
    let value = document.get(&["src".into()]).expect("src").kind.clone();
    document.set(&["dst".into()], &value).expect("set");
    assert_eq!(
        document.as_str(),
        "src = {\n  a = 1, # c\n  b = [\n    2, # two\n  ],\n  c.d = 3,\n}\n\
         dst = { a = 1, b = [2], c = { d = 3 } }\n"
    );
}

#[test]
fn adding_to_a_one_line_inline_table_adds_no_1_1_syntax() {
    let result = edited("t = { a = 1 }\n", &["t".into(), "b".into()], &int(7));
    assert_eq!(result, "t = { a = 1, b = 7 }\n");
    let braces = &result[result.find('{').expect("{")..=result.find('}').expect("}")];
    assert!(!braces.contains('\n') && !braces.contains('#') && !braces.contains(", }"));
}

// ----------------------------------------------------- errors and the API

/// A document's text and limits, a path and value to set there, and the
/// step and kind of the error that refuses it.
type Refusal<'p> = (
    &'static str,
    Limits,
    Vec<Step<'p>>,
    ValueKind,
    usize,
    EditErrorKind,
);

#[test]
fn refused_edits_change_nothing() {
    let small = |change: fn(&mut Limits)| {
        let mut limits = Limits::DEFAULT;
        change(&mut limits);
        limits
    };
    let date = Date {
        year: 2024,
        month: 1,
        day: 31,
    };
    let time = Time {
        hour: 1,
        minute: 2,
        second: 3,
        nanosecond: 0,
    };
    let bad_date = |date| ValueKind::Datetime(Datetime::LocalDate(date));
    let bad_time = |time| ValueKind::Datetime(Datetime::LocalTime(time));
    let mut deep = int(1);
    for _ in 0..4 {
        deep = ValueKind::Array(Array::new(vec![Value::from(deep)]));
    }
    let too_deep = |limit| EditErrorKind::Limit(ErrorKind::TooDeep { limit });
    let cases: Vec<Refusal<'_>> = vec![
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec![],
            int(7),
            0,
            EditErrorKind::EmptyPath,
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["a".into(), "b".into()],
            int(7),
            1,
            EditErrorKind::NotATable {
                found: "an integer",
            },
        ),
        (
            "a = [1]\n",
            Limits::DEFAULT,
            vec!["a".into(), "b".into()],
            int(7),
            1,
            EditErrorKind::NotATable { found: "an array" },
        ),
        (
            "[[a]]\n",
            Limits::DEFAULT,
            vec!["a".into(), "b".into()],
            int(7),
            1,
            EditErrorKind::NotATable {
                found: "an array of tables",
            },
        ),
        (
            "[a]\n",
            Limits::DEFAULT,
            vec!["a".into(), 0usize.into()],
            int(7),
            1,
            EditErrorKind::NotAnArray { found: "a table" },
        ),
        (
            "a = {}\n",
            Limits::DEFAULT,
            vec!["a".into(), 0usize.into()],
            int(7),
            1,
            EditErrorKind::NotAnArray {
                found: "an inline table",
            },
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec![0usize.into()],
            int(7),
            0,
            EditErrorKind::NotAnArray { found: "a table" },
        ),
        (
            "a = [1, 2]\n",
            Limits::DEFAULT,
            vec!["a".into(), 2usize.into()],
            int(7),
            1,
            EditErrorKind::IndexOutOfRange { len: 2 },
        ),
        (
            "[[a]]\n",
            Limits::DEFAULT,
            vec!["a".into(), 1usize.into(), "k".into()],
            int(7),
            1,
            EditErrorKind::IndexOutOfRange { len: 1 },
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["b".into(), 0usize.into()],
            int(7),
            1,
            EditErrorKind::MissingArray,
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["b".into(), "c".into(), 0usize.into(), "d".into()],
            int(7),
            2,
            EditErrorKind::MissingArray,
        ),
        (
            "[t]\nx = 1\n",
            Limits::DEFAULT,
            vec!["t".into()],
            int(7),
            0,
            EditErrorKind::NotReplaceable {
                found: "a table defined by a header",
            },
        ),
        (
            "[[t]]\n",
            Limits::DEFAULT,
            vec!["t".into(), 0usize.into()],
            int(7),
            1,
            EditErrorKind::NotReplaceable {
                found: "an element of an array of tables",
            },
        ),
        (
            "[a.b]\n",
            Limits::DEFAULT,
            vec!["a".into()],
            int(7),
            0,
            EditErrorKind::NotReplaceable {
                found: "a table made by a header's prefix",
            },
        ),
        (
            "a.b = 1\n",
            Limits::DEFAULT,
            vec!["a".into()],
            int(7),
            0,
            EditErrorKind::NotReplaceable {
                found: "a table made by dotted keys",
            },
        ),
        (
            "[[t]]\n",
            Limits::DEFAULT,
            vec!["t".into()],
            int(7),
            0,
            EditErrorKind::NotReplaceable {
                found: "an array of tables",
            },
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["b".into()],
            bad_date(Date {
                year: 10_000,
                ..date
            }),
            0,
            EditErrorKind::InvalidDatetime {
                expected: "a year from 0000 to 9999",
            },
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["a".into()],
            bad_date(Date { month: 13, ..date }),
            0,
            EditErrorKind::InvalidDatetime {
                expected: "a month from 01 to 12",
            },
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["a".into()],
            bad_date(Date {
                month: 2,
                day: 30,
                ..date
            }),
            0,
            EditErrorKind::InvalidDatetime {
                expected: "a day that the month has",
            },
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["a".into()],
            bad_time(Time { hour: 24, ..time }),
            0,
            EditErrorKind::InvalidDatetime {
                expected: "an hour from 00 to 23",
            },
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["a".into()],
            bad_time(Time { minute: 60, ..time }),
            0,
            EditErrorKind::InvalidDatetime {
                expected: "a minute from 00 to 59",
            },
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["a".into()],
            bad_time(Time { second: 61, ..time }),
            0,
            EditErrorKind::InvalidDatetime {
                expected: "seconds from 00 to 60",
            },
        ),
        (
            "a = 1\n",
            Limits::DEFAULT,
            vec!["a".into()],
            bad_time(Time {
                nanosecond: 1_000_000_000,
                ..time
            }),
            0,
            EditErrorKind::InvalidDatetime {
                expected: "a fraction of a second of 0 to 999999999 nanoseconds",
            },
        ),
        (
            "t = {}\n",
            Limits::DEFAULT,
            vec!["t".into(), "k".into()],
            ValueKind::Array(Array::new(vec![Value::from(ValueKind::Datetime(
                Datetime::OffsetDatetime {
                    date,
                    time,
                    offset: Offset::Minutes(24 * 60),
                },
            ))])),
            1,
            EditErrorKind::InvalidDatetime {
                expected: "an offset of 00 to 23 hours",
            },
        ),
        (
            "a = 1\n",
            small(|limits| limits.max_input_bytes = 16),
            vec!["b".into()],
            string(&"x".repeat(20)),
            1,
            EditErrorKind::TooLarge { limit: 16 },
        ),
        (
            "a = 1\n",
            small(|limits| limits.max_input_bytes = 16),
            vec!["a".into()],
            string(&"x".repeat(20)),
            1,
            EditErrorKind::TooLarge { limit: 16 },
        ),
        (
            "a = 1\n",
            small(|limits| limits.max_depth = 3),
            vec!["a".into()],
            deep.clone(),
            0,
            too_deep(3),
        ),
        (
            "a = 1\n",
            small(|limits| limits.max_depth = 3),
            vec!["t".into(), "u".into(), "v".into(), "w".into(), "k".into()],
            int(7),
            4,
            too_deep(3),
        ),
        (
            "a = 1\nb = 2\n",
            small(|limits| limits.max_entries = 2),
            vec!["c".into()],
            int(7),
            1,
            EditErrorKind::Limit(ErrorKind::TooManyEntries { limit: 2 }),
        ),
        (
            "a = 1\n",
            small(|limits| limits.max_string_bytes = 4),
            vec!["a".into()],
            string("hello"),
            1,
            EditErrorKind::Limit(ErrorKind::StringTooLong { limit: 4 }),
        ),
        (
            "a = 1\n",
            small(|limits| limits.max_string_bytes = 4),
            vec!["long key".into()],
            int(7),
            1,
            EditErrorKind::Limit(ErrorKind::StringTooLong { limit: 4 }),
        ),
        (
            "a = 1\n",
            small(|limits| limits.max_number_bytes = 4),
            vec!["a".into()],
            ValueKind::Float(1.5e300),
            1,
            EditErrorKind::Limit(ErrorKind::NumberTooLong { limit: 4 }),
        ),
    ];
    for (text, limits, path, value, step, kind) in cases {
        let mut document = Document::parse(text, limits).expect("parses");
        let before = format!("{:?}", document.root());
        let error = document.set(&path, &value);
        assert_eq!(
            error,
            Err(CodecError::TomlEdit {
                step,
                kind: kind.clone()
            }),
            "{path:?} on {text:?}"
        );
        assert_eq!(document.as_str(), text, "{kind:?}");
        assert_eq!(format!("{:?}", document.root()), before, "{kind:?}");
    }
    // A parse error is not an edit error.
    assert!(matches!(
        Document::parse("a = ", Limits::DEFAULT),
        Err(CodecError::Toml { .. })
    ));
}

#[test]
fn edits_up_to_each_limit_are_made() {
    let set = |text: &str, limits: Limits, path: &[Step<'_>], value: &ValueKind| {
        let mut document = Document::parse(text, limits).expect("parses");
        document.set(path, value).map(|()| document.into_string())
    };
    let input = |max_input_bytes| Limits {
        max_input_bytes,
        ..Limits::DEFAULT
    };
    let depth = |max_depth| Limits {
        max_depth,
        ..Limits::DEFAULT
    };
    let too = |step, kind| Err(CodecError::TomlEdit { step, kind });
    // An insertion and a replacement that end exactly at the input limit,
    // and one byte past it.
    let insert: &[Step<'_>] = &["b".into()];
    assert_eq!(
        set("a = 1\n", input(15), insert, &string("xx")).as_deref(),
        Ok("a = 1\nb = \"xx\"\n")
    );
    assert_eq!(
        set("a = 1\n", input(14), insert, &string("xx")),
        too(1, EditErrorKind::TooLarge { limit: 14 })
    );
    let replace: &[Step<'_>] = &["a".into()];
    assert_eq!(
        set("a = 1\n", input(10), replace, &int(12345)).as_deref(),
        Ok("a = 12345\n")
    );
    assert_eq!(
        set("a = 1\n", input(9), replace, &int(12345)),
        too(1, EditErrorKind::TooLarge { limit: 9 })
    );
    // A value, and a chain of new tables, exactly as deep as the limit,
    // and one level deeper.
    let deep = |levels| {
        let mut value = int(1);
        for _ in 0..levels {
            value = ValueKind::Array(Array::new(vec![Value::from(value)]));
        }
        value
    };
    let too_deep = EditErrorKind::Limit(ErrorKind::TooDeep { limit: 3 });
    let nested: &[Step<'_>] = &["t".into(), "u".into(), "k".into()];
    assert_eq!(
        set("a = 1\n", depth(3), nested, &deep(1)).as_deref(),
        Ok("a = 1\n\n[t.u]\nk = [1]\n")
    );
    assert_eq!(
        set("a = 1\n", depth(3), nested, &deep(2)),
        too(2, too_deep.clone())
    );
    let chain = ["x".into(), "y".into(), "z".into(), "k".into()];
    assert_eq!(
        set("a = 1\n", depth(3), &chain, &int(7)).as_deref(),
        Ok("a = 1\n\n[x.y.z]\nk = 7\n")
    );
    let longer = ["x".into(), "y".into(), "z".into(), "w".into(), "k".into()];
    assert_eq!(set("a = 1\n", depth(3), &longer, &int(7)), too(4, too_deep));
}

#[test]
fn get_reads_paths() {
    let text = "a = 1\nt = { b = [1, { c = 2 }] }\n[[e]]\nx = 3\n";
    let document = Document::parse(text, Limits::DEFAULT).expect("parses");
    let integer = |path: &[Step<'_>]| document.get(path).and_then(Value::as_integer);
    assert_eq!(integer(&["a".into()]), Some(1));
    assert_eq!(
        integer(&["t".into(), "b".into(), 1usize.into(), "c".into()]),
        Some(2)
    );
    assert_eq!(integer(&["e".into(), 0usize.into(), "x".into()]), Some(3));
    let span = document.get(&["t".into(), "b".into()]).expect("b").span;
    assert_eq!(&document.as_str()[span.range()], "[1, { c = 2 }]");
    for path in [
        vec![],
        vec![Step::Key("z")],
        vec![Step::Key("e"), Step::Key("x")],
        vec![Step::Key("a"), Step::Key("b")],
        vec![Step::Key("t"), Step::Key("b"), Step::Index(5)],
        vec![Step::Index(0)],
    ] {
        assert_eq!(document.get(&path), None, "{path:?}");
    }
    assert_eq!(Step::from("a"), Step::Key("a"));
    assert_eq!(Step::from(3usize), Step::Index(3));
    // The tree is the parser's.
    assert_eq!(
        format!("{:?}", document.root()),
        format!("{:?}", toml::parse(text, Limits::DEFAULT).expect("parses"))
    );
}

#[test]
fn built_values_are_written() {
    let mut table = Table::new_inline();
    assert_eq!(table.insert("a", int(1)), None);
    assert_eq!(table.insert("b c", Value::from(string("x"))), None);
    // Replacing an entry keeps its place.
    assert_eq!(table.insert("a", int(3)).map(|old| old.kind), Some(int(1)));
    table.insert(
        "list",
        ValueKind::Array(Array::new(vec![
            Value::from(ValueKind::Boolean(true)),
            Value::from(ValueKind::Table(Table::new_inline())),
        ])),
    );
    let value = ValueKind::Table(table);
    assert_eq!(
        edited("", &["k".into()], &value),
        "k = { a = 3, \"b c\" = \"x\", list = [true, {}] }\n"
    );
    // A batch: edit a clone, keep it only if every edit succeeds.
    let document = Document::parse("a = 1\n", Limits::DEFAULT).expect("parses");
    let mut batch = document.clone();
    batch.set(&["b".into()], &int(2)).expect("set");
    assert!(batch.set(&["a".into(), "c".into()], &int(3)).is_err());
    assert_eq!(document.as_str(), "a = 1\n");
    assert_eq!(batch.as_str(), "a = 1\nb = 2\n");
}
