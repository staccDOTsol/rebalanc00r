//! Parser example for ISO8601 dates. This does not handle the entire specification but it should
//! show the gist of it and be easy to extend to parse additional forms.
extern crate combine;

use std::env;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};

use combine::error::ParseError;
use combine::parser::char::{char, digit};
use combine::stream::state::State;
use combine::{choice, many, optional, Parser, Stream};

#[cfg(feature = "std")]
use combine::stream::easy;
#[cfg(feature = "std")]
use combine::stream::state::SourcePosition;

enum Error<E> {
    Io(io::Error),
    Parse(E),
}

impl<E> fmt::Display for Error<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Io(ref err) => write!(f, "{}", err),
            Error::Parse(ref err) => write!(f, "{}", err),
        }
    }
}

#[derive(PartialEq, Debug)]
pub struct Date {
    pub year: i32,
    pub month: i32,
    pub day: i32,
}

#[derive(PartialEq, Debug)]
pub struct Time {
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
    pub time_zone: i32,
}

#[derive(PartialEq, Debug)]
pub struct DateTime {
    pub date: Date,
    pub time: Time,
}

fn two_digits<I>() -> impl Parser<Input = I, Output = i32>
where
    I: Stream<Item = char>,
    // Necessary due to rust-lang/rust#24159
    I::Error: ParseError<I::Item, I::Range, I::Position>,
{
    (digit(), digit()).map(|(x, y): (char, char)| {
        let x = x.to_digit(10).expect("digit");
        let y = y.to_digit(10).expect("digit");
        (x * 10 + y) as i32
    })
}

/// Parses a time zone
/// +0012
/// -06:30
/// -01
/// Z
fn time_zone<I>() -> impl Parser<Input = I, Output = i32>
where
    I: Stream<Item = char>,
    I::Error: ParseError<I::Item, I::Range, I::Position>,
{
    let utc = char('Z').map(|_| 0);
    let offset = (
        choice([char('-'), char('+')]),
        two_digits(),
        optional(optional(char(':')).with(two_digits())),
    )
        .map(|(sign, hour, minute)| {
            let offset = hour * 60 + minute.unwrap_or(0);
            if sign == '-' {
                -offset
            } else {
                offset
            }
        });

    utc.or(offset)
}

/// Parses a date
/// 2010-01-30
fn date<I>() -> impl Parser<Input = I, Output = Date>
where
    I: Stream<Item = char>,
    I::Error: ParseError<I::Item, I::Range, I::Position>,
{
    (
        many::<String, _>(digit()),
        char('-'),
        two_digits(),
        char('-'),
        two_digits(),
    )
        .map(|(year, _, month, _, day)| {
            // Its ok to just unwrap since we only parsed digits
            Date {
                year: year.parse().unwrap(),
                month: month,
                day: day,
            }
        })
}

/// Parses a time
/// 12:30:02
fn time<I>() -> impl Parser<Input = I, Output = Time>
where
    I: Stream<Item = char>,
    I::Error: ParseError<I::Item, I::Range, I::Position>,
{
    (
        two_digits(),
        char(':'),
        two_digits(),
        char(':'),
        two_digits(),
        time_zone(),
    )
        .map(|(hour, _, minute, _, second, time_zone)| {
            // Its ok to just unwrap since we only parsed digits
            Time {
                hour: hour,
                minute: minute,
                second: second,
                time_zone: time_zone,
            }
        })
}

/// Parses a date time according to ISO8601
/// 2015-08-02T18:54:42+02
fn date_time<I>() -> impl Parser<Input = I, Output = DateTime>
where
    I: Stream<Item = char>,
    I::Error: ParseError<I::Item, I::Range, I::Position>,
{
    (date(), char('T'), time()).map(|(date, _, time)| DateTime {
        date: date,
        time: time,
    })
}

#[test]
fn test() {
    // A parser for
    let result = date_time().parse("2015-08-02T18:54:42+02");
    let d = DateTime {
        date: Date {
            year: 2015,
            month: 8,
            day: 2,
        },
        time: Time {
            hour: 18,
            minute: 54,
            second: 42,
            time_zone: 2 * 60,
        },
    };
    assert_eq!(result, Ok((d, "")));

    let result = date_time().parse("50015-12-30T08:54:42Z");
    let d = DateTime {
        date: Date {
            year: 50015,
            month: 12,
            day: 30,
        },
        time: Time {
            hour: 8,
            minute: 54,
            second: 42,
            time_zone: 0,
        },
    };
    assert_eq!(result, Ok((d, "")));
}

fn main() {
    let result = match env::args().nth(1) {
        Some(file) => File::open(file).map_err(Error::Io).and_then(main_),
        None => main_(io::stdin()),
    };
    match result {
        Ok(_) => println!("OK"),
        Err(err) => println!("{}", err),
    }
}

#[cfg(feature = "std")]
fn main_<R>(mut read: R) -> Result<(), Error<easy::Errors<char, String, SourcePosition>>>
where
    R: Read,
{
    let mut text = String::new();
    read.read_to_string(&mut text).map_err(Error::Io)?;
    date_time()
        .easy_parse(State::new(&*text))
        .map_err(|err| Error::Parse(err.map_range(|s| s.to_string())))?;
    Ok(())
}

#[cfg(not(feature = "std"))]
fn main_<R>(mut read: R) -> Result<(), Error<::combine::error::StringStreamError>>
where
    R: Read,
{
    let mut text = String::new();
    read.read_to_string(&mut text).map_err(Error::Io)?;
    date_time()
        .parse(State::new(&*text))
        .map_err(Error::Parse)?;
    Ok(())
}
