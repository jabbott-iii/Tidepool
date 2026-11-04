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

//! TOML's date-times: [`Datetime`] and its parts, [`Date`], [`Time`] and
//! [`Offset`].

use std::fmt;

/// A TOML date-time, of one of TOML's four kinds.
///
/// The parser validates every field: the month has the day (leap years
/// follow the Gregorian rules), and the hour, minute, second and offset are
/// in range (see [`Time`] and [`Offset`]). A value built in code is not
/// checked.
///
/// `Display` writes the RFC 3339 form TOML uses, with `T` between the date
/// and the time, the seconds always written, and the fraction of a second
/// only when it is not zero, without trailing zeros:
///
/// ```
/// use tidepool_codec::toml::{Date, Datetime, Offset, Time};
///
/// let date = Date { year: 1979, month: 5, day: 27 };
/// let time = Time { hour: 7, minute: 32, second: 0, nanosecond: 500_000_000 };
/// let offset = Offset::Minutes(-7 * 60);
/// assert_eq!(
///     Datetime::OffsetDatetime { date, time, offset }.to_string(),
///     "1979-05-27T07:32:00.5-07:00"
/// );
/// assert_eq!(Datetime::LocalDate(date).to_string(), "1979-05-27");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Datetime {
    /// An instant: a date and time with an offset from UTC, such as
    /// `1979-05-27T07:32:00Z` or `1979-05-27 00:32:00-07:00`.
    OffsetDatetime {
        /// The date.
        date: Date,
        /// The time of day, at `offset`.
        time: Time,
        /// The offset from UTC.
        offset: Offset,
    },
    /// A date and time with no offset, such as `1979-05-27T07:32:00`. It
    /// names no instant until a time zone is chosen.
    LocalDatetime {
        /// The date.
        date: Date,
        /// The time of day.
        time: Time,
    },
    /// A whole day with no offset, such as `1979-05-27`.
    LocalDate(Date),
    /// A time of day with no date or offset, such as `07:32:00`.
    LocalTime(Time),
}

/// A calendar date in the proleptic Gregorian calendar, as TOML writes it:
/// `YYYY-MM-DD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Date {
    /// The year, from 0 to 9999.
    pub year: u16,
    /// The month, from 1 to 12.
    pub month: u8,
    /// The day of the month, from 1 to the number of days that month has.
    pub day: u8,
}

/// A time of day: `HH:MM:SS` with an optional fraction of a second, or
/// `HH:MM`, whose seconds are zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Time {
    /// The hour, from 0 to 23.
    pub hour: u8,
    /// The minute, from 0 to 59.
    pub minute: u8,
    /// The second, from 0 to 60. RFC 3339 allows 60 for a leap second; the
    /// parser accepts it in any minute, since it has no table of leap
    /// seconds and a local time has no date to look one up by.
    pub second: u8,
    /// The fraction of a second, in nanoseconds, from 0 to 999,999,999.
    /// Digits written beyond the ninth are dropped, not rounded, as TOML
    /// requires.
    pub nanosecond: u32,
}

/// The offset of an [`Datetime::OffsetDatetime`] from UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Offset {
    /// `Z` (or `z`): UTC.
    Utc,
    /// `+HH:MM` or `-HH:MM`: this many minutes east of UTC, from -1439 to
    /// 1439 (hours from 00 to 23, minutes from 00 to 59). `-07:00` is
    /// `Minutes(-420)`. `+00:00` and `-00:00` are both `Minutes(0)`, which
    /// is the same instant as [`Offset::Utc`] but kept apart from it.
    Minutes(i16),
}

/// The number of days in `month` (1 to 12) of `year`, or 0 for a month
/// that does not exist.
pub(super) const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Whether `year` is a leap year in the Gregorian calendar: divisible by 4,
/// and not by 100 unless also by 400.
const fn is_leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

impl fmt::Display for Datetime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OffsetDatetime { date, time, offset } => write!(f, "{date}T{time}{offset}"),
            Self::LocalDatetime { date, time } => write!(f, "{date}T{time}"),
            Self::LocalDate(date) => write!(f, "{date}"),
            Self::LocalTime(time) => write!(f, "{time}"),
        }
    }
}

impl fmt::Display for Date {
    /// `YYYY-MM-DD`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl fmt::Display for Time {
    /// `HH:MM:SS`, then the fraction of a second if it is not zero, without
    /// trailing zeros: `07:32:00.5`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)?;
        if self.nanosecond != 0 {
            let digits = format!("{:09}", self.nanosecond);
            write!(f, ".{}", digits.trim_end_matches('0'))?;
        }
        Ok(())
    }
}

impl fmt::Display for Offset {
    /// `Z`, or `+HH:MM` or `-HH:MM`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Utc => f.write_str("Z"),
            Self::Minutes(minutes) => {
                let sign = if minutes < 0 { '-' } else { '+' };
                let minutes = minutes.unsigned_abs();
                write!(f, "{sign}{:02}:{:02}", minutes / 60, minutes % 60)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_in_month_follows_the_gregorian_calendar() {
        assert_eq!(days_in_month(2023, 1), 31);
        assert_eq!(days_in_month(2023, 4), 30);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29);
        // Centuries are leap years only every 400 years.
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        // Year 0 is divisible by 400.
        assert_eq!(days_in_month(0, 2), 29);
        assert_eq!(days_in_month(9999, 12), 31);
        assert_eq!(days_in_month(2024, 0), 0);
        assert_eq!(days_in_month(2024, 13), 0);
        let days: Vec<u8> = (1..=12).map(|month| days_in_month(2023, month)).collect();
        assert_eq!(days, [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]);
    }

    #[test]
    fn display_writes_rfc_3339() {
        let date = Date {
            year: 7,
            month: 1,
            day: 2,
        };
        let time = Time {
            hour: 3,
            minute: 4,
            second: 5,
            nanosecond: 0,
        };
        let cases = [
            (
                Datetime::OffsetDatetime {
                    date,
                    time,
                    offset: Offset::Utc,
                },
                "0007-01-02T03:04:05Z",
            ),
            (
                Datetime::OffsetDatetime {
                    date,
                    time,
                    offset: Offset::Minutes(5 * 60 + 30),
                },
                "0007-01-02T03:04:05+05:30",
            ),
            (
                Datetime::OffsetDatetime {
                    date,
                    time,
                    offset: Offset::Minutes(-(23 * 60 + 59)),
                },
                "0007-01-02T03:04:05-23:59",
            ),
            (
                Datetime::OffsetDatetime {
                    date,
                    time,
                    offset: Offset::Minutes(0),
                },
                "0007-01-02T03:04:05+00:00",
            ),
            (
                Datetime::LocalDatetime { date, time },
                "0007-01-02T03:04:05",
            ),
            (Datetime::LocalDate(date), "0007-01-02"),
            (Datetime::LocalTime(time), "03:04:05"),
            (
                Datetime::LocalTime(Time {
                    nanosecond: 1,
                    ..time
                }),
                "03:04:05.000000001",
            ),
            (
                Datetime::LocalTime(Time {
                    nanosecond: 120_000_000,
                    ..time
                }),
                "03:04:05.12",
            ),
            (
                Datetime::LocalTime(Time {
                    nanosecond: 999_999_999,
                    second: 60,
                    ..time
                }),
                "03:04:60.999999999",
            ),
        ];
        for (datetime, expected) in cases {
            assert_eq!(datetime.to_string(), expected);
        }
    }
}
