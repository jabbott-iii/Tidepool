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

//! RFC 8259 conformance: a table of accept and reject cases modelled on
//! the categories of Nicolas Seriot's JSONTestSuite (`test_parsing/`):
//!
//! - `y_`: the RFC requires a parser to accept these.
//! - `n_`: the RFC requires a parser to reject these. A few are JSONC
//!   extensions, which [`Dialect::Jsonc`] accepts.
//! - `i_`: the RFC leaves these to the implementation. Each one records
//!   Tidepool's choice, as the `json` module documents it.
//!
//! The cases were written by hand from the suite's case names; the files
//! were not copied. Each case names the error kind it must fail with, in
//! each dialect, so that a rejection for the wrong reason fails the test;
//! some also give the byte offset it must fail at. An accepted case must
//! survive being written and read back bit for bit.
//!
//! JSON `\u` escapes are written `~u` and expanded before parsing, so that
//! an escape in this file cannot be mistaken for (or edited into) the
//! character it stands for.

mod common;

use common::identical;
use tidepool_codec::json::{self, Dialect, Value};
use tidepool_codec::{CodecError, Limits};

/// What a case must do in one dialect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// Parse.
    Accept,
    /// Fail with an error kind of this name.
    Reject(&'static str),
}

use Outcome::{Accept, Reject};

struct Case {
    name: &'static str,
    input: Vec<u8>,
    json: Outcome,
    jsonc: Outcome,
    /// Where a rejection must be reported, if the case says.
    offset: Option<usize>,
}

impl Case {
    /// The same case, which must be rejected at byte `offset` in each
    /// dialect that rejects it.
    fn at(self, offset: usize) -> Self {
        Self {
            offset: Some(offset),
            ..self
        }
    }
}

/// Turns each `~u` into `\u`.
fn expand(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut rest = input;
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'~' && tail.first() == Some(&b'u') {
            out.extend_from_slice(b"\\u");
            rest = tail.get(1..).unwrap_or_default();
        } else {
            out.push(byte);
            rest = tail;
        }
    }
    out
}

fn case(name: &'static str, input: impl AsRef<[u8]>, json: Outcome, jsonc: Outcome) -> Case {
    Case {
        name,
        input: expand(input.as_ref()),
        json,
        jsonc,
        offset: None,
    }
}

/// Accepted in both dialects.
fn y(name: &'static str, input: impl AsRef<[u8]>) -> Case {
    case(name, input, Accept, Accept)
}

/// Rejected in both dialects with the same kind of error.
fn n(name: &'static str, input: impl AsRef<[u8]>, kind: &'static str) -> Case {
    case(name, input, Reject(kind), Reject(kind))
}

/// Rejected in both dialects, for a different reason in each.
fn n2(
    name: &'static str,
    input: impl AsRef<[u8]>,
    json: &'static str,
    jsonc: &'static str,
) -> Case {
    case(name, input, Reject(json), Reject(jsonc))
}

/// Rejected by strict JSON; accepted as a JSONC extension.
fn c(name: &'static str, input: impl AsRef<[u8]>, json: &'static str) -> Case {
    case(name, input, Reject(json), Accept)
}

// Error kind names.
const CHAR: &str = "UnexpectedCharacter";
const END: &str = "UnexpectedEnd";
const UTF8: &str = "InvalidUtf8";
const LITERAL: &str = "InvalidLiteral";
const ESCAPE: &str = "InvalidEscape";
const SURROGATE: &str = "LoneSurrogate";
const CONTROL: &str = "ControlCharacter";
const ZERO: &str = "LeadingZero";
const RANGE: &str = "NumberOutOfRange";
const COMMENT: &str = "CommentNotAllowed";
const COMMA: &str = "TrailingCommaNotAllowed";
const DEEP: &str = "TooDeep";

/// The `y_` cases: what every JSON parser must accept.
fn accepted() -> Vec<Case> {
    vec![
        y("y_array_arraysWithSpaces", "[[]   ]"),
        y("y_array_empty", "[]"),
        y("y_array_empty-string", r#"[""]"#),
        y("y_array_ending_with_newline", "[\"a\"]\n"),
        y("y_array_false", "[false]"),
        y("y_array_heterogeneous", r#"[null, 1, "1", {}]"#),
        y("y_array_null", "[null]"),
        y("y_array_with_1_and_newline", "[1\n]"),
        y("y_array_with_leading_space", " [1]"),
        y("y_array_with_several_null", "[1,null,null,null,2]"),
        y("y_array_with_trailing_space", "[2] "),
        y("y_number", "[123e65]"),
        y("y_number_0e+1", "[0e+1]"),
        y("y_number_0e1", "[0e1]"),
        y("y_number_after_space", "[ 4]"),
        y(
            "y_number_double_close_to_zero",
            "[-0.000000000000000000000000000000000000000000000000000000000000000000000000000001]",
        ),
        y("y_number_int_with_exp", "[20e1]"),
        y("y_number_minus_zero", "[-0]"),
        y("y_number_negative_int", "[-123]"),
        y("y_number_negative_one", "[-1]"),
        y("y_number_negative_zero", "[-0]"),
        y("y_number_real_capital_e", "[1E22]"),
        y("y_number_real_capital_e_neg_exp", "[1E-2]"),
        y("y_number_real_capital_e_pos_exp", "[1E+2]"),
        y("y_number_real_exponent", "[123e45]"),
        y("y_number_real_fraction_exponent", "[123.456e78]"),
        y("y_number_real_neg_exp", "[1e-2]"),
        y("y_number_real_pos_exponent", "[1e+2]"),
        y("y_number_simple_int", "[123]"),
        y("y_number_simple_real", "[123.456789]"),
        y("y_object", r#"{"asd":"sdf", "dfg":"fgh"}"#),
        y("y_object_basic", r#"{"asd":"sdf"}"#),
        y("y_object_duplicated_key", r#"{"a":"b","a":"c"}"#),
        y("y_object_duplicated_key_and_value", r#"{"a":"b","a":"b"}"#),
        y("y_object_empty", "{}"),
        y("y_object_empty_key", r#"{"":0}"#),
        y("y_object_escaped_null_in_key", r#"{"foo~u0000bar": 42}"#),
        y(
            "y_object_extreme_numbers",
            r#"{ "min": -1.0e+28, "max": 1.0e+28 }"#,
        ),
        y(
            "y_object_long_strings",
            r#"{"x":[{"id": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}], "id": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}"#,
        ),
        y("y_object_simple", r#"{"a":[]}"#),
        y(
            "y_object_string_unicode",
            r#"{"title":"~u041f~u043e~u043b~u0442~u043e~u0440~u0430 ~u0417~u0435~u043c~u043b~u0435~u043a~u043e~u043f~u0430" }"#,
        ),
        y("y_object_with_newlines", "{\n\"a\": \"b\"\n}"),
        y(
            "y_string_1_2_3_bytes_UTF-8_sequences",
            r#"["~u0060~u012a~u12AB"]"#,
        ),
        y("y_string_accepted_surrogate_pair", r#"["~uD801~udc37"]"#),
        y(
            "y_string_accepted_surrogate_pairs",
            r#"["~ud83d~ude39~ud83d~udc8d"]"#,
        ),
        y("y_string_allowed_escapes", r#"["\"\\\/\b\f\n\r\t"]"#),
        y("y_string_backslash_and_u_escaped_zero", r#"["\\u0000"]"#),
        y("y_string_backslash_doublequotes", r#"["\""]"#),
        y("y_string_comments", r#"["a/*b*/c/*d//e"]"#),
        y("y_string_double_escape_a", r#"["\\a"]"#),
        y("y_string_double_escape_n", r#"["\\n"]"#),
        y("y_string_escaped_control_character", r#"["~u0012"]"#),
        y("y_string_escaped_noncharacter", r#"["~uFFFF"]"#),
        y("y_string_in_array", r#"["asd"]"#),
        y("y_string_in_array_with_leading_space", r#"[ "asd"]"#),
        y("y_string_last_surrogates_1_and_2", r#"["~uDBFF~uDFFF"]"#),
        y("y_string_nbsp_uescaped", r#"["new~u00A0line"]"#),
        y(
            "y_string_nonCharacterInUTF-8_U+10FFFF",
            b"[\"\xf4\x8f\xbf\xbf\"]",
        ),
        y("y_string_nonCharacterInUTF-8_U+FFFF", b"[\"\xef\xbf\xbf\"]"),
        y("y_string_null_escape", r#"["~u0000"]"#),
        y("y_string_one-byte-utf-8", r#"["~u002c"]"#),
        y("y_string_pi", b"[\"\xcf\x80\"]"),
        y(
            "y_string_reservedCharacterInUTF-8_U+1BFFF",
            b"[\"\xf0\x9b\xbf\xbf\"]",
        ),
        y("y_string_simple_ascii", r#"["asd "]"#),
        y("y_string_space", r#"" ""#),
        y(
            "y_string_surrogates_U+1D11E_MUSICAL_SYMBOL_G_CLEF",
            r#"["~uD834~uDd1e"]"#,
        ),
        y("y_string_three-byte-utf-8", r#"["~u0821"]"#),
        y("y_string_two-byte-utf-8", r#"["~u0123"]"#),
        y("y_string_u+2028_line_sep", b"[\"\xe2\x80\xa8\"]"),
        y("y_string_u+2029_par_sep", b"[\"\xe2\x80\xa9\"]"),
        y("y_string_uEscape", r#"["~u0061~u30af~u30EA~u30b9"]"#),
        y("y_string_uescaped_newline", r#"["new~u000Aline"]"#),
        y("y_string_unescaped_char_delete", b"[\"\x7f\"]"),
        y("y_string_unicode", r#"["~uA66D"]"#),
        y("y_string_unicodeEscapedBackslash", r#"["~u005C"]"#),
        y(
            "y_string_unicode_2",
            b"[\"\xe2\x8d\x82\xe3\x88\xb4\xe2\x8d\x82\"]",
        ),
        y("y_string_unicode_U+FDD0_nonchar", r#"["~uFDD0"]"#),
        y("y_string_unicode_U+FFFE_nonchar", r#"["~uFFFE"]"#),
        y("y_string_unicode_escaped_double_quote", r#"["~u0022"]"#),
        y("y_string_utf8", b"[\"\xe2\x82\xac\xf0\x9d\x84\x9e\"]"),
        y("y_string_with_del_character", b"[\"a\x7fa\"]"),
        y("y_structure_lonely_false", "false"),
        y("y_structure_lonely_int", "42"),
        y("y_structure_lonely_negative_real", "-0.1"),
        y("y_structure_lonely_null", "null"),
        y("y_structure_lonely_string", r#""asd""#),
        y("y_structure_lonely_true", "true"),
        y("y_structure_string_empty", r#""""#),
        y("y_structure_trailing_newline", "[\"a\"]\n"),
        y("y_structure_true_in_array", "[true]"),
        y("y_structure_whitespace_array", " [] "),
    ]
}

/// The `n_` cases: what every JSON parser must reject.
fn rejected() -> Vec<Case> {
    vec![
        n("n_array_1_true_without_comma", "[1 true]", CHAR),
        n("n_array_a_invalid_utf8", b"[a\xe5]", UTF8),
        n("n_array_colon_instead_of_comma", r#"["": 1]"#, CHAR),
        n("n_array_comma_after_close", r#"[""],"#, CHAR),
        n("n_array_comma_and_number", "[,1]", CHAR),
        n("n_array_double_comma", "[1,,2]", CHAR),
        n("n_array_double_extra_comma", r#"["x",,]"#, CHAR),
        n("n_array_extra_close", r#"["x"]]"#, CHAR).at(5),
        c("n_array_extra_comma", r#"["",]"#, COMMA).at(3),
        n("n_array_incomplete", r#"["x""#, END).at(4),
        n("n_array_incomplete_invalid_value", "[x", CHAR),
        n("n_array_inner_array_no_comma", "[3[4]]", CHAR),
        n("n_array_invalid_utf8", b"[\xff]", UTF8),
        n("n_array_items_separated_by_semicolon", "[1:2]", CHAR),
        n("n_array_just_comma", "[,]", CHAR),
        n("n_array_just_minus", "[-]", CHAR),
        n("n_array_missing_value", r#"[   , ""]"#, CHAR),
        n("n_array_newlines_unclosed", "[\"a\",\n4\n,1,", END),
        c("n_array_number_and_comma", "[1,]", COMMA),
        n("n_array_number_and_several_commas", "[1,,]", CHAR),
        n(
            "n_array_spaces_vertical_tab_formfeed",
            b"[\"\x0ba\"\x0c]",
            CONTROL,
        ),
        n("n_array_star_inside", "[*]", CHAR),
        n("n_array_unclosed", r#"["""#, END),
        n("n_array_unclosed_trailing_comma", "[1,", END),
        n("n_array_unclosed_with_new_lines", "[1,\n1\n,1", END),
        n("n_array_unclosed_with_object_inside", "[{}", END),
        n("n_incomplete_false", "[fals]", LITERAL),
        n("n_incomplete_null", "[nul]", LITERAL),
        n("n_incomplete_true", "[tru]", LITERAL),
        n("n_multidigit_number_then_00", b"123\x00", CHAR),
        n("n_number_++", "[++1234]", CHAR),
        n("n_number_+1", "[+1]", CHAR),
        n("n_number_+Inf", "[+Inf]", CHAR),
        n("n_number_-01", "[-01]", ZERO).at(1),
        n("n_number_-1.0.", "[-1.0.]", CHAR),
        n("n_number_-2.", "[-2.]", CHAR),
        n("n_number_-NaN", "[-NaN]", CHAR),
        n("n_number_.-1", "[.-1]", CHAR),
        n("n_number_.2e-3", "[.2e-3]", CHAR),
        n("n_number_0.1.2", "[0.1.2]", CHAR),
        n("n_number_0.3e+", "[0.3e+]", CHAR),
        n("n_number_0.3e", "[0.3e]", CHAR),
        n("n_number_0.e1", "[0.e1]", CHAR).at(3),
        n("n_number_0_capital_E+", "[0E+]", CHAR),
        n("n_number_0_capital_E", "[0E]", CHAR),
        n("n_number_0e+", "[0e+]", CHAR),
        n("n_number_0e", "[0e]", CHAR),
        n("n_number_1.0e+", "[1.0e+]", CHAR),
        n("n_number_1.0e-", "[1.0e-]", CHAR),
        n("n_number_1.0e", "[1.0e]", CHAR),
        n("n_number_1_000", "[1 000.0]", CHAR),
        n("n_number_1eE2", "[1eE2]", CHAR),
        n("n_number_2.e+3", "[2.e+3]", CHAR),
        n("n_number_2.e-3", "[2.e-3]", CHAR),
        n("n_number_2.e3", "[2.e3]", CHAR),
        n("n_number_9.e+", "[9.e+]", CHAR),
        n("n_number_Inf", "[Inf]", CHAR),
        n("n_number_NaN", "[NaN]", CHAR),
        n(
            "n_number_U+FF11_fullwidth_digit_one",
            b"[\xef\xbc\x91]",
            CHAR,
        ),
        n("n_number_expression", "[1+2]", CHAR),
        n("n_number_hex_1_digit", "[0x1]", CHAR),
        n("n_number_hex_2_digits", "[0x42]", CHAR),
        n("n_number_infinity", "[Infinity]", CHAR),
        n("n_number_invalid+-", "[0e+-1]", CHAR),
        n("n_number_invalid-negative-real", "[-123.123foo]", CHAR),
        n("n_number_invalid-utf-8-in-bigger-int", b"[123\xe5]", UTF8),
        n("n_number_invalid-utf-8-in-exponent", b"[1e1\xe5]", UTF8),
        n("n_number_invalid-utf-8-in-int", b"[0\xe5]", UTF8),
        n("n_number_minus_infinity", "[-Infinity]", CHAR),
        n("n_number_minus_sign_with_trailing_garbage", "[-foo]", CHAR),
        n("n_number_minus_space_1", "[- 1]", CHAR),
        n("n_number_neg_int_starting_with_zero", "[-012]", ZERO),
        n("n_number_neg_real_without_int_part", "[-.123]", CHAR),
        n("n_number_neg_with_garbage_at_end", "[-1x]", CHAR),
        n("n_number_real_garbage_after_e", "[1ea]", CHAR),
        n("n_number_real_with_invalid_utf8_after_e", b"[1e\xe5]", UTF8).at(3),
        n("n_number_real_without_fractional_part", "[1.]", CHAR),
        n("n_number_starting_with_dot", "[.123]", CHAR),
        n("n_number_with_alpha", "[1.2a-3]", CHAR),
        n(
            "n_number_with_alpha_char",
            "[1.8011670033376514H-308]",
            CHAR,
        ),
        n("n_number_with_leading_zero", "[012]", ZERO),
        n("n_object_bad_value", r#"["x", truth]"#, LITERAL),
        n("n_object_bracket_key", r#"{[: "x"}"#, CHAR),
        n("n_object_comma_instead_of_colon", r#"{"x", null}"#, CHAR),
        n("n_object_double_colon", r#"{"x"::"b"}"#, CHAR),
        n(
            "n_object_emoji",
            b"{\xf0\x9f\x87\xa8\xf0\x9f\x87\xad}",
            CHAR,
        ),
        n("n_object_garbage_at_end", r#"{"a":"a" 123}"#, CHAR),
        n("n_object_key_with_single_quotes", "{key: 'value'}", CHAR),
        n(
            "n_object_lone_continuation_byte_in_key_and_trailing_comma",
            b"{\"\xb9\":\"0\",}",
            UTF8,
        ),
        n("n_object_missing_colon", r#"{"a" b}"#, CHAR),
        n("n_object_missing_key", r#"{:"b"}"#, CHAR),
        n("n_object_missing_semicolon", r#"{"a" "b"}"#, CHAR),
        n("n_object_missing_value", r#"{"a":"#, END),
        n("n_object_no-colon", r#"{"a""#, END),
        n("n_object_non_string_key", "{1:1}", CHAR),
        n(
            "n_object_non_string_key_but_huge_number_instead",
            "{9999E9999:1}",
            CHAR,
        ),
        n("n_object_repeated_null_null", "{null:null,null:null}", CHAR),
        n("n_object_several_trailing_commas", r#"{"id":0,,,,,}"#, CHAR),
        n("n_object_single_quote", "{'a':0}", CHAR),
        c("n_object_trailing_comma", r#"{"id":0,}"#, COMMA),
        c("n_object_trailing_comment", r#"{"a":"b"}/**/"#, COMMENT).at(9),
        n2(
            "n_object_trailing_comment_open",
            r#"{"a":"b"}/**//"#,
            COMMENT,
            CHAR,
        ),
        c(
            "n_object_trailing_comment_slash_open",
            r#"{"a":"b"}//"#,
            COMMENT,
        ),
        n(
            "n_object_trailing_comment_slash_open_incomplete",
            r#"{"a":"b"}/"#,
            CHAR,
        ),
        n(
            "n_object_two_commas_in_a_row",
            r#"{"a":"b",,"c":"d"}"#,
            CHAR,
        ),
        n("n_object_unquoted_key", r#"{a: "b"}"#, CHAR),
        n("n_object_unterminated-value", r#"{"a":"a"#, END),
        n(
            "n_object_with_single_string",
            r#"{ "foo" : "bar", "a" }"#,
            CHAR,
        ),
        n("n_object_with_trailing_garbage", r#"{"a":"b"}#"#, CHAR),
        n("n_single_space", " ", END),
        n(
            "n_string_1_surrogate_then_escape",
            r#"["~uD800\"]"#,
            SURROGATE,
        )
        .at(2),
        n(
            "n_string_1_surrogate_then_escape_u",
            r#"["~uD800~u"]"#,
            ESCAPE,
        ),
        n(
            "n_string_1_surrogate_then_escape_u1",
            r#"["~uD800~u1"]"#,
            ESCAPE,
        ),
        n(
            "n_string_1_surrogate_then_escape_u1x",
            r#"["~uD800~u1x"]"#,
            ESCAPE,
        ),
        n("n_string_accentuated_char_no_quotes", b"[\xc3\xa9]", CHAR),
        n("n_string_backslash_00", b"[\"\\\x00\"]", ESCAPE),
        n("n_string_escape_x", r#"["\x00"]"#, ESCAPE).at(2),
        // JSON5 escapes, which RFC 8259 does not have.
        n("n_string_escape_v", r#"["\v"]"#, ESCAPE).at(2),
        n("n_string_escape_single_quote", r#"["\'"]"#, ESCAPE).at(2),
        n("n_string_escape_zero", r#"["\0"]"#, ESCAPE).at(2),
        n("n_string_escape_line_continuation", "[\"a\\\nb\"]", ESCAPE).at(3),
        n("n_string_escaped_backslash_bad", r#"["\\\"]"#, END),
        n("n_string_escaped_ctrl_char_tab", b"[\"\\\t\"]", ESCAPE),
        n(
            "n_string_escaped_emoji",
            b"[\"\\\xf0\x9f\x8c\x80\"]",
            ESCAPE,
        ),
        n("n_string_incomplete_escape", r#"["\"]"#, END),
        n(
            "n_string_incomplete_escaped_character",
            r#"["~u00A"]"#,
            ESCAPE,
        ),
        n("n_string_incomplete_surrogate", r#"["~uD834~uDd"]"#, ESCAPE),
        n(
            "n_string_incomplete_surrogate_escape_invalid",
            r#"["~uD800~uD800\x"]"#,
            SURROGATE,
        ),
        n("n_string_invalid-utf-8-in-escape", b"[\"~u\xe5\"]", UTF8),
        n("n_string_invalid_backslash_esc", r#"["\a"]"#, ESCAPE),
        n("n_string_invalid_unicode_escape", r#"["~uqqqq"]"#, ESCAPE),
        n("n_string_invalid_utf8_after_escape", b"[\"\\\xe5\"]", UTF8),
        n(
            "n_string_leading_uescaped_thinspace",
            r#"[~u0020"asd"]"#,
            CHAR,
        ),
        n("n_string_no_quotes_with_bad_escape", r"[\n]", CHAR),
        n("n_string_single_doublequote", "\"", END),
        n("n_string_single_quote", "['single quote']", CHAR),
        n("n_string_single_string_no_double_quotes", "abc", CHAR),
        n("n_string_start_escape_unclosed", r#"["\"#, END),
        n("n_string_unescaped_ctrl_char", b"[\"a\x00a\"]", CONTROL),
        n("n_string_unescaped_newline", "[\"new\nline\"]", CONTROL).at(5),
        n("n_string_unescaped_tab", "[\"\t\"]", CONTROL),
        n("n_string_unicode_CapitalU", r#""\UA66D""#, ESCAPE),
        n("n_string_with_trailing_garbage", r#"""x"#, CHAR),
        n(
            "n_structure_100000_opening_arrays",
            "[".repeat(100_000),
            DEEP,
        )
        .at(128),
        n("n_structure_U+2060_word_joined", b"[\xe2\x81\xa0]", CHAR),
        n("n_structure_UTF8_BOM_no_data", b"\xef\xbb\xbf", END).at(3),
        n("n_structure_angle_bracket_.", "<.>", CHAR),
        n("n_structure_angle_bracket_null", "[<null>]", CHAR),
        n("n_structure_array_trailing_garbage", "[1]x", CHAR),
        n("n_structure_array_with_extra_array_close", "[1]]", CHAR),
        // Reported where the string starts.
        n("n_structure_array_with_unclosed_string", r#"["asd]"#, END).at(1),
        n("n_structure_ascii-unicode-identifier", b"a\xc3\xa5", CHAR),
        n("n_structure_capitalized_True", "[True]", CHAR),
        n("n_structure_close_unopened_array", "1]", CHAR),
        n(
            "n_structure_comma_instead_of_closing_brace",
            r#"{"x": true,"#,
            END,
        ),
        n("n_structure_double_array", "[][]", CHAR),
        n("n_structure_end_array", "]", CHAR),
        n("n_structure_incomplete_UTF8_BOM", b"\xef\xbb{}", UTF8),
        n("n_structure_lone-invalid-utf-8", b"\xe5", UTF8),
        n("n_structure_lone-open-bracket", "[", END),
        n("n_structure_no_data", "", END),
        n("n_structure_null-byte-outside-string", b"[\x00]", CHAR),
        n("n_structure_number_with_trailing_garbage", "2@", CHAR),
        n("n_structure_object_followed_by_closing_object", "{}}", CHAR),
        n("n_structure_object_unclosed_no_value", r#"{"":"#, END),
        c(
            "n_structure_object_with_comment",
            r#"{"a":/*comment*/"b"}"#,
            COMMENT,
        ),
        n(
            "n_structure_object_with_trailing_garbage",
            r#"{"a": true} "x""#,
            CHAR,
        ),
        n("n_structure_open_array_apostrophe", "['", CHAR),
        n("n_structure_open_array_comma", "[,", CHAR),
        n(
            "n_structure_open_array_object",
            "[{\"\":".repeat(50_000),
            DEEP,
        ),
        n("n_structure_open_array_open_object", "[{", END),
        n("n_structure_open_array_open_string", r#"["a"#, END),
        n("n_structure_open_array_string", r#"["a""#, END),
        n("n_structure_open_object", "{", END),
        n("n_structure_open_object_close_array", "{]", CHAR),
        n("n_structure_open_object_comma", "{,", CHAR),
        n("n_structure_open_object_open_array", "{[", CHAR),
        n("n_structure_open_object_open_string", r#"{"a"#, END),
        n(
            "n_structure_open_object_string_with_apostrophes",
            "{'a'",
            CHAR,
        ),
        n("n_structure_open_open", r#"["\{["\{["\{["\{"#, ESCAPE),
        n("n_structure_single_eacute", b"\xe9", UTF8),
        n("n_structure_single_star", "*", CHAR),
        n("n_structure_trailing_#", r##"{"a":"b"}#{}"##, CHAR),
        n(
            "n_structure_uescaped_LF_before_string",
            r#"[~u000A""]"#,
            CHAR,
        ),
        n("n_structure_unclosed_array", "[1", END),
        n(
            "n_structure_unclosed_array_partial_null",
            "[ false, nul",
            LITERAL,
        ),
        n(
            "n_structure_unclosed_array_unfinished_false",
            "[ true, fals",
            LITERAL,
        ),
        n(
            "n_structure_unclosed_array_unfinished_true",
            "[ false, tru",
            LITERAL,
        ),
        n("n_structure_unclosed_object", r#"{"asd":"asd""#, END),
        n("n_structure_unicode-identifier", b"\xc3\xa5", CHAR),
        n("n_structure_whitespace_formfeed", b"[\x0c]", CHAR),
    ]
}

/// The `i_` cases Tidepool accepts.
fn implementation_defined_accepted() -> Vec<Case> {
    vec![
        // Underflow rounds to zero.
        y("i_number_double_huge_neg_exp", "[123.456e-789]"),
        y("i_number_real_underflow", "[123e-10000000]"),
        // Integers too large for u64 or i64 become the nearest f64.
        y(
            "i_number_too_big_neg_int",
            "[-123123123123123123123123123123]",
        ),
        y("i_number_too_big_pos_int", "[100000000000000000000]"),
        y(
            "i_number_very_big_negative_int",
            "[-237462374673276894279832749832423479823246327846]",
        ),
        // A leading byte-order mark is skipped (RFC 8259 §8.1).
        y("i_structure_UTF-8_BOM_empty_object", b"\xef\xbb\xbf{}"),
    ]
}

/// The `i_` cases Tidepool rejects.
fn implementation_defined_rejected() -> Vec<Case> {
    vec![
        // Magnitudes beyond f64 are errors, not infinities.
        n(
            "i_number_huge_exp",
            "[0.4e00669999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999969999999006]",
            RANGE,
        ),
        n("i_number_neg_int_huge_exp", "[-1e+9999]", RANGE),
        n("i_number_pos_double_huge_exp", "[1.5e+9999]", RANGE),
        n("i_number_real_neg_overflow", "[-123123e100000]", RANGE),
        n("i_number_real_pos_overflow", "[123123e100000]", RANGE),
        // Lone surrogates are rejected.
        n(
            "i_object_key_lone_2nd_surrogate",
            r#"{"~uDFAA":0}"#,
            SURROGATE,
        ),
        n(
            "i_string_1st_surrogate_but_2nd_missing",
            r#"["~uDADA"]"#,
            SURROGATE,
        ),
        n(
            "i_string_1st_valid_surrogate_2nd_invalid",
            r#"["~uD888~u1234"]"#,
            SURROGATE,
        ),
        n(
            "i_string_incomplete_surrogate_and_escape_valid",
            r#"["~uD800\n"]"#,
            SURROGATE,
        ),
        n(
            "i_string_incomplete_surrogate_pair",
            r#"["~uDd1ea"]"#,
            SURROGATE,
        ),
        n(
            "i_string_incomplete_surrogates_escape_valid",
            r#"["~uD800~uD800\n"]"#,
            SURROGATE,
        ),
        n(
            "i_string_invalid_lonely_surrogate",
            r#"["~ud800"]"#,
            SURROGATE,
        ),
        n("i_string_invalid_surrogate", r#"["~ud800abc"]"#, SURROGATE),
        n(
            "i_string_inverted_surrogates_U+1D11E",
            r#"["~uDd1e~uD834"]"#,
            SURROGATE,
        ),
        n("i_string_lone_second_surrogate", r#"["~uDFAA"]"#, SURROGATE),
        // Input that is not UTF-8 is rejected, wherever it is.
        n(
            "i_string_UTF-16LE_with_BOM",
            b"\xff\xfe[\x00\"\x00\xe9\x00\"\x00]\x00",
            UTF8,
        ),
        n(
            "i_string_UTF-8_invalid_sequence",
            b"[\"\xe6\x97\xa5\xd1\x88\xfa\"]",
            UTF8,
        ),
        n(
            "i_string_UTF8_surrogate_U+D800",
            b"[\"\xed\xa0\x80\"]",
            UTF8,
        ),
        n("i_string_invalid_utf-8", b"[\"\xff\"]", UTF8),
        n("i_string_iso_latin_1", b"[\"\xe9\"]", UTF8),
        n("i_string_lone_utf8_continuation_byte", b"[\"\x81\"]", UTF8),
        n(
            "i_string_not_in_unicode_range",
            b"[\"\xf4\xbf\xbf\xbf\"]",
            UTF8,
        ),
        n(
            "i_string_overlong_sequence_2_bytes",
            b"[\"\xc0\xaf\"]",
            UTF8,
        ),
        n(
            "i_string_overlong_sequence_6_bytes",
            b"[\"\xfc\x83\xbf\xbf\xbf\xbf\"]",
            UTF8,
        ),
        n(
            "i_string_overlong_sequence_6_bytes_null",
            b"[\"\xfc\x80\x80\x80\x80\x80\"]",
            UTF8,
        ),
        n("i_string_truncated-utf-8", b"[\"\xe0\xff\"]", UTF8),
        n(
            "i_string_utf16BE_no_BOM",
            b"\x00[\x00\"\x00\xe9\x00\"\x00]",
            UTF8,
        ),
        n(
            "i_string_utf16LE_no_BOM",
            b"[\x00\"\x00\xe9\x00\"\x00]\x00",
            UTF8,
        ),
        // Deeper than the default limit of 128.
        n(
            "i_structure_500_nested_arrays",
            format!("{}{}", "[".repeat(500), "]".repeat(500)),
            DEEP,
        ),
    ]
}

/// JSONC: exactly comments and trailing commas, nothing more.
fn jsonc_cases() -> Vec<Case> {
    vec![
        c("jsonc_line_comment", "// settings\n[1]", COMMENT),
        c("jsonc_line_comment_at_end", "[1] // done", COMMENT),
        c("jsonc_block_comment", "/* a */ [1 /* b */]", COMMENT),
        c(
            "jsonc_comment_between_key_and_colon",
            r#"{"a" /* c */ : 1}"#,
            COMMENT,
        ),
        c("jsonc_trailing_comma_array", "[1, 2,]", COMMA),
        c("jsonc_trailing_comma_object", r#"{"a": 1,}"#, COMMA),
        c("jsonc_trailing_comma_then_comment", "[1, // c\n]", COMMENT),
        n("jsonc_lone_comma_array", "[,]", CHAR),
        n("jsonc_lone_comma_object", "{,}", CHAR),
        n("jsonc_two_trailing_commas", "[1,,]", CHAR),
        n("jsonc_trailing_comma_top_level", "1,", CHAR),
        n2("jsonc_unterminated_block", "[1] /* open", COMMENT, END).at(4),
        n2("jsonc_nested_block", "/* /* */ */ 1", COMMENT, CHAR),
        n("jsonc_hash_comment", "# c\n1", CHAR),
        n("jsonc_single_quotes", "['a']", CHAR),
        n("jsonc_unquoted_key", "{a: 1}", CHAR),
        n("jsonc_hex_number", "0x10", CHAR),
        n("jsonc_leading_plus", "+1", CHAR),
        n("jsonc_infinity", "Infinity", CHAR),
        n("jsonc_lone_slash", "[1 / 2]", CHAR),
    ]
}

/// Runs `case` in `dialect` and checks the outcome. Returns a description
/// of the problem, if there is one.
fn check_case(case: &Case, dialect: Dialect, expected: Outcome) -> Option<String> {
    let result = json::parse_bytes(&case.input, dialect, Limits::DEFAULT);
    let problem = match (&result, expected) {
        (Ok(value), Accept) => round_trip_problem(value, dialect),
        (Err(CodecError::Json { offset, kind, .. }), Reject(name)) => {
            let debug = format!("{kind:?}");
            if !debug.starts_with(name) {
                Some(format!("rejected with {debug}, not {name}"))
            } else if *offset > case.input.len() {
                Some(format!("offset {offset} is past the end"))
            } else if case.offset.is_some_and(|expected| expected != *offset) {
                Some(format!("rejected at byte {offset}, not {:?}", case.offset))
            } else {
                None
            }
        }
        (Ok(value), Reject(name)) => Some(format!(
            "accepted as {value:?}, but should fail with {name}"
        )),
        (Err(error), Accept) => Some(format!("rejected, but should be accepted: {error}")),
        (Err(error), Reject(_)) => Some(format!("not a JSON error: {error}")),
    };
    problem.map(|problem| format!("{} ({dialect:?}): {problem}", case.name))
}

/// An accepted value must be written and read back unchanged, bit for bit:
/// `-0.0` must not come back as `0.0`.
fn round_trip_problem(value: &Value, dialect: Dialect) -> Option<String> {
    for written in [
        json::to_string_compact(value),
        json::to_string_pretty(value),
    ] {
        match json::parse(&written, dialect, Limits::DEFAULT) {
            Ok(back) if identical(&back, value) => {}
            other => return Some(format!("wrote {written:?}, which reads back as {other:?}")),
        }
    }
    None
}

fn run(cases: &[Case]) {
    let problems: Vec<String> = cases
        .iter()
        .flat_map(|case| {
            [
                check_case(case, Dialect::Json, case.json),
                check_case(case, Dialect::Jsonc, case.jsonc),
            ]
        })
        .flatten()
        .collect();
    assert!(
        problems.is_empty(),
        "{} of {} cases failed:\n{}",
        problems.len(),
        cases.len(),
        problems.join("\n")
    );
}

#[test]
fn rfc_8259_y_cases_are_accepted() {
    run(&accepted());
}

#[test]
fn rfc_8259_n_cases_are_rejected() {
    run(&rejected());
}

#[test]
fn implementation_defined_cases_follow_the_documented_choices() {
    run(&implementation_defined_accepted());
    run(&implementation_defined_rejected());
}

#[test]
fn jsonc_adds_comments_and_trailing_commas_only() {
    run(&jsonc_cases());
}

#[test]
fn case_names_are_unique_and_follow_the_suite() {
    let groups = [
        ("y_", accepted()),
        ("n_", rejected()),
        ("i_", implementation_defined_accepted()),
        ("i_", implementation_defined_rejected()),
        ("jsonc_", jsonc_cases()),
    ];
    let mut names: Vec<&str> = Vec::new();
    for (prefix, cases) in &groups {
        for case in cases {
            assert!(
                case.name.starts_with(prefix),
                "{} is not {prefix}",
                case.name
            );
            names.push(case.name);
        }
    }
    let count = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), count, "a case name is used twice");
}

/// A few accepted cases, checked for the value they decode to, not only for
/// being accepted.
#[test]
fn accepted_cases_decode_to_the_right_values() {
    let value_of = |name: &str| {
        let case = accepted()
            .into_iter()
            .chain(implementation_defined_accepted())
            .find(|case| case.name == name)
            .unwrap_or_else(|| panic!("no case {name}"));
        json::parse_bytes(&case.input, Dialect::Json, Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{name}: {error}"))
    };
    let first = |name: &str| {
        value_of(name)
            .as_array()
            .and_then(|items| items.first().cloned())
            .unwrap_or_else(|| panic!("{name} is not a non-empty array"))
    };
    let text = |name: &str| first(name).as_str().map(str::to_string);

    assert_eq!(
        text("y_string_accepted_surrogate_pair"),
        Some("\u{10437}".to_string())
    );
    assert_eq!(
        text("y_string_accepted_surrogate_pairs"),
        Some("\u{1f639}\u{1f48d}".to_string())
    );
    assert_eq!(
        text("y_string_allowed_escapes"),
        Some("\"\\/\u{8}\u{c}\n\r\t".to_string())
    );
    assert_eq!(
        text("y_string_backslash_and_u_escaped_zero"),
        Some("\\u0000".to_string())
    );
    assert_eq!(text("y_string_null_escape"), Some("\0".to_string()));
    assert_eq!(
        text("y_string_unicodeEscapedBackslash"),
        Some("\\".to_string())
    );
    assert_eq!(
        text("y_string_uescaped_newline"),
        Some("new\nline".to_string())
    );
    assert_eq!(
        text("y_string_last_surrogates_1_and_2"),
        Some("\u{10ffff}".to_string())
    );
    assert_eq!(
        text("y_string_escaped_noncharacter"),
        Some("\u{ffff}".to_string())
    );
    assert_eq!(text("y_string_comments"), Some("a/*b*/c/*d//e".to_string()));
    // Both members are kept; `get` returns the last.
    let duplicated = value_of("y_object_duplicated_key");
    assert_eq!(duplicated.as_object().map(<[_]>::len), Some(2));
    assert_eq!(duplicated.get("a"), Some(&Value::from("c")));
    assert_eq!(
        value_of("y_object_escaped_null_in_key")
            .as_object()
            .and_then(|members| members.first())
            .map(|(key, _)| key.clone()),
        Some("foo\0bar".to_string())
    );

    assert_eq!(first("y_number_simple_int").as_u64(), Some(123));
    assert_eq!(first("y_number_negative_int").as_i64(), Some(-123));
    assert_eq!(first("y_number_real_capital_e").as_f64(), Some(1e22));
    assert_eq!(
        first("y_number_real_fraction_exponent").as_f64(),
        Some(123.456e78)
    );
    let minus_zero = first("y_number_minus_zero").as_f64().expect("a number");
    assert!(minus_zero == 0.0 && minus_zero.is_sign_negative());
    assert_eq!(first("i_number_real_underflow").as_f64(), Some(0.0));
    assert_eq!(first("i_number_too_big_pos_int").as_f64(), Some(1e20));
    assert_eq!(first("i_number_too_big_pos_int").as_u64(), None);
}
