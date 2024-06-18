use crate::data::{
    FieldValue, FilterExpression, SerialColour, Utf32CachedString, ValueMatchExpression,
};
use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case, take_while_m_n};
use nom::character::complete::{alpha1, digit0, digit1, none_of, one_of};
use nom::combinator::{map, map_opt, map_res, opt};
use nom::error::ParseError;
use nom::multi::{count, fold_many0, fold_many1, fold_many_m_n, many0, many1};
use nom::sequence::{delimited, pair, preceded, separated_pair, terminated, tuple};
use nom::{IResult, Parser};
use std::cell::RefCell;
use std::path::PathBuf;
use uuid::Uuid;

use crate::data::filter::ValueMatchExpressionDiscriminants;
use chrono::{
    DateTime, FixedOffset, MappedLocalTime, NaiveDate, NaiveDateTime, NaiveTime, ParseResult,
    TimeZone, Utc,
};
use itertools::Itertools;
use nom::number::complete::double;
use regex::Regex;
use std::str::FromStr;
use std::sync::Mutex;

#[cfg(not(test))]
static LOCAL: Mutex<Option<chrono::Local>> = Mutex::new(Some(chrono::Local));

#[cfg(test)]
static LOCAL: Mutex<Option<FixedOffset>> = Mutex::new(FixedOffset::east_opt(8 * 3600));

macro_rules! local {
    () => {
        LOCAL.lock().unwrap().unwrap()
    };
}

const WHITESPACE: &str = " \t\r\n\u{3000}";

pub fn hex_digit(c: char) -> Option<u8> {
    Some(match c {
        '0' => 0,
        '1' => 1,
        '2' => 2,
        '3' => 3,
        '4' => 4,
        '5' => 5,
        '6' => 6,
        '7' => 7,
        '8' => 8,
        '9' => 9,
        'A' | 'a' => 0xa,
        'B' | 'b' => 0xb,
        'C' | 'c' => 0xc,
        'D' | 'd' => 0xd,
        'E' | 'e' => 0xe,
        'F' | 'f' => 0xf,
        _ => return None,
    })
}

fn vec_append<T>(mut v: Vec<T>, item: T) -> Vec<T> {
    v.push(item);
    v
}

fn ws<'a, E: ParseError<&'a str>>(s: &'a str) -> IResult<&'a str, (), E> {
    map(many0(one_of(WHITESPACE)), |_| ())(s)
}

fn with_ws<'a, O, E: ParseError<&'a str>, F: Parser<&'a str, O, E>>(
    f: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, O, E> {
    delimited(ws, f, ws)
}

fn tag_ws<'a, 'b: 'a>(t: &'b str) -> impl Parser<&'a str, &'a str, nom::error::Error<&'a str>> {
    move |s| with_ws(tag_no_case(t))(s)
}

fn is_hex_digit(c: char) -> bool {
    hex_digit(c).is_some()
}

fn hex_n<'a>(n: usize) -> impl Parser<&'a str, &'a str, nom::error::Error<&'a str>> {
    take_while_m_n(n, n, is_hex_digit)
}

fn hex_m_n<'a>(m: usize, n: usize) -> impl Parser<&'a str, &'a str, nom::error::Error<&'a str>> {
    take_while_m_n(m, n, is_hex_digit)
}

#[allow(clippy::many_single_char_names)]
fn uuid(s: &str) -> IResult<&str, Uuid> {
    map(
        tuple((
            terminated(hex_n(8), tag("-")),
            terminated(hex_n(4), tag("-")),
            terminated(hex_n(4), tag("-")),
            terminated(hex_n(4), tag("-")),
            hex_n(12),
        )),
        |(a, b, c, d, e)| {
            let (a, b, c, d) = (
                u32::from_str_radix(a, 16).unwrap(),
                u16::from_str_radix(b, 16).unwrap(),
                u16::from_str_radix(c, 16).unwrap(),
                [
                    &d[0..=1],
                    &d[2..=3],
                    &e[0..=1],
                    &e[2..=3],
                    &e[4..=5],
                    &e[6..=7],
                    &e[8..=9],
                    &e[10..=11],
                ]
                .map(|s| u8::from_str_radix(s, 16).unwrap()),
            );
            Uuid::from_fields(a, b, c, &d)
        },
    )(s)
}

fn escaped_character(s: &str) -> IResult<&str, char> {
    alt((
        none_of("\\\""),
        map_opt(preceded(tag("\\"), one_of("\\\"0nrt")), |c| {
            Some(match c {
                '\\' => '\\',
                '0' => '\0',
                '"' => '"',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                _ => return None,
            })
        }),
        map_opt(preceded(tag("\\x"), hex_n(2)), |hs| {
            char::from_u32(u32::from_str_radix(hs, 16).ok()?)
        }),
        map_opt(preceded(tag("\\u"), hex_m_n(1, 6)), |us: &str| {
            char::from_u32(u32::from_str_radix(us, 16).ok()?)
        }),
        map_opt(
            delimited(tag("\\u{"), hex_m_n(1, 6), tag("}")),
            |us: &str| char::from_u32(u32::from_str_radix(us, 16).ok()?),
        ),
        nom::character::complete::char('\\'),
    ))(s)
}

fn escaped_string_literal(s: &str) -> IResult<&str, String> {
    map(
        delimited(tag("\""), many0(escaped_character), tag("\"")),
        |cs| cs.into_iter().collect::<String>(),
    )(s)
}

const KEYWORDS: [&str; 3] = ["and", "or", "not"];
fn auto_string_literal(s: &str) -> IResult<&str, String> {
    map_opt(many1(none_of(" \r\n\t\u{3000}:;,\"(){}[]|&")), |v| {
        let s = v.into_iter().collect::<String>();
        if KEYWORDS.contains(&s.as_str()) {
            None
        } else {
            Some(s)
        }
    })(s)
}

fn bool_value(s: &str) -> IResult<&str, FieldValue> {
    alt((
        map(tag_no_case("true"), |_| FieldValue::boolean(true)),
        map(tag_no_case("yes"), |_| FieldValue::boolean(true)),
        map(tag_no_case("false"), |_| FieldValue::boolean(false)),
        map(tag_no_case("no"), |_| FieldValue::boolean(false)),
    ))(s)
}

fn int_value(s: &str) -> IResult<&str, FieldValue> {
    map_opt(
        pair(opt(alt((tag("-"), tag("+")))), digit1),
        |(sign, digits): (Option<&str>, &str)| {
            let i = digits.parse::<i64>().ok()?;
            match sign {
                Some("-") => Some(FieldValue::int(-i)),
                Some("+") | None => Some(FieldValue::int(i)),
                _ => None,
            }
        },
    )(s)
}

fn float_value(s: &str) -> IResult<&str, FieldValue> {
    map(double, |d| FieldValue::float(d.into()))(s)
}

fn auto_num_value(s: &str) -> IResult<&str, FieldValue> {
    match (float_value(s), int_value(s)) {
        (Ok((float_i, float_v)), Ok((int_i, int_v))) => {
            // if float matched more characters (i.e. there are fewer characters left),
            // then prefer float; otherwise, use int
            if float_i.len() < int_i.len() {
                Ok((float_i, float_v))
            } else {
                Ok((int_i, int_v))
            }
        }
        (Ok(r), Err(_)) | (Err(_), Ok(r)) => Ok(r),
        (Err(_), Err(_)) => nom::combinator::fail(s),
    }
}

fn string_value_as_cached_string(s: &str) -> IResult<&str, Utf32CachedString> {
    map(alt((escaped_string_literal, auto_string_literal)), |s| {
        s.into()
    })(s)
}

fn string_value(s: &str) -> IResult<&str, FieldValue> {
    map(string_value_as_cached_string, FieldValue::string)(s)
}

fn itemref_value(s: &str) -> IResult<&str, FieldValue> {
    map(
        separated_pair(
            alt((escaped_string_literal, auto_string_literal)),
            tag(":"),
            alt((escaped_string_literal, auto_string_literal)),
        ),
        |(a, b)| FieldValue::itemref((a.into(), b.into())),
    )(s)
}

fn hex_colour_value(s: &str) -> IResult<&str, FieldValue> {
    map(
        alt((
            map_opt(tuple((hex_n(2), hex_n(2), hex_n(2))), |(r, g, b)| {
                Some([
                    u8::from_str_radix(r, 16).ok()?,
                    u8::from_str_radix(g, 16).ok()?,
                    u8::from_str_radix(b, 16).ok()?,
                ])
            }),
            map_opt(tuple((hex_n(1), hex_n(1), hex_n(1))), |(r, g, b)| {
                let rr = u8::from_str_radix(r, 16).ok()?;
                let gg = u8::from_str_radix(g, 16).ok()?;
                let bb = u8::from_str_radix(b, 16).ok()?;
                Some([rr << 4 | rr, gg << 4 | gg, bb << 4 | bb])
            }),
        )),
        |[r, g, b]| FieldValue::colour([r, g, b].into()),
    )(s)
}

fn colour_value(s: &str) -> IResult<&str, FieldValue> {
    preceded(opt(tag("#")), hex_colour_value)(s)
}

fn local_date_value(s: &str) -> IResult<&str, FieldValue> {
    match NaiveDate::parse_and_remainder(s, "%Y-%m-%d") {
        Ok((naive_d, i)) => match local!()
            .from_local_datetime(&naive_d.and_time(NaiveTime::MIN))
            .single()
        {
            Some(dt) => Ok((i, FieldValue::datetime(dt.to_utc()))),
            None => nom::combinator::fail(s),
        },
        Err(_) => nom::combinator::fail(s),
    }
}

fn local_datetime_value(s: &str) -> IResult<&str, FieldValue> {
    match NaiveDateTime::parse_and_remainder(s, "%Y-%m-%dT%H:%M:%S") {
        Ok((naive_dt, i)) => match local!().from_local_datetime(&naive_dt).single() {
            Some(dt) => Ok((i, FieldValue::datetime(dt.to_utc()))),
            None => nom::combinator::fail(s),
        },
        Err(_) => nom::combinator::fail(s),
    }
}

fn timezone_datetime_value(s: &str) -> IResult<&str, FieldValue> {
    match chrono::DateTime::parse_and_remainder(s, "%+") {
        Ok((dt, i)) => Ok((i, FieldValue::datetime(dt.to_utc()))),
        Err(_) => nom::combinator::fail(s),
    }
}

fn datetime_value(s: &str) -> IResult<&str, FieldValue> {
    alt((
        timezone_datetime_value,
        local_datetime_value,
        local_date_value,
    ))(s)
}

fn fold_rest_list_m<'a>(
    val1: FieldValue,
    n: usize,
) -> impl FnMut(&'a str) -> IResult<&'a str, FieldValue> {
    map(
        fold_many_m_n(
            n,
            usize::MAX,
            preceded(tag_ws(","), atomic_field_value),
            move || vec![val1.clone()],
            vec_append,
        ),
        FieldValue::list,
    )
}

fn plain_list_value(s: &str) -> IResult<&str, FieldValue> {
    let (i, val1) = atomic_field_value(s)?;
    fold_rest_list_m(val1, 1)(i)
}

fn delimited_list_value(s: &str) -> IResult<&str, FieldValue> {
    let (i, _) = tag_ws("[").parse(s)?;
    let (i, val1) = opt(atomic_field_value)(i)?;
    if let Some(val1) = val1 {
        terminated(fold_rest_list_m(val1, 0), tag_ws("]"))(i)
    } else {
        map(tag_ws("]"), |_| FieldValue::list(vec![]))(i)
    }
}

fn list_value(s: &str) -> IResult<&str, FieldValue> {
    alt((delimited_list_value, plain_list_value))(s)
}

fn fold_rest_dictionary_m<'a>(
    key1: Utf32CachedString,
    value1: FieldValue,
    n: usize,
) -> impl FnMut(&'a str) -> IResult<&'a str, FieldValue> {
    map(
        fold_many_m_n(
            n,
            usize::MAX,
            preceded(
                alt((tag_ws(","), tag_ws(";"))),
                separated_pair(
                    string_value_as_cached_string,
                    tag_ws(":"),
                    atomic_field_value,
                ),
            ),
            move || vec![(key1.clone(), value1.clone())],
            vec_append,
        ),
        FieldValue::dictionary,
    )
}

fn plain_dictionary_value(s: &str) -> IResult<&str, FieldValue> {
    let (i, (key1, value1)) = separated_pair(
        string_value_as_cached_string,
        tag_ws(":"),
        atomic_field_value,
    )(s)?;
    fold_rest_dictionary_m(key1, value1, 1)(i)
}

fn delimited_dictionary_value(s: &str) -> IResult<&str, FieldValue> {
    let (i, _) = tag_ws("{").parse(s)?;
    let (i, kv1) = opt(separated_pair(
        string_value_as_cached_string,
        tag_ws(":"),
        atomic_field_value,
    ))(i)?;
    if let Some((key1, value1)) = kv1 {
        terminated(fold_rest_dictionary_m(key1, value1, 0), tag_ws("}"))(i)
    } else {
        map(tag_ws("}"), |_| FieldValue::dictionary(vec![]))(i)
    }
}

fn dictionary_value(s: &str) -> IResult<&str, FieldValue> {
    alt((delimited_dictionary_value, plain_dictionary_value))(s)
}

fn atomic_auto_field_value(s: &str) -> IResult<&str, FieldValue> {
    let (i, res) = alt((
        delimited_dictionary_value,
        delimited_list_value,
        datetime_value,
        preceded(tag("#"), hex_colour_value),
        itemref_value,
        auto_num_value,
        bool_value,
        string_value,
    ))(s)?;

    if let Ok((string_i, string_v)) = auto_string_literal(s) {
        // if the auto string parser matched more characters (i.e. there are fewer chars left)
        // then use the auto string value (since the matched value does not use the entire token)
        if string_i.len() < i.len() {
            Ok((string_i, FieldValue::string(string_v.into())))
        } else {
            Ok((i, res))
        }
    } else {
        Ok((i, res))
    }
}

fn tagged_field_value(s: &str) -> IResult<&str, FieldValue> {
    let (i, tag) = terminated(alpha1, tag(":"))(s)?;
    match tag.to_ascii_lowercase().as_str() {
        "bool" => bool_value(i),
        "int" | "uint" | "integer" => int_value(i),
        "float" => float_value(i),
        "str" | "string" => string_value(i),
        "item" | "itemref" => itemref_value(i),
        "color" | "colour" => colour_value(i),
        "dict" | "dictionary" => dictionary_value(i),
        "list" | "array" => list_value(i),
        "date" | "datetime" => datetime_value(i),
        _ => nom::combinator::fail(s),
    }
}

fn auto_field_value(s: &str) -> IResult<&str, FieldValue> {
    alt((
        plain_dictionary_value,
        plain_list_value,
        atomic_auto_field_value,
    ))(s)
}

fn atomic_field_value(s: &str) -> IResult<&str, FieldValue> {
    alt((tagged_field_value, atomic_auto_field_value))(s)
}

fn field_value(s: &str) -> IResult<&str, FieldValue> {
    alt((tagged_field_value, auto_field_value))(s)
}

fn regex_character(s: &str) -> IResult<&str, char> {
    alt((preceded(tag("\\"), one_of("/")), none_of("/")))(s)
}

fn regex_literal(s: &str) -> IResult<&str, Regex> {
    map_res(delimited(tag("/"), many1(regex_character), tag("/")), |v| {
        Regex::new(v.into_iter().collect::<String>().as_str())
    })(s)
}

fn folder_match(s: &str) -> IResult<&str, FilterExpression> {
    map(
        preceded(
            tag("folder:"),
            alt((escaped_string_literal, auto_string_literal)),
        ),
        |s| FilterExpression::FolderMatch(PathBuf::from(s).into_boxed_path()),
    )(s)
}

fn tag_match(s: &str) -> IResult<&str, FilterExpression> {
    map(preceded(tag("tag:"), uuid), |id| {
        FilterExpression::TagMatch(id)
    })(s)
}

fn exact_text_search(s: &str) -> IResult<&str, FilterExpression> {
    map(escaped_string_literal, |s| {
        FilterExpression::ExactTextSearch(s.into())
    })(s)
}

fn text_search(s: &str) -> IResult<&str, FilterExpression> {
    map(auto_string_literal, |s| {
        FilterExpression::TextSearch(s.into())
    })(s)
}

fn filter_atom(s: &str) -> IResult<&str, FilterExpression> {
    alt((
        delimited(tag_ws("("), filter_expression, tag_ws(")")),
        with_ws(alt((
            folder_match,
            field_match,
            tag_match,
            exact_text_search,
            text_search,
        ))),
    ))(s)
}

fn not_expression(s: &str) -> IResult<&str, FilterExpression> {
    map(
        alt((
            delimited(
                pair(tag_ws("not"), tag_ws("(")),
                filter_expression,
                tag_ws(")"),
            ),
            preceded(tag_ws("!"), filter_atom),
            preceded(
                pair(tag_no_case("not"), many1(one_of(" \r\n\t\u{3000}"))),
                filter_atom,
            ),
        )),
        |exp| FilterExpression::Not(Box::new(exp)),
    )(s)
    .or_else(|_| filter_atom(s))
}

fn field_match_operator(s: &str) -> IResult<&str, ValueMatchExpressionDiscriminants> {
    with_ws(alt((
        map(
            alt((
                tag("~"),
                tag("~="),
                tag("=~"),
                tag_no_case("matches"),
                tag_no_case("like"),
            )),
            |_| ValueMatchExpressionDiscriminants::Regex,
        ),
        map(alt((tag("="), tag("=="), tag_no_case("eq"))), |_| {
            ValueMatchExpressionDiscriminants::Equals
        }),
        map(
            alt((tag("!="), tag("<>"), tag_no_case("neq"), tag_no_case("ne"))),
            |_| ValueMatchExpressionDiscriminants::NotEquals,
        ),
        map(alt((tag_no_case("in"),)), |_| {
            ValueMatchExpressionDiscriminants::IsOneOf
        }),
        map(alt((tag_no_case("contains"), tag_no_case("has"))), |_| {
            ValueMatchExpressionDiscriminants::Contains
        }),
        map(
            alt((tag("<="), tag_no_case("leq"), tag_no_case("le"))),
            |_| ValueMatchExpressionDiscriminants::LessThanOrEqual,
        ),
        map(
            alt((tag(">="), tag_no_case("geq"), tag_no_case("ge"))),
            |_| ValueMatchExpressionDiscriminants::GreaterThanOrEqual,
        ),
        map(alt((tag("<"), tag_no_case("lt"))), |_| {
            ValueMatchExpressionDiscriminants::LessThan
        }),
        map(alt((tag(">"), tag_no_case("gt"))), |_| {
            ValueMatchExpressionDiscriminants::GreaterThan
        }),
        map(
            alt((
                tag("^="),
                tag_no_case("starts with"),
                tag_no_case("startswith"),
                tag_no_case("starts"),
            )),
            |_| ValueMatchExpressionDiscriminants::StartsWith,
        ),
        map(
            alt((
                tag("$="),
                tag_no_case("ends with"),
                tag_no_case("endswith"),
                tag_no_case("ends"),
            )),
            |_| ValueMatchExpressionDiscriminants::EndsWith,
        ),
    )))(s)
}

fn field_match(s: &str) -> IResult<&str, FilterExpression> {
    let (i, (id, op)) = pair(preceded(tag_ws("field:"), uuid), field_match_operator)(s)?;
    map(
        move |i| match op {
            ValueMatchExpressionDiscriminants::Equals => {
                map(field_value, ValueMatchExpression::Equals)(i)
            }
            ValueMatchExpressionDiscriminants::NotEquals => {
                map(field_value, ValueMatchExpression::NotEquals)(i)
            }
            ValueMatchExpressionDiscriminants::IsOneOf => map_opt(list_value, |l| {
                Some(ValueMatchExpression::IsOneOf(
                    l.as_list_opt()?.iter().cloned().collect(),
                ))
            })(i),
            ValueMatchExpressionDiscriminants::Contains => {
                map(field_value, ValueMatchExpression::Contains)(i)
            }
            ValueMatchExpressionDiscriminants::LessThan => {
                map(field_value, ValueMatchExpression::LessThan)(i)
            }
            ValueMatchExpressionDiscriminants::LessThanOrEqual => {
                map(field_value, ValueMatchExpression::LessThanOrEqual)(i)
            }
            ValueMatchExpressionDiscriminants::GreaterThan => {
                map(field_value, ValueMatchExpression::GreaterThan)(i)
            }
            ValueMatchExpressionDiscriminants::GreaterThanOrEqual => {
                map(field_value, ValueMatchExpression::GreaterThanOrEqual)(i)
            }
            ValueMatchExpressionDiscriminants::StartsWith => {
                map(field_value, ValueMatchExpression::StartsWith)(i)
            }
            ValueMatchExpressionDiscriminants::EndsWith => {
                map(field_value, ValueMatchExpression::EndsWith)(i)
            }
            ValueMatchExpressionDiscriminants::Regex => {
                map(regex_literal, |re| ValueMatchExpression::Regex(re.into()))(i)
            }
        },
        move |expr| FilterExpression::FieldMatch(id, expr),
    )(i)
}

fn filter_expression_implicit_and(s: &str) -> IResult<&str, FilterExpression> {
    let (rest, exp1) = not_expression(s)?;
    let Ok((rest, exp2)) = not_expression(rest) else {
        return Ok((rest, exp1));
    };
    let and_exp = FilterExpression::And(Box::new(exp1), Box::new(exp2));
    fold_many0(
        not_expression,
        move || and_exp.clone(),
        |exp1, exp2| FilterExpression::And(Box::new(exp1), Box::new(exp2)),
    )(rest)
}

fn or_expression(s: &str) -> IResult<&str, FilterExpression> {
    map(
        alt((
            delimited(
                pair(tag_ws("or"), tag_ws("(")),
                separated_pair(filter_expression, tag_ws(","), filter_expression),
                tag_ws(")"),
            ),
            separated_pair(
                filter_expression_implicit_and,
                alt((tag_ws("||"), tag_ws("or"))),
                filter_expression_implicit_and,
            ),
        )),
        |(exp1, exp2)| FilterExpression::Or(Box::new(exp1), Box::new(exp2)),
    )(s)
    .or_else(|_| filter_expression_implicit_and(s))
}

fn and_expression(s: &str) -> IResult<&str, FilterExpression> {
    map(
        alt((
            delimited(
                pair(tag_ws("and"), tag_ws("(")),
                separated_pair(filter_expression, tag_ws(","), filter_expression),
                tag_ws(")"),
            ),
            separated_pair(
                or_expression,
                alt((tag_ws("&&"), tag_ws("and"))),
                or_expression,
            ),
        )),
        |(exp1, exp2)| FilterExpression::And(Box::new(exp1), Box::new(exp2)),
    )(s)
    .or_else(|_| or_expression(s))
}

pub fn filter_expression(s: &str) -> IResult<&str, FilterExpression> {
    with_ws(and_expression)(s)
}

#[cfg(test)]
mod test {
    use super::*;

    use super::FieldValue as V;

    static TZ_OVERRIDE_LOCK: Mutex<()> = Mutex::new(());

    #[allow(clippy::cast_lossless)]
    fn with_tz(hours: i8, f: impl FnOnce()) {
        let _l = TZ_OVERRIDE_LOCK.lock().unwrap();
        *LOCAL.lock().unwrap() = FixedOffset::east_opt(hours as i32 * 3600);
        f();
    }

    fn and(exp1: FilterExpression, exp2: FilterExpression) -> FilterExpression {
        FilterExpression::And(Box::new(exp1), Box::new(exp2))
    }

    fn or(exp1: FilterExpression, exp2: FilterExpression) -> FilterExpression {
        FilterExpression::Or(Box::new(exp1), Box::new(exp2))
    }

    fn not(exp: FilterExpression) -> FilterExpression {
        FilterExpression::Not(Box::new(exp))
    }

    fn exact(text: &str) -> FilterExpression {
        FilterExpression::ExactTextSearch(text.into())
    }

    fn text(text: &str) -> FilterExpression {
        FilterExpression::TextSearch(text.into())
    }

    fn eq(id: Uuid, value: V) -> FilterExpression {
        FilterExpression::FieldMatch(id, ValueMatchExpression::Equals(value))
    }

    fn ne(id: Uuid, value: V) -> FilterExpression {
        FilterExpression::FieldMatch(id, ValueMatchExpression::NotEquals(value))
    }

    fn is_one_of(id: Uuid, values: &[V]) -> FilterExpression {
        FilterExpression::FieldMatch(
            id,
            ValueMatchExpression::IsOneOf(values.iter().cloned().collect()),
        )
    }

    fn contains(id: Uuid, value: V) -> FilterExpression {
        FilterExpression::FieldMatch(id, ValueMatchExpression::Contains(value))
    }

    fn lt(id: Uuid, value: V) -> FilterExpression {
        FilterExpression::FieldMatch(id, ValueMatchExpression::LessThan(value))
    }

    fn le(id: Uuid, value: V) -> FilterExpression {
        FilterExpression::FieldMatch(id, ValueMatchExpression::LessThanOrEqual(value))
    }

    fn gt(id: Uuid, value: V) -> FilterExpression {
        FilterExpression::FieldMatch(id, ValueMatchExpression::GreaterThan(value))
    }

    fn ge(id: Uuid, value: V) -> FilterExpression {
        FilterExpression::FieldMatch(id, ValueMatchExpression::GreaterThanOrEqual(value))
    }

    fn starts(id: Uuid, value: V) -> FilterExpression {
        FilterExpression::FieldMatch(id, ValueMatchExpression::StartsWith(value))
    }

    fn ends(id: Uuid, value: V) -> FilterExpression {
        FilterExpression::FieldMatch(id, ValueMatchExpression::EndsWith(value))
    }

    fn regex(id: Uuid, pat: &str) -> FilterExpression {
        FilterExpression::FieldMatch(
            id,
            ValueMatchExpression::Regex(Regex::new(pat).unwrap().into()),
        )
    }

    #[allow(clippy::unnecessary_wraps)]
    fn ok<T>(v: T) -> IResult<&'static str, T> {
        Ok(("", v))
    }

    #[test]
    fn test_filter_expression_and() {
        assert_eq!(
            ok(and(exact("abc"), exact("def"))),
            filter_expression("and(\"abc\", \"def\")")
        );

        assert_eq!(
            ok(and(text("abc"), text("def"))),
            filter_expression("and(abc, def)")
        );

        assert_eq!(
            ok(and(exact("abc"), exact("def"))),
            filter_expression("\"abc\" && \"def\"")
        );

        assert_eq!(
            ok(and(exact("abc"), exact("def"))),
            filter_expression("\"abc\" and \"def\"")
        );

        assert_eq!(
            ok(and(text("abc"), text("def"))),
            filter_expression("abc and def")
        );
    }

    #[test]
    fn test_filter_expression_or() {
        assert_eq!(
            ok(or(exact("abc"), exact("def"))),
            filter_expression("or(\"abc\", \"def\")")
        );

        assert_eq!(
            ok(or(text("abc"), text("def"))),
            filter_expression("or(abc, def)")
        );

        assert_eq!(
            ok(or(exact("abc"), exact("def"))),
            filter_expression("\"abc\" || \"def\"")
        );

        assert_eq!(
            ok(or(exact("abc"), exact("def"))),
            filter_expression("\"abc\" or \"def\"")
        );

        assert_eq!(
            ok(or(text("abc"), text("def"))),
            filter_expression("abc or def")
        );

        assert_eq!(ok(text("abcordef")), filter_expression("abcordef"));
    }

    #[test]
    fn test_filter_expression_not() {
        assert_eq!(ok(not(text("abc"))), filter_expression("!abc"));

        assert_eq!(ok(not(text("abc"))), filter_expression("not abc"));

        assert_eq!(ok(not(text("abc"))), filter_expression("not ( abc )"));

        assert_eq!(ok(text("notabc")), filter_expression("notabc"));
    }

    #[test]
    fn test_filter_expression_implicit_and() {
        assert_eq!(
            ok(and(exact("abc"), exact("def"))),
            filter_expression("\"abc\" \"def\"")
        );
        assert_eq!(
            ok(and(exact("abc"), text("def"))),
            filter_expression("\"abc\" def")
        );

        assert_eq!(
            ok(and(text("abc"), text("def"))),
            filter_expression("abc def")
        );

        assert_eq!(
            ok(and(
                or(text("abc"), text("def")),
                and(text("ghi"), text("jkl"))
            )),
            filter_expression("abc or def and ghi jkl")
        );

        assert_eq!(
            ok(and(
                and(and(text("abc"), not(text("def"))), text("ghi")),
                exact("jkl")
            )),
            filter_expression("abc !def ghi \"jkl\"")
        );

        assert_eq!(
            ok(and(
                or(and(text("abc"), text("def")), text("ghi")),
                and(text("jkl"), text("mno"))
            )),
            filter_expression("abc def or ghi and jkl mno")
        );
    }

    #[test]
    fn test_field_value_bool() {
        assert_eq!(ok(V::boolean(true)), field_value("true"));
        assert_eq!(ok(V::boolean(false)), field_value("false"));
        assert_eq!(ok(V::boolean(true)), field_value("yes"));
        assert_eq!(ok(V::boolean(false)), field_value("no"));
        assert_eq!(ok(V::boolean(true)), field_value("bool:true"));
        assert_eq!(ok(V::boolean(false)), field_value("bool:false"));
    }

    #[test]
    fn test_field_value_int() {
        assert_eq!(ok(V::int(0)), field_value("0"));
        assert_eq!(ok(V::int(12_943_128_349)), field_value("12943128349"));
        assert_eq!(ok(V::int(-64321)), field_value("-64321"));
        assert_eq!(ok(V::int(-12100)), field_value("int:-12100"));
    }

    #[test]
    fn test_field_value_float() {
        assert_eq!(ok(V::float(0.0.into())), field_value("0.0"));
        assert_eq!(
            ok(V::float((-98713.12981e-50).into())),
            field_value("-98713.12981e-50")
        );
        assert_eq!(ok(V::float(f64::INFINITY.into())), field_value("inf"));
        assert_eq!(
            ok(V::float(12908.14289e-220.into())),
            field_value("float:12908.14289e-220")
        );
    }

    #[test]
    fn test_field_value_datetime() {
        with_tz(-7, || {
            assert_eq!(
                ok(V::datetime(Utc.timestamp_nanos(1_718_597_772_000_000_000))),
                field_value("2024-06-17T12:16:12+08:00")
            );
            assert_eq!(
                ok(V::datetime(Utc.timestamp_nanos(1_718_597_772_000_000_000))),
                field_value("2024-06-17T04:16:12Z")
            );
            assert_eq!(
                ok(V::datetime(Utc.timestamp_nanos(1_718_597_772_000_000_000))),
                field_value("2024-06-16T21:16:12")
            );
            assert_eq!(
                ok(V::datetime(Utc.timestamp_nanos(1_718_607_600_000_000_000))),
                field_value("2024-06-17")
            );
            assert_eq!(
                ok(V::datetime(Utc.timestamp_nanos(1_718_597_772_000_000_000))),
                field_value("datetime:2024-06-17T12:16:12+08:00")
            );
        });
    }

    #[test]
    fn test_field_value_itemref() {
        assert_eq!(
            ok(V::itemref(("abc".into(), "def".into()))),
            field_value("abc:def")
        );
        assert_eq!(
            ok(V::itemref(("abc".into(), "def".into()))),
            field_value("\"abc\":\"def\"")
        );
        assert_eq!(ok(V::string("abc:def".into())), field_value("\"abc:def\""));
        assert_eq!(
            ok(V::itemref(("abc".into(), "def".into()))),
            field_value("itemref:abc:def")
        );
    }

    #[test]
    fn test_field_value_colour() {
        assert_eq!(ok(V::colour([0, 119, 255].into())), field_value("#0077FF"));
        assert_eq!(ok(V::string("0077FF".into())), field_value("0077FF"));
        assert_eq!(
            ok(V::colour([0, 119, 255].into())),
            field_value("colour:0077FF")
        );
        assert_eq!(
            ok(V::colour([0, 119, 255].into())),
            field_value("color:0077FF")
        );
        assert_eq!(ok(V::colour([0, 119, 255].into())), field_value("#07F"));
        assert_eq!(ok(V::string("07F".into())), field_value("07F"));
        assert_eq!(
            ok(V::colour([0, 119, 255].into())),
            field_value("colour:07F")
        );
        assert_eq!(
            ok(V::colour([0, 119, 255].into())),
            field_value("color:07F")
        );
    }

    #[test]
    fn test_field_value_list() {
        assert_eq!(ok(V::list(vec![])), field_value("[]"));
        assert_eq!(
            ok(V::list(vec![V::string("abc".into())])),
            field_value("[ abc ]")
        );
        assert_eq!(
            ok(V::list(vec![
                V::string("abc".into()),
                V::string("def".into())
            ])),
            field_value("abc,def")
        );
        assert_eq!(
            ok(V::list(vec![
                V::float((-26.4).into()),
                V::int(15.into()),
                V::itemref(("abc".into(), "def".into()))
            ])),
            field_value("-26.4, 15, abc:def")
        );
    }

    #[test]
    fn test_field_value_dictionary() {
        assert_eq!(ok(V::dictionary(vec![])), field_value("{}"));
        assert_eq!(
            ok(V::dictionary(vec![("abc".into(), V::string("def".into()))])),
            field_value("{abc:def}")
        );
        assert_eq!(
            ok(V::dictionary(vec![("abc".into(), V::string("def".into()))])),
            field_value("{\"abc\": \"def\"}")
        );
        assert_eq!(
            ok(V::dictionary(vec![
                ("abc".into(), V::string("def".into())),
                ("ghi".into(), V::int(32))
            ])),
            field_value("{\"abc\": \"def\", \"ghi\": 32}")
        );
        assert_eq!(
            ok(V::dictionary(vec![
                ("abc".into(), V::string("def".into())),
                ("ghi".into(), V::int(32))
            ])),
            field_value("abc:def;ghi:32")
        );
        assert_eq!(
            ok(V::dictionary(vec![
                ("abc".into(), V::string("def".into())),
                ("ghi".into(), V::int(32))
            ])),
            field_value("abc:def,ghi:32")
        );
    }

    #[test]
    fn test_field_match() {
        with_tz(8, || {
            let id1 = crate::fields::image::WIDTH.id;
            let id2 = crate::fields::meta::ALIASES.id;
            let id3 = crate::fields::general::MEDIA_TYPE.id;
            assert_eq!(
                ok(eq(id1, V::int(600))),
                filter_expression(format!("field:{id1}=600").as_str())
            );
            assert_eq!(
                ok(ne(id1, V::float(549.2.into()))),
                filter_expression(format!("field:{id1} ne 549.2").as_str())
            );
            assert_eq!(
                ok(is_one_of(id1, &[V::int(600), V::int(800)])),
                filter_expression(format!("field:{id1} in 600,800").as_str())
            );
            assert_eq!(
                ok(contains(id2, V::string("abc".into()))),
                filter_expression(format!("field:{id2} contains abc").as_str())
            );
            assert_eq!(
                ok(lt(id1, V::int(-500))),
                filter_expression(format!("field:{id1}<-500").as_str())
            );
            assert_eq!(
                ok(le(id1, V::int(600))),
                filter_expression(format!("field:{id1}<= 600").as_str())
            );
            assert_eq!(
                ok(ge(
                    id1,
                    V::datetime(Utc.timestamp_nanos(1_718_553_600_000_000_000))
                )),
                filter_expression(format!("field:{id1}>=2024-06-17").as_str())
            );
            assert_eq!(
                ok(gt(id1, V::string("ne".into()))),
                filter_expression(format!("field:{id1}>ne").as_str())
            );
            assert_eq!(
                ok(starts(id3, V::string("image/".into()))),
                filter_expression(format!("field:{id3}^=\"image/\"").as_str())
            );
            assert_eq!(
                ok(starts(id3, V::string("image/".into()))),
                filter_expression(format!("field:{id3} starts with image/").as_str())
            );
            assert_eq!(
                ok(ends(id3, V::string("/jpeg".into()))),
                filter_expression(format!("field:{id3}$=/jpeg").as_str())
            );
            assert_eq!(
                ok(ends(id3, V::string("/jpeg".into()))),
                filter_expression(format!("field:{id3} ends with \"/jpeg\"").as_str())
            );
            assert_eq!(
                ok(regex(id3, r"[^/]+/[^/]+")),
                filter_expression(format!(r"field:{id3}=~/[^\/]+\/[^\/]+/").as_str())
            );
            assert_eq!(
                ok(regex(id1, r"6..\..")),
                filter_expression(format!(r"field:{id1} like /6..\../").as_str())
            );
        });
    }
}
