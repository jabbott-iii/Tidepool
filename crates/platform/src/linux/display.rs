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

//! Which X servers the window connects to: a local one, over a Unix socket
//! only (ADR-0039).
//!
//! New in Tidepool. Nadir passes `XOpenDisplay` null, which reads `DISPLAY`
//! itself and may connect over TCP.
//!
//! The connection to the X server is the one socket `AGENTS.md` rule 8
//! lets the core binary open, and only when it is local. Xlib (libxcb
//! underneath) reads a display name as `[protocol/][host]:N[.S]`, and for
//! `:N`, with no host, tries the Unix sockets (the abstract
//! `@/tmp/.X11-unix/XN`, then `/tmp/.X11-unix/XN`) and then falls back to
//! TCP on localhost, port 6000 + N; for `unix:N` it tries the Unix sockets
//! and nothing else; and for any other host it goes straight to TCP. So
//! [`display_name`] reads `DISPLAY` and gives Xlib only a name that cannot
//! reach TCP:
//!
//! - `:N` and `:N.S` become `unix:N` and `unix:N.S`;
//! - `unix:N` and `unix:N.S` stay as they are;
//! - everything else is refused, with the reason: unset, empty, a host
//!   name (`localhost:10.0`, which `ssh -X` sets), a transport
//!   (`tcp/…`, `inet6/…`, `unix/…`), a socket path (`/…`), or a display or
//!   screen number that is not decimal digits, or is over `i32::MAX`.
//!
//! `XOpenDisplay` is never given null, which would make it read `DISPLAY`
//! itself. Xlib still reads the Xauthority file and sends the display's
//! cookie over the socket; Tidepool never reads the file.

use std::ffi::OsStr;

/// Returns the name to give `XOpenDisplay` for the value of `DISPLAY`
/// (`None` if it is not set), or why it is refused, as words that follow
/// `DISPLAY=<value>` in a message.
///
/// It never fails on any value: it reads bytes, which need not be UTF-8.
pub(super) fn display_name(value: Option<&OsStr>) -> Result<String, &'static str> {
    let Some(value) = value else {
        return Err("is not set");
    };
    let bytes = value.as_encoded_bytes();
    if bytes.is_empty() {
        return Err("is empty");
    }
    if bytes.starts_with(b"/") {
        return Err("is a socket path");
    }
    // As libxcb reads it: the host is everything before the last colon,
    // and a protocol before a slash in it.
    let Some(colon) = bytes.iter().rposition(|&byte| byte == b':') else {
        return Err("has no `:` before a display number");
    };
    let (host, number) = (&bytes[..colon], &bytes[colon + 1..]);
    if host.contains(&b'/') {
        return Err("names a transport, such as tcp/");
    }
    if !host.is_empty() && host != b"unix" {
        return Err("names a host");
    }
    let (display, screen) = match number.iter().position(|&byte| byte == b'.') {
        Some(dot) => (&number[..dot], Some(&number[dot + 1..])),
        None => (number, None),
    };
    check_number(display).map_err(|why| match why {
        Number::NotDecimal => "has a display number that is not a decimal number",
        Number::TooLarge => "has a display number that is too large",
    })?;
    if let Some(screen) = screen {
        check_number(screen).map_err(|why| match why {
            Number::NotDecimal => "has a screen number that is not a decimal number",
            Number::TooLarge => "has a screen number that is too large",
        })?;
    }
    // Only ASCII digits, a dot and `unix` are left, so the bytes are text.
    let number = String::from_utf8_lossy(number);
    Ok(format!("unix:{number}"))
}

/// Why a display or screen number is refused.
enum Number {
    /// It is empty, or has a byte that is not an ASCII digit, such as a
    /// sign, a space or a second dot.
    NotDecimal,
    /// It is over `i32::MAX`, which libxcb would not read as it is.
    TooLarge,
}

/// Checks that `digits` is a decimal number from 0 to `i32::MAX`. Leading
/// zeros are allowed, as libxcb allows them.
fn check_number(digits: &[u8]) -> Result<(), Number> {
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(Number::NotDecimal);
    }
    let value = digits.iter().try_fold(0_i32, |value, &digit| {
        value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i32::from(digit - b'0')))
    });
    match value {
        Some(_) => Ok(()),
        None => Err(Number::TooLarge),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStrExt as _;

    use tidepool_core::testing::check;

    use super::*;

    fn name(value: &str) -> Result<String, &'static str> {
        display_name(Some(OsStr::new(value)))
    }

    #[test]
    fn a_local_display_becomes_its_unix_socket() {
        assert_eq!(name(":0").as_deref(), Ok("unix:0"));
        assert_eq!(name(":1").as_deref(), Ok("unix:1"));
        assert_eq!(name(":99.0").as_deref(), Ok("unix:99.0"));
        assert_eq!(name(":10.3").as_deref(), Ok("unix:10.3"));
        assert_eq!(name(":007").as_deref(), Ok("unix:007"));
        assert_eq!(
            name(":2147483647.2147483647").as_deref(),
            Ok("unix:2147483647.2147483647")
        );
    }

    #[test]
    fn a_unix_display_is_kept_as_it_is() {
        assert_eq!(name("unix:0").as_deref(), Ok("unix:0"));
        assert_eq!(name("unix:12.1").as_deref(), Ok("unix:12.1"));
    }

    #[test]
    fn an_unset_or_empty_display_is_refused() {
        assert_eq!(display_name(None), Err("is not set"));
        assert_eq!(name(""), Err("is empty"));
    }

    #[test]
    fn a_display_on_a_host_is_refused() {
        for value in [
            "localhost:10.0",
            "localhost:0",
            "127.0.0.1:0",
            "remote.example:1",
            "::1:0",
            "[::1]:0",
            "UNIX:0",
            "unix2:0",
            " :0",
            "host::0",
        ] {
            assert_eq!(name(value), Err("names a host"), "{value:?}");
        }
    }

    #[test]
    fn a_transport_or_a_socket_path_is_refused() {
        for value in [
            "tcp/localhost:0",
            "inet6/::1:0",
            "unix/:0",
            "local/host:0",
            "tcp/:0",
        ] {
            assert_eq!(
                name(value),
                Err("names a transport, such as tcp/"),
                "{value:?}"
            );
        }
        for value in [
            "/tmp/.X11-unix/X0",
            "/private/tmp/launch-x/org.xquartz:0",
            "/",
        ] {
            assert_eq!(name(value), Err("is a socket path"), "{value:?}");
        }
    }

    #[test]
    fn a_number_that_is_not_decimal_is_refused() {
        for value in [
            ":", ":x", ":0x1", ":+1", ":-1", ": 1", ":1 ", ":1a", ":١", "0", "unix", "unix:",
        ] {
            let refused = name(value);
            assert!(
                matches!(
                    refused,
                    Err("has a display number that is not a decimal number"
                        | "has no `:` before a display number")
                ),
                "{value:?}: {refused:?}"
            );
        }
        for value in [":0.", ":0.x", ":0.1.2", ":0.-1", ":1..2"] {
            assert_eq!(
                name(value),
                Err("has a screen number that is not a decimal number"),
                "{value:?}"
            );
        }
        assert_eq!(name("0"), Err("has no `:` before a display number"));
    }

    #[test]
    fn a_number_over_i32_max_is_refused() {
        assert_eq!(
            name(":2147483648"),
            Err("has a display number that is too large")
        );
        assert_eq!(
            name(":99999999999999999999999"),
            Err("has a display number that is too large")
        );
        assert_eq!(
            name(":0.2147483648"),
            Err("has a screen number that is too large")
        );
    }

    #[test]
    fn bytes_that_are_not_utf8_are_refused_not_misread() {
        let value = OsStr::from_bytes(b":\xff");
        assert_eq!(
            display_name(Some(value)),
            Err("has a display number that is not a decimal number")
        );
        let value = OsStr::from_bytes(b"h\xc3\x28st:0");
        assert_eq!(display_name(Some(value)), Err("names a host"));
    }

    #[test]
    fn whatever_the_bytes_an_accepted_name_is_a_unix_socket() {
        // Bytes biased toward the characters a display name is made of.
        let alphabet = b":.0123456789unixtcp/ \xff-+";
        check(
            "an accepted display name reaches only a Unix socket",
            5000,
            |rng| {
                let len = rng.range(0..12_u32);
                (0..len)
                    .map(|_| alphabet[rng.range(0..alphabet.len() as u32) as usize])
                    .collect::<Vec<u8>>()
            },
            |bytes| match display_name(Some(OsStr::from_bytes(bytes))) {
                Err(_) => Ok(()),
                Ok(name) => {
                    let rest = name.strip_prefix("unix:").ok_or("no unix: prefix")?;
                    let parts: Vec<&str> = rest.split('.').collect();
                    let decimal = |part: &str| {
                        !part.is_empty()
                            && part.bytes().all(|byte| byte.is_ascii_digit())
                            && part.parse::<i32>().is_ok()
                    };
                    if (1..=2).contains(&parts.len()) && parts.iter().all(|part| decimal(part)) {
                        Ok(())
                    } else {
                        Err(format!("accepted {name:?}"))
                    }
                }
            },
        );
    }
}
