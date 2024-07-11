use crate::data::{
    FieldDefinition, FieldValue, FilterExpression, SerialColour, Utf32CachedString,
    ValueMatchExpression, Vault,
};
use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case, take_while_m_n};
use nom::character::complete::{alpha1, digit0, digit1, none_of, one_of};
use nom::combinator::{map, map_opt, map_res, opt};
use nom::error::ParseError;
use nom::multi::{count, fold_many0, fold_many1, fold_many_m_n, many0, many1};
use nom::sequence::{delimited, pair, preceded, separated_pair, terminated, tuple};
use nom::{FindSubstring, IResult, InputIter, Parser, Slice};
use std::cell::RefCell;
use std::path::PathBuf;
use uuid::Uuid;

use crate::data::filter::ValueMatchExpressionDiscriminants;
use chrono::{
    DateTime, FixedOffset, MappedLocalTime, NaiveDate, NaiveDateTime, NaiveTime, ParseResult,
    TimeZone, Utc,
};
use eframe::egui;
use eframe::egui::text::{CCursor, CCursorRange, CursorRange};
use eframe::egui::text_selection::text_cursor_state::byte_index_from_char_index;
use eframe::egui::TextBuffer;
use eframe::epaint::text::cursor::{Cursor, PCursor, RCursor};
use itertools::Itertools;
use nom::number::complete::double;
use nom_locate::{position, LocatedSpan};
use regex::Regex;
use std::str::FromStr;
use std::sync::Mutex;
use tracing::warn;

#[cfg(not(test))]
static LOCAL: Mutex<Option<chrono::Local>> = Mutex::new(Some(chrono::Local));

#[cfg(test)]
static LOCAL: Mutex<Option<FixedOffset>> = Mutex::new(FixedOffset::east_opt(8 * 3600));

macro_rules! local {
    () => {
        LOCAL.lock().unwrap().unwrap()
    };
}

type Span<'a> = LocatedSpan<&'a str>;

pub const WHITESPACE: &str = " \t\r\n\u{3000}";

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

fn substring_to_span<'a>(s: Span<'a>, substr: &'_ str) -> Option<Span<'a>> {
    let start = s.find_substring(substr)?;
    Some(s.slice(start..start + substr.len()))
}

fn ws<'a, E: ParseError<Span<'a>>>(s: Span<'a>) -> IResult<Span<'a>, (), E> {
    map(many0(one_of(WHITESPACE)), |_| ())(s)
}

fn with_ws<'a, O, E: ParseError<Span<'a>>, F: Parser<Span<'a>, O, E>>(
    f: F,
) -> impl FnMut(Span<'a>) -> IResult<Span<'a>, O, E> {
    delimited(ws, f, ws)
}

fn tag_ws<'a, 'b: 'a>(t: &'b str) -> impl Parser<Span<'a>, Span<'a>, nom::error::Error<Span<'a>>> {
    move |s| with_ws(tag_no_case(t))(s)
}

fn is_hex_digit(c: char) -> bool {
    hex_digit(c).is_some()
}

fn hex_n<'a>(n: usize) -> impl Parser<Span<'a>, Span<'a>, nom::error::Error<Span<'a>>> {
    take_while_m_n(n, n, is_hex_digit)
}

fn hex_m_n<'a>(m: usize, n: usize) -> impl Parser<Span<'a>, Span<'a>, nom::error::Error<Span<'a>>> {
    take_while_m_n(m, n, is_hex_digit)
}

#[allow(clippy::many_single_char_names)]
fn uuid(s: Span) -> IResult<Span, Uuid> {
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
                u32::from_str_radix(a.as_str(), 16).unwrap(),
                u16::from_str_radix(b.as_str(), 16).unwrap(),
                u16::from_str_radix(c.as_str(), 16).unwrap(),
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

fn escaped_character(s: Span) -> IResult<Span, char> {
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
            char::from_u32(u32::from_str_radix(hs.as_str(), 16).ok()?)
        }),
        map_opt(preceded(tag("\\u"), hex_m_n(1, 6)), |us| {
            char::from_u32(u32::from_str_radix(us.as_str(), 16).ok()?)
        }),
        map_opt(delimited(tag("\\u{"), hex_m_n(1, 6), tag("}")), |us| {
            char::from_u32(u32::from_str_radix(us.as_str(), 16).ok()?)
        }),
        nom::character::complete::char('\\'),
    ))(s)
}

fn escaped_string_literal(s: Span) -> IResult<Span, String> {
    map(
        delimited(tag("\""), many0(escaped_character), tag("\"")),
        |cs| cs.into_iter().collect::<String>(),
    )(s)
}

const KEYWORDS: [&str; 3] = ["and", "or", "not"];
fn auto_string_literal(s: Span) -> IResult<Span, String> {
    map_opt(many1(none_of(" \r\n\t\u{3000}:;,\"(){}[]|&")), |v| {
        let s = v.into_iter().collect::<String>();
        if KEYWORDS.contains(&s.as_str()) {
            None
        } else {
            Some(s)
        }
    })(s)
}

fn bool_value(s: Span) -> IResult<Span, FieldValue> {
    alt((
        map(tag_no_case("true"), |_| FieldValue::boolean(true)),
        map(tag_no_case("yes"), |_| FieldValue::boolean(true)),
        map(tag_no_case("false"), |_| FieldValue::boolean(false)),
        map(tag_no_case("no"), |_| FieldValue::boolean(false)),
    ))(s)
}

fn int_value(s: Span) -> IResult<Span, FieldValue> {
    map_opt(
        pair(opt(alt((tag("-"), tag("+")))), digit1),
        |(sign, digits): (Option<Span>, Span)| {
            let i = digits.parse::<i64>().ok()?;
            match sign {
                Some(s) if s.as_str() == "-" => Some(FieldValue::int(-i)),
                Some(s) if s.as_str() == "+" => Some(FieldValue::int(i)),
                None => Some(FieldValue::int(i)),
                _ => None,
            }
        },
    )(s)
}

fn float_value(s: Span) -> IResult<Span, FieldValue> {
    map(double, |d| FieldValue::float(d.into()))(s)
}

fn auto_num_value(s: Span) -> IResult<Span, FieldValue> {
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

fn string_value_as_cached_string(s: Span) -> IResult<Span, Utf32CachedString> {
    map(alt((escaped_string_literal, auto_string_literal)), |s| {
        s.into()
    })(s)
}

fn string_value(s: Span) -> IResult<Span, FieldValue> {
    map(string_value_as_cached_string, FieldValue::string)(s)
}

fn itemref_value(s: Span) -> IResult<Span, FieldValue> {
    map(
        separated_pair(
            alt((escaped_string_literal, auto_string_literal)),
            tag(":"),
            alt((escaped_string_literal, auto_string_literal)),
        ),
        |(a, b)| FieldValue::itemref((a.into(), b.into())),
    )(s)
}

fn hex_colour_value(s: Span) -> IResult<Span, FieldValue> {
    map(
        alt((
            map_opt(tuple((hex_n(2), hex_n(2), hex_n(2))), |(r, g, b)| {
                Some([
                    u8::from_str_radix(r.as_str(), 16).ok()?,
                    u8::from_str_radix(g.as_str(), 16).ok()?,
                    u8::from_str_radix(b.as_str(), 16).ok()?,
                ])
            }),
            map_opt(tuple((hex_n(1), hex_n(1), hex_n(1))), |(r, g, b)| {
                let rr = u8::from_str_radix(r.as_str(), 16).ok()?;
                let gg = u8::from_str_radix(g.as_str(), 16).ok()?;
                let bb = u8::from_str_radix(b.as_str(), 16).ok()?;
                Some([rr << 4 | rr, gg << 4 | gg, bb << 4 | bb])
            }),
        )),
        |[r, g, b]| FieldValue::colour([r, g, b].into()),
    )(s)
}

fn colour_value(s: Span) -> IResult<Span, FieldValue> {
    preceded(opt(tag("#")), hex_colour_value)(s)
}

fn local_date_value(s: Span) -> IResult<Span, FieldValue> {
    match NaiveDate::parse_and_remainder(s.as_str(), "%Y-%m-%d") {
        Ok((naive_d, i)) => match local!()
            .from_local_datetime(&naive_d.and_time(NaiveTime::MIN))
            .earliest()
        {
            Some(dt) => Ok((
                substring_to_span(s, i).unwrap(),
                FieldValue::datetime(dt.to_utc()),
            )),
            None => nom::combinator::fail(s),
        },
        Err(_) => nom::combinator::fail(s),
    }
}

fn local_datetime_value(s: Span) -> IResult<Span, FieldValue> {
    match NaiveDateTime::parse_and_remainder(s.as_str(), "%Y-%m-%dT%H:%M:%S") {
        Ok((naive_dt, i)) => match local!().from_local_datetime(&naive_dt).earliest() {
            Some(dt) => Ok((
                substring_to_span(s, i).unwrap(),
                FieldValue::datetime(dt.to_utc()),
            )),
            None => nom::combinator::fail(s),
        },
        Err(_) => nom::combinator::fail(s),
    }
}

fn timezone_datetime_value(s: Span) -> IResult<Span, FieldValue> {
    match chrono::DateTime::parse_and_remainder(s.as_str(), "%+") {
        Ok((dt, i)) => Ok((
            substring_to_span(s, i).unwrap(),
            FieldValue::datetime(dt.to_utc()),
        )),
        Err(_) => nom::combinator::fail(s),
    }
}

fn datetime_value(s: Span) -> IResult<Span, FieldValue> {
    alt((
        timezone_datetime_value,
        local_datetime_value,
        local_date_value,
    ))(s)
}

fn fold_rest_list_m<'a>(
    val1: FieldValue,
    n: usize,
) -> impl FnMut(Span<'a>) -> IResult<Span<'a>, FieldValue> {
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

fn plain_list_value(s: Span) -> IResult<Span, FieldValue> {
    let (i, val1) = atomic_field_value(s)?;
    fold_rest_list_m(val1, 1)(i)
}

fn delimited_list_value(s: Span) -> IResult<Span, FieldValue> {
    let (i, _) = tag_ws("[").parse(s)?;
    let (i, val1) = opt(atomic_field_value)(i)?;
    if let Some(val1) = val1 {
        terminated(fold_rest_list_m(val1, 0), tag_ws("]"))(i)
    } else {
        map(tag_ws("]"), |_| FieldValue::list(vec![]))(i)
    }
}

fn list_value(s: Span) -> IResult<Span, FieldValue> {
    alt((delimited_list_value, plain_list_value))(s)
}

fn fold_rest_dictionary_m<'a>(
    key1: Utf32CachedString,
    value1: FieldValue,
    n: usize,
) -> impl FnMut(Span<'a>) -> IResult<Span<'a>, FieldValue> {
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

fn plain_dictionary_value(s: Span) -> IResult<Span, FieldValue> {
    let (i, (key1, value1)) = separated_pair(
        string_value_as_cached_string,
        tag_ws(":"),
        atomic_field_value,
    )(s)?;
    fold_rest_dictionary_m(key1, value1, 1)(i)
}

fn delimited_dictionary_value(s: Span) -> IResult<Span, FieldValue> {
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

fn dictionary_value(s: Span) -> IResult<Span, FieldValue> {
    alt((delimited_dictionary_value, plain_dictionary_value))(s)
}

fn atomic_auto_field_value(s: Span) -> IResult<Span, FieldValue> {
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

fn tagged_field_value(s: Span) -> IResult<Span, FieldValue> {
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

fn auto_field_value(s: Span) -> IResult<Span, FieldValue> {
    alt((
        plain_dictionary_value,
        plain_list_value,
        atomic_auto_field_value,
    ))(s)
}

fn atomic_field_value(s: Span) -> IResult<Span, FieldValue> {
    alt((tagged_field_value, atomic_auto_field_value))(s)
}

fn field_value(s: Span) -> IResult<Span, FieldValue> {
    alt((tagged_field_value, auto_field_value))(s)
}

fn regex_character(s: Span) -> IResult<Span, char> {
    alt((preceded(tag("\\"), one_of("/")), none_of("/")))(s)
}

fn regex_literal(s: Span) -> IResult<Span, Regex> {
    map_res(delimited(tag("/"), many1(regex_character), tag("/")), |v| {
        Regex::new(v.into_iter().collect::<String>().as_str())
    })(s)
}

fn record_range<'a>(
    mut f: impl Parser<Span<'a>, FilterExpression, nom::error::Error<Span<'a>>>,
) -> impl FnMut(Span<'a>) -> IResult<Span<'a>, FilterExpressionParseNode> {
    move |s| {
        let (s, start) = position(s)?;
        let (s, expr) = f.parse(s)?;
        let (s, end) = position(s)?;
        Ok((
            s,
            FilterExpressionParseNode::leaf(expr, start.location_offset(), end.location_offset()),
        ))
    }
}

fn folder_match(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    record_range(map(
        preceded(
            tag("folder:"),
            alt((escaped_string_literal, auto_string_literal)),
        ),
        |s| FilterExpression::FolderMatch(PathBuf::from(s).into_boxed_path()),
    ))(s)
}

fn tag_match(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    record_range(map(preceded(tag("field:"), uuid), |id| {
        FilterExpression::TagMatch(id)
    }))(s)
}

fn exact_text_search(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    record_range(map(escaped_string_literal, |s| {
        FilterExpression::ExactTextSearch(s.into())
    }))(s)
}

fn text_search(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    record_range(map(auto_string_literal, |s| {
        FilterExpression::TextSearch(s.into())
    }))(s)
}

fn filter_atom(s: Span) -> IResult<Span, FilterExpressionParseNode> {
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

fn not_expression(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    let (s, start) = position(s)?;
    match alt((
        delimited(
            pair(tag_ws("not"), tag_ws("(")),
            filter_expression,
            tag_ws(")"),
        ),
        preceded(alt((tag_ws("!"), tag_ws("-"))), filter_atom),
        preceded(
            pair(tag_no_case("not"), many1(one_of(" \r\n\t\u{3000}"))),
            filter_atom,
        ),
    ))(s)
    {
        Ok((s, inner_node)) => {
            let (s, end) = position(s)?;
            let expr = FilterExpression::Not(Box::new(inner_node.expr.clone()));
            Ok((
                s,
                FilterExpressionParseNode::parent(
                    expr,
                    vec![inner_node],
                    start.location_offset(),
                    end.location_offset(),
                ),
            ))
        }
        Err(_) => filter_atom(s),
    }
}

fn field_match_operator(s: Span) -> IResult<Span, ValueMatchExpressionDiscriminants> {
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
        map(alt((tag("=="), tag("="), tag_no_case("eq"))), |_| {
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

fn field_match(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    record_range(|s| {
        let (s, (id, op)) = pair(preceded(tag_ws("field:"), uuid), field_match_operator)(s)?;

        let (s, expr) = match op {
            ValueMatchExpressionDiscriminants::Equals => {
                map(field_value, ValueMatchExpression::Equals)(s)
            }
            ValueMatchExpressionDiscriminants::NotEquals => {
                map(field_value, ValueMatchExpression::NotEquals)(s)
            }
            ValueMatchExpressionDiscriminants::IsOneOf => map_opt(list_value, |l| {
                Some(ValueMatchExpression::IsOneOf(
                    l.as_list_opt()?.iter().cloned().collect(),
                ))
            })(s),
            ValueMatchExpressionDiscriminants::Contains => {
                map(field_value, ValueMatchExpression::Contains)(s)
            }
            ValueMatchExpressionDiscriminants::LessThan => {
                map(field_value, ValueMatchExpression::LessThan)(s)
            }
            ValueMatchExpressionDiscriminants::LessThanOrEqual => {
                map(field_value, ValueMatchExpression::LessThanOrEqual)(s)
            }
            ValueMatchExpressionDiscriminants::GreaterThan => {
                map(field_value, ValueMatchExpression::GreaterThan)(s)
            }
            ValueMatchExpressionDiscriminants::GreaterThanOrEqual => {
                map(field_value, ValueMatchExpression::GreaterThanOrEqual)(s)
            }
            ValueMatchExpressionDiscriminants::StartsWith => {
                map(field_value, ValueMatchExpression::StartsWith)(s)
            }
            ValueMatchExpressionDiscriminants::EndsWith => {
                map(field_value, ValueMatchExpression::EndsWith)(s)
            }
            ValueMatchExpressionDiscriminants::Regex => {
                map(regex_literal, |re| ValueMatchExpression::Regex(re.into()))(s)
            }
        }?;

        Ok((s, FilterExpression::FieldMatch(id, expr)))
    })(s)
}

fn filter_expression_implicit_and(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    let (s, start) = position(s)?;
    let (s, node1) = not_expression(s)?;
    let Ok((s, node2)) = not_expression(s) else {
        return Ok((s, node1));
    };
    let (s, end) = position(s)?;

    let and_exp = FilterExpression::And(Box::new(node1.expr.clone()), Box::new(node2.expr.clone()));
    let node = FilterExpressionParseNode::parent(
        and_exp,
        vec![node1, node2],
        start.location_offset(),
        end.location_offset(),
    );
    fold_many0(
        not_expression,
        move || node.clone(),
        |node1, node2| {
            let expr =
                FilterExpression::And(Box::new(node1.expr.clone()), Box::new(node2.expr.clone()));
            let (start, end) = (node1.start, node2.end);
            FilterExpressionParseNode::parent(expr, vec![node1, node2], start, end)
        },
    )(s)
}

fn or_expression(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    let (s, start) = position(s)?;
    match alt((
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
    ))(s)
    {
        Ok((s, (node1, node2))) => {
            let (s, end) = position(s)?;
            let expr =
                FilterExpression::Or(Box::new(node1.expr.clone()), Box::new(node2.expr.clone()));
            Ok((
                s,
                FilterExpressionParseNode::parent(
                    expr,
                    vec![node1, node2],
                    start.location_offset(),
                    end.location_offset(),
                ),
            ))
        }
        Err(_) => filter_expression_implicit_and(s),
    }
}

fn and_expression(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    let (s, start) = position(s)?;
    match alt((
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
    ))(s)
    {
        Ok((s, (node1, node2))) => {
            let (s, end) = position(s)?;
            let expr =
                FilterExpression::And(Box::new(node1.expr.clone()), Box::new(node2.expr.clone()));
            Ok((
                s,
                FilterExpressionParseNode::parent(
                    expr,
                    vec![node1, node2],
                    start.location_offset(),
                    end.location_offset(),
                ),
            ))
        }
        Err(_) => or_expression(s),
    }
}

pub fn filter_expression(s: Span) -> IResult<Span, FilterExpressionParseNode> {
    with_ws(and_expression)(s)
}

#[derive(Debug, Default, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub struct FilterExpressionParseNode {
    pub expr: FilterExpression,
    children: Vec<FilterExpressionParseNode>,
    pub start: usize,
    pub end: usize,
}

impl FilterExpressionParseNode {
    pub fn leaf(expr: FilterExpression, start: usize, end: usize) -> Self {
        Self {
            expr,
            children: vec![],
            start,
            end,
        }
    }

    pub fn parent(
        expr: FilterExpression,
        children: Vec<FilterExpressionParseNode>,
        start: usize,
        end: usize,
    ) -> Self {
        Self {
            expr,
            children,
            start,
            end,
        }
    }

    pub fn flat_nodes(&self) -> Vec<FilterExpressionParseNode> {
        let mut list = vec![self.clone()];
        for child in &self.children {
            list.extend(child.flat_nodes());
        }

        list
    }

    pub fn replacement_range(&self) -> Option<(usize, usize)> {
        // field:01234567-89ab-cdef-0123-456789abcdef
        const FIELD_MATCH_REPLACEMENT_LENGTH: usize = 42;

        match &self.expr {
            FilterExpression::TagMatch(_) => Some((self.start, self.end)),
            FilterExpression::FieldMatch(_, _) => {
                Some((self.start, self.start + FIELD_MATCH_REPLACEMENT_LENGTH))
            }
            _ => None,
        }
    }

    pub fn replacement_size(&self, ui: &egui::Ui, vault: &Vault) -> Option<egui::Vec2> {
        match &self.expr {
            FilterExpression::TagMatch(id) | FilterExpression::FieldMatch(id, _) => {
                let def = vault.get_definition_or_placeholder(id);
                Some(crate::ui::widgets::Tag::new(&def).small(true).size(ui))
            }
            _ => None,
        }
    }

    pub fn replacement_char(&self, ui: &egui::Ui, vault: &Vault) -> Option<char> {
        const PRIVATE_USE_AREA_START: u32 = 0xE000;
        const PRIVATE_USE_AREA_SIZE: u32 = 0x1000;

        let size = self.replacement_size(ui, vault)?;
        if size.x < 0.0 {
            return None;
        }

        #[allow(clippy::cast_sign_loss)]
        #[allow(clippy::cast_possible_truncation)]
        let x = (size.x as u32).min(PRIVATE_USE_AREA_SIZE - 1);
        // x is bounded to [0, 0x0FFF], thus argument is bounded to [0xE000, 0xEFFF]
        char::from_u32(PRIVATE_USE_AREA_START + x)
    }
}

#[derive(Debug, Default, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub struct FilterExpressionParseResult {
    pub expr: FilterExpression,
    nodes: Vec<FilterExpressionParseNode>,
    text: String,
    rest: usize,
}

#[derive(Debug)]
pub enum FilterExpressionTextSection {
    Normal(usize, usize),
    Replacement(usize, FilterExpressionParseNode),
}

impl FilterExpressionParseResult {
    pub fn sections(&self) -> Vec<FilterExpressionTextSection> {
        let mut sections = vec![FilterExpressionTextSection::Normal(0, self.text.len())];
        for node in &self.nodes {
            if let Some((repl_start, repl_end)) = node.replacement_range() {
                // assume that replacement ranges do not overlap, such that there is exactly
                // one normal range which encompasses the entire replacement range
                let match_pred = |sec: &FilterExpressionTextSection| matches!(sec, FilterExpressionTextSection::Normal(start, end) if *start <= repl_start && *end >= repl_end);
                let sec_idx = match sections.iter().positions(match_pred).collect::<Vec<_>>()[..] {
                    [] => {
                        warn!("Could not find match for range ({repl_start}, {repl_end}) in list: {sections:?}");
                        continue;
                    }
                    [idx] => idx,
                    _ => {
                        warn!("Found too many matches for range ({repl_start}, {repl_end}) in list: {sections:?}");
                        continue;
                    }
                };

                let sec = sections.swap_remove(sec_idx);
                let FilterExpressionTextSection::Normal(start, end) = sec else {
                    continue;
                };

                let node = node.clone();
                sections.extend(match (start == repl_start, end == repl_end) {
                    (true, true) => vec![FilterExpressionTextSection::Replacement(start, node)],
                    (true, false) => vec![
                        FilterExpressionTextSection::Replacement(start, node),
                        FilterExpressionTextSection::Normal(repl_end, end),
                    ],
                    (false, true) => vec![
                        FilterExpressionTextSection::Normal(start, repl_start),
                        FilterExpressionTextSection::Replacement(repl_start, node),
                    ],
                    (false, false) => vec![
                        FilterExpressionTextSection::Normal(start, repl_start),
                        FilterExpressionTextSection::Replacement(repl_start, node),
                        FilterExpressionTextSection::Normal(repl_end, end),
                    ],
                });
            }
        }
        sections.sort_by_key(|sec| match sec {
            FilterExpressionTextSection::Normal(start, _)
            | FilterExpressionTextSection::Replacement(start, _) => *start,
        });
        sections
    }

    pub fn tag_ids(&self) -> Vec<Uuid> {
        let mut results = vec![];
        for node in &self.nodes {
            if !node.children.is_empty() {
                continue;
            }

            match node.expr {
                FilterExpression::TagMatch(id) | FilterExpression::FieldMatch(id, _) => {
                    results.push(id);
                }
                _ => {}
            }
        }

        results
    }
}

#[allow(clippy::cast_possible_wrap)]
fn ccursor_add(cur: CCursor, diff: isize) -> CCursor {
    CCursor {
        index: usize::try_from((cur.index as isize) + diff)
            .ok()
            .unwrap_or(0),
        prefer_next_row: cur.prefer_next_row,
    }
}

#[allow(clippy::cast_possible_wrap)]
fn cursor_add(cur: Cursor, diff: isize) -> Cursor {
    Cursor {
        ccursor: ccursor_add(cur.ccursor, diff),
        // TODO: currently assuming one paragraph, one row
        rcursor: RCursor {
            row: cur.rcursor.row,
            column: usize::try_from((cur.rcursor.column as isize) + diff)
                .ok()
                .unwrap_or(0),
        },
        pcursor: PCursor {
            paragraph: cur.pcursor.paragraph,
            offset: usize::try_from((cur.pcursor.offset as isize) + diff)
                .ok()
                .unwrap_or(0),
            prefer_next_row: cur.pcursor.prefer_next_row,
        },
    }
}

#[allow(clippy::cast_possible_wrap)]
pub trait ReplacementStringConversion {
    fn replacement_idx_to_text_idx(&self, idx: usize) -> usize;
    fn replacement_ccursor_to_text_ccursor(&self, cur: CCursor) -> CCursor {
        let idx = cur.index;
        let new_idx = self.replacement_idx_to_text_idx(idx);
        ccursor_add(cur, (new_idx as isize) - (idx as isize))
    }
    fn replacement_cursor_to_text_cursor(&self, cur: Cursor) -> Cursor {
        let idx = cur.ccursor.index;
        let new_idx = self.replacement_idx_to_text_idx(idx);
        cursor_add(cur, (new_idx as isize) - (idx as isize))
    }
    fn replacement_ccursor_range_to_text_ccursor_range(&self, range: CCursorRange) -> CCursorRange {
        CCursorRange::two(
            self.replacement_ccursor_to_text_ccursor(range.primary),
            self.replacement_ccursor_to_text_ccursor(range.secondary),
        )
    }
    fn replacement_range_to_text_range(&self, range: CursorRange) -> CursorRange {
        CursorRange::two(
            self.replacement_cursor_to_text_cursor(range.primary),
            self.replacement_cursor_to_text_cursor(range.secondary),
        )
    }

    fn text_idx_to_replacement_idx(&self, idx: usize) -> usize;
    fn text_ccursor_to_replacement_ccursor(&self, cur: CCursor) -> CCursor {
        let idx = cur.index;
        let new_idx = self.text_idx_to_replacement_idx(idx);
        ccursor_add(cur, (new_idx as isize) - (idx as isize))
    }
    fn text_cursor_to_replacement_cursor(&self, cur: Cursor) -> Cursor {
        let idx = cur.ccursor.index;
        let new_idx = self.text_idx_to_replacement_idx(idx);
        cursor_add(cur, (new_idx as isize) - (idx as isize))
    }
    fn text_ccursor_range_to_replacement_ccursor_range(&self, range: CCursorRange) -> CCursorRange {
        CCursorRange::two(
            self.text_ccursor_to_replacement_ccursor(range.primary),
            self.text_ccursor_to_replacement_ccursor(range.secondary),
        )
    }
    fn text_range_to_replacement_range(&self, range: CursorRange) -> CursorRange {
        CursorRange::two(
            self.text_cursor_to_replacement_cursor(range.primary),
            self.text_cursor_to_replacement_cursor(range.secondary),
        )
    }
}

impl ReplacementStringConversion for FilterExpressionParseResult {
    fn replacement_idx_to_text_idx(&self, idx: usize) -> usize {
        //       0123456789012345678
        // text: abc[defgh]ijkl[mn]
        // repl: abc_ijkl_
        // repl_nodes: (3,10), (14,18)

        // repl_idx: 8
        // text_idx: 14 = 8 + ((10-3) - 1)
        // first node: (3, 10)
        // diff: 6
        // node.start (3) - acc_diff (0) = 3 < idx (8)
        // acc_diff := 0 + 6 = 6
        // second node: (14, 18)
        // diff: 4
        // node.start (14) - acc_diff (6) = 8
        let mut acc_diff = 0;
        for node in &self.nodes {
            if let Some((repl_start, repl_end)) = node.replacement_range() {
                let diff = (repl_end - repl_start) - 1;
                if repl_start - acc_diff < idx {
                    acc_diff += diff;
                }
            }
        }
        idx + acc_diff
    }

    fn text_idx_to_replacement_idx(&self, idx: usize) -> usize {
        //       0123456789012345678
        // text: abc[defgh]ijkl[mn]
        // repl: abc_ijkl_
        // repl_nodes: (3,10), (14,18)

        // text_idx: 16
        // repl_idx: 8 = 16 - (7 - 1) - (3 - 1)
        //             = 16 - ((10-3) - 1) - ((16-14) - 1)
        let mut idx_diff = 0;
        for node in &self.nodes {
            if let Some((repl_start, repl_end)) = node.replacement_range() {
                if idx >= repl_end {
                    idx_diff += (repl_end - repl_start) - 1;
                } else if idx > repl_start {
                    idx_diff += (idx - repl_start) - 1;
                }
            }
        }
        idx.saturating_sub(idx_diff)
    }
}

impl ReplacementStringConversion for Option<FilterExpressionParseResult> {
    fn replacement_idx_to_text_idx(&self, idx: usize) -> usize {
        if let Some(expr) = self.as_ref() {
            expr.replacement_idx_to_text_idx(idx)
        } else {
            idx
        }
    }

    fn text_idx_to_replacement_idx(&self, idx: usize) -> usize {
        if let Some(expr) = self.as_ref() {
            expr.text_idx_to_replacement_idx(idx)
        } else {
            idx
        }
    }
}

impl FromStr for FilterExpressionParseResult {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match filter_expression(LocatedSpan::new(s)) {
            Ok((rest, node)) => {
                let nodes = node.flat_nodes();
                Ok(FilterExpressionParseResult {
                    expr: node.expr.clone(),
                    text: s.to_string(),
                    nodes,
                    rest: rest.location_offset(),
                })
            }
            Err(_) => Err(()),
        }
    }
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

    fn assert_ok<T: PartialEq>(v: T, res: IResult<Span, T>) {
        assert!(matches!(res, Ok((_, val)) if val == v));
    }

    fn assert_ok_fe(v: FilterExpression, res: IResult<Span, FilterExpressionParseNode>) {
        assert!(matches!(res, Ok((_, val)) if val.expr == v));
    }

    fn s(s: &str) -> Span {
        Span::new(s)
    }

    #[test]
    fn test_filter_expression_and() {
        assert_ok_fe(
            and(exact("abc"), exact("def")),
            filter_expression(s("and(\"abc\", \"def\")")),
        );

        assert_ok_fe(
            and(text("abc"), text("def")),
            filter_expression(s("and(abc, def)")),
        );

        assert_ok_fe(
            and(exact("abc"), exact("def")),
            filter_expression(s("\"abc\" && \"def\"")),
        );

        assert_ok_fe(
            and(exact("abc"), exact("def")),
            filter_expression(s("\"abc\" and \"def\"")),
        );

        assert_ok_fe(
            and(text("abc"), text("def")),
            filter_expression(s("abc and def")),
        );
    }

    #[test]
    fn test_filter_expression_or() {
        assert_ok_fe(
            or(exact("abc"), exact("def")),
            filter_expression(s("or(\"abc\", \"def\")")),
        );

        assert_ok_fe(
            or(text("abc"), text("def")),
            filter_expression(s("or(abc, def)")),
        );

        assert_ok_fe(
            or(exact("abc"), exact("def")),
            filter_expression(s("\"abc\" || \"def\"")),
        );

        assert_ok_fe(
            or(exact("abc"), exact("def")),
            filter_expression(s("\"abc\" or \"def\"")),
        );

        assert_ok_fe(
            or(text("abc"), text("def")),
            filter_expression(s("abc or def")),
        );

        assert_ok_fe(text("abcordef"), filter_expression(s("abcordef")));
    }

    #[test]
    fn test_filter_expression_not() {
        assert_ok_fe(not(text("abc")), filter_expression(s("!abc")));

        assert_ok_fe(not(text("abc")), filter_expression(s("not abc")));

        assert_ok_fe(not(text("abc")), filter_expression(s("not ( abc )")));

        assert_ok_fe(text("notabc"), filter_expression(s("notabc")));
    }

    #[test]
    fn test_filter_expression_implicit_and() {
        assert_ok_fe(
            and(exact("abc"), exact("def")),
            filter_expression(s("\"abc\" \"def\"")),
        );
        assert_ok_fe(
            and(exact("abc"), text("def")),
            filter_expression(s("\"abc\" def")),
        );

        assert_ok_fe(
            and(text("abc"), text("def")),
            filter_expression(s("abc def")),
        );

        assert_ok_fe(
            and(or(text("abc"), text("def")), and(text("ghi"), text("jkl"))),
            filter_expression(s("abc or def and ghi jkl")),
        );

        assert_ok_fe(
            and(
                and(and(text("abc"), not(text("def"))), text("ghi")),
                exact("jkl"),
            ),
            filter_expression(s("abc !def ghi \"jkl\"")),
        );

        assert_ok_fe(
            and(
                or(and(text("abc"), text("def")), text("ghi")),
                and(text("jkl"), text("mno")),
            ),
            filter_expression(s("abc def or ghi and jkl mno")),
        );
    }

    #[test]
    fn test_field_value_bool() {
        assert_ok(V::boolean(true), field_value(s("true")));
        assert_ok(V::boolean(false), field_value(s("false")));
        assert_ok(V::boolean(true), field_value(s("yes")));
        assert_ok(V::boolean(false), field_value(s("no")));
        assert_ok(V::boolean(true), field_value(s("bool:true")));
        assert_ok(V::boolean(false), field_value(s("bool:false")));
    }

    #[test]
    fn test_field_value_int() {
        assert_ok(V::int(0), field_value(s("0")));
        assert_ok(V::int(12_943_128_349), field_value(s("12943128349")));
        assert_ok(V::int(-64321), field_value(s("-64321")));
        assert_ok(V::int(-12100), field_value(s("int:-12100")));
    }

    #[test]
    fn test_field_value_float() {
        assert_ok(V::float(0.0.into()), field_value(s("0.0")));
        assert_ok(
            V::float((-98713.12981e-50).into()),
            field_value(s("-98713.12981e-50")),
        );
        assert_ok(V::float(f64::INFINITY.into()), field_value(s("inf")));
        assert_ok(
            V::float(12908.14289e-220.into()),
            field_value(s("float:12908.14289e-220")),
        );
    }

    #[test]
    fn test_field_value_datetime() {
        with_tz(-7, || {
            assert_ok(
                V::datetime(Utc.timestamp_nanos(1_718_597_772_000_000_000)),
                field_value(s("2024-06-17T12:16:12+08:00")),
            );
            assert_ok(
                V::datetime(Utc.timestamp_nanos(1_718_597_772_000_000_000)),
                field_value(s("2024-06-17T04:16:12Z")),
            );
            assert_ok(
                V::datetime(Utc.timestamp_nanos(1_718_597_772_000_000_000)),
                field_value(s("2024-06-16T21:16:12")),
            );
            assert_ok(
                V::datetime(Utc.timestamp_nanos(1_718_607_600_000_000_000)),
                field_value(s("2024-06-17")),
            );
            assert_ok(
                V::datetime(Utc.timestamp_nanos(1_718_597_772_000_000_000)),
                field_value(s("datetime:2024-06-17T12:16:12+08:00")),
            );
        });
    }

    #[test]
    fn test_field_value_itemref() {
        assert_ok(
            V::itemref(("abc".into(), "def".into())),
            field_value(s("abc:def")),
        );
        assert_ok(
            V::itemref(("abc".into(), "def".into())),
            field_value(s("\"abc\":\"def\"")),
        );
        assert_ok(V::string("abc:def".into()), field_value(s("\"abc:def\"")));
        assert_ok(
            V::itemref(("abc".into(), "def".into())),
            field_value(s("itemref:abc:def")),
        );
    }

    #[test]
    fn test_field_value_colour() {
        assert_ok(V::colour([0, 119, 255].into()), field_value(s("#0077FF")));
        assert_ok(V::string("0077FF".into()), field_value(s("0077FF")));
        assert_ok(
            V::colour([0, 119, 255].into()),
            field_value(s("colour:0077FF")),
        );
        assert_ok(
            V::colour([0, 119, 255].into()),
            field_value(s("color:0077FF")),
        );
        assert_ok(V::colour([0, 119, 255].into()), field_value(s("#07F")));
        assert_ok(V::string("07F".into()), field_value(s("07F")));
        assert_ok(
            V::colour([0, 119, 255].into()),
            field_value(s("colour:07F")),
        );
        assert_ok(V::colour([0, 119, 255].into()), field_value(s("color:07F")));
    }

    #[test]
    fn test_field_value_list() {
        assert_ok(V::list(vec![]), field_value(s("[]")));
        assert_ok(
            V::list(vec![V::string("abc".into())]),
            field_value(s("[ abc ]")),
        );
        assert_ok(
            V::list(vec![V::string("abc".into()), V::string("def".into())]),
            field_value(s("abc,def")),
        );
        assert_ok(
            V::list(vec![
                V::float((-26.4).into()),
                V::int(15.into()),
                V::itemref(("abc".into(), "def".into())),
            ]),
            field_value(s("-26.4, 15, abc:def")),
        );
    }

    #[test]
    fn test_field_value_dictionary() {
        assert_ok(V::dictionary(vec![]), field_value(s("{}")));
        assert_ok(
            V::dictionary(vec![("abc".into(), V::string("def".into()))]),
            field_value(s("{abc:def}")),
        );
        assert_ok(
            V::dictionary(vec![("abc".into(), V::string("def".into()))]),
            field_value(s("{\"abc\": \"def\"}")),
        );
        assert_ok(
            V::dictionary(vec![
                ("abc".into(), V::string("def".into())),
                ("ghi".into(), V::int(32)),
            ]),
            field_value(s("{\"abc\": \"def\", \"ghi\": 32}")),
        );
        assert_ok(
            V::dictionary(vec![
                ("abc".into(), V::string("def".into())),
                ("ghi".into(), V::int(32)),
            ]),
            field_value(s("abc:def;ghi:32")),
        );
        assert_ok(
            V::dictionary(vec![
                ("abc".into(), V::string("def".into())),
                ("ghi".into(), V::int(32)),
            ]),
            field_value(s("abc:def,ghi:32")),
        );
    }

    #[test]
    fn test_field_match() {
        with_tz(8, || {
            let id1 = crate::fields::image::WIDTH.id;
            let id2 = crate::fields::meta::ALIASES.id;
            let id3 = crate::fields::general::MEDIA_TYPE.id;
            assert_ok_fe(
                eq(id1, V::int(600)),
                filter_expression(s(format!("field:{id1}=600").as_str())),
            );
            assert_ok_fe(
                ne(id1, V::float(549.2.into())),
                filter_expression(s(format!("field:{id1} ne 549.2").as_str())),
            );
            assert_ok_fe(
                is_one_of(id1, &[V::int(600), V::int(800)]),
                filter_expression(s(format!("field:{id1} in 600,800").as_str())),
            );
            assert_ok_fe(
                contains(id2, V::string("abc".into())),
                filter_expression(s(format!("field:{id2} contains abc").as_str())),
            );
            assert_ok_fe(
                lt(id1, V::int(-500)),
                filter_expression(s(format!("field:{id1}<-500").as_str())),
            );
            assert_ok_fe(
                le(id1, V::int(600)),
                filter_expression(s(format!("field:{id1}<= 600").as_str())),
            );
            assert_ok_fe(
                ge(
                    id1,
                    V::datetime(Utc.timestamp_nanos(1_718_553_600_000_000_000)),
                ),
                filter_expression(s(format!("field:{id1}>=2024-06-17").as_str())),
            );
            assert_ok_fe(
                gt(id1, V::string("ne".into())),
                filter_expression(s(format!("field:{id1}>ne").as_str())),
            );
            assert_ok_fe(
                starts(id3, V::string("image/".into())),
                filter_expression(s(format!("field:{id3}^=\"image/\"").as_str())),
            );
            assert_ok_fe(
                starts(id3, V::string("image/".into())),
                filter_expression(s(format!("field:{id3} starts with image/").as_str())),
            );
            assert_ok_fe(
                ends(id3, V::string("/jpeg".into())),
                filter_expression(s(format!("field:{id3}$=/jpeg").as_str())),
            );
            assert_ok_fe(
                ends(id3, V::string("/jpeg".into())),
                filter_expression(s(format!("field:{id3} ends with \"/jpeg\"").as_str())),
            );
            assert_ok_fe(
                regex(id3, r"[^/]+/[^/]+"),
                filter_expression(s(format!(r"field:{id3}=~/[^\/]+\/[^\/]+/").as_str())),
            );
            assert_ok_fe(
                regex(id1, r"6..\.."),
                filter_expression(s(format!(r"field:{id1} like /6..\../").as_str())),
            );
        });
    }
}
