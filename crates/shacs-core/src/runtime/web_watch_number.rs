use std::cmp::Ordering;

/// Separator characters a number on a page may use before a format is confirmed.
pub(super) const ALL_SEPARATORS: [char; 4] = ['.', ',', '\'', ' '];
const CURRENCY: &[char] = &[
    '$', '¢', '£', '¥', '฿', '₡', '₦', '₩', '₪', '₫', '€', '₭', '₮', '₱', '₲', '₴', '₵', '₸', '₹',
    '₺', '₼', '₽', '₾', '₿', '＄', '￠', '￡', '￥', '￦',
];
const MAX_UNIT_CHARS: usize = 8;
const MAX_DIGITS: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NumberFormat {
    pub decimal: char,
    pub thousands: Option<char>,
}

impl NumberFormat {
    pub fn separators(self) -> Vec<char> {
        std::iter::once(self.decimal)
            .chain(self.thousands)
            .collect()
    }
}

pub(super) fn is_unit_char(character: char) -> bool {
    character.is_alphabetic() || character == '%' || CURRENCY.contains(&character)
}

/// Byte spans of ASCII digit runs joined by `separators`; each run starts and ends with a digit.
pub(super) fn numeric_runs(text: &str, separators: &[char]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    let mut digit_end = 0;
    for (index, character) in text.char_indices() {
        if character.is_ascii_digit() {
            start.get_or_insert(index);
            digit_end = index + 1;
        } else if start.is_some() && separators.contains(&character) {
            continue;
        } else if let Some(begin) = start.take() {
            runs.push((begin, digit_end));
        }
    }
    if let Some(begin) = start {
        runs.push((begin, digit_end));
    }
    runs
}

/// Reads the format a page number appears to use. A lone `.` or `,` followed by
/// exactly three digits is ambiguous and follows the preferred decimal separator.
pub(super) fn infer_format(raw: &str, decimal_comma: bool) -> Option<NumberFormat> {
    let preferred = if decimal_comma { ',' } else { '.' };
    let other = |decimal: char| if decimal == ',' { '.' } else { ',' };
    let separators: Vec<(usize, char)> = raw
        .char_indices()
        .filter(|(_, character)| !character.is_ascii_digit())
        .collect();
    let mut distinct: Vec<char> = separators.iter().map(|(_, character)| *character).collect();
    distinct.sort_unstable();
    distinct.dedup();
    let format = match (distinct.as_slice(), separators.last()) {
        ([], _) => NumberFormat {
            decimal: preferred,
            thousands: Some(other(preferred)),
        },
        ([single], Some((position, _))) => {
            let digits_after = raw.len() - position - 1;
            let decimal = separators.len() == 1
                && matches!(single, '.' | ',')
                && (digits_after != 3 || *single == preferred);
            if decimal {
                NumberFormat {
                    decimal: *single,
                    thousands: Some(other(*single)),
                }
            } else {
                NumberFormat {
                    decimal: if matches!(single, '.' | ',') {
                        other(*single)
                    } else {
                        preferred
                    },
                    thousands: Some(*single),
                }
            }
        }
        ([_, _], Some((_, last))) if matches!(last, '.' | ',') => NumberFormat {
            decimal: *last,
            thousands: distinct.iter().copied().find(|character| character != last),
        },
        _ => return None,
    };
    parse_number(raw, format).map(|_| format)
}

/// Canonical non-negative decimal (`12900`, `12.5`) of `raw` under `format`.
pub(super) fn parse_number(raw: &str, format: NumberFormat) -> Option<String> {
    let (integer, fraction) = match raw.split_once(format.decimal) {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (raw, None),
    };
    let integer = match format.thousands {
        Some(separator) if integer.contains(separator) => {
            let groups: Vec<&str> = integer.split(separator).collect();
            let grouped = groups
                .first()
                .is_some_and(|group| (1..=3).contains(&group.len()))
                && groups.iter().skip(1).all(|group| group.len() == 3);
            if !grouped {
                return None;
            }
            groups.concat()
        }
        _ => integer.to_owned(),
    };
    let fraction = match fraction {
        Some(fraction) if !fraction.is_empty() => fraction,
        Some(_) => return None,
        None => "",
    };
    if integer.is_empty()
        || !integer
            .chars()
            .chain(fraction.chars())
            .all(|c| c.is_ascii_digit())
        || integer.len() + fraction.len() > MAX_DIGITS
    {
        return None;
    }
    let integer = match integer.trim_start_matches('0') {
        "" => "0",
        trimmed => trimmed,
    };
    Some(match fraction.trim_end_matches('0') {
        "" => integer.to_owned(),
        fraction => format!("{integer}.{fraction}"),
    })
}

/// Plain user input such as `10000` or `9.99` as a canonical decimal.
pub(super) fn parse_plain_decimal(input: &str) -> Option<String> {
    parse_number(
        input.trim(),
        NumberFormat {
            decimal: '.',
            thousands: None,
        },
    )
}

pub(super) fn compare_canonical(left: &str, right: &str) -> Ordering {
    let (left_integer, left_fraction) = left.split_once('.').unwrap_or((left, ""));
    let (right_integer, right_fraction) = right.split_once('.').unwrap_or((right, ""));
    let width = left_fraction.len().max(right_fraction.len());
    left_integer
        .len()
        .cmp(&right_integer.len())
        .then_with(|| left_integer.cmp(right_integer))
        .then_with(|| {
            format!("{left_fraction:0<width$}").cmp(&format!("{right_fraction:0<width$}"))
        })
}

/// Unit text directly attached before (`₩12,900`) or after (`12,900원`) a number.
pub(super) fn adjacent_unit(text: &str, before: bool) -> &str {
    let mut count = 0;
    let mut boundary = if before { text.len() } else { 0 };
    let mut take = |character: char, index: usize| {
        if count == MAX_UNIT_CHARS || !is_unit_char(character) {
            return false;
        }
        count += 1;
        boundary = if before {
            index
        } else {
            index + character.len_utf8()
        };
        true
    };
    if before {
        for (index, character) in text.char_indices().rev() {
            if !take(character, index) {
                break;
            }
        }
        &text[boundary..]
    } else {
        for (index, character) in text.char_indices() {
            if !take(character, index) {
                break;
            }
        }
        &text[..boundary]
    }
}

#[cfg(test)]
mod tests {
    use super::{compare_canonical, infer_format, parse_number, NumberFormat};
    use std::cmp::Ordering;

    #[test]
    fn inferred_formats_follow_the_preferred_decimal_for_three_digit_groups() {
        let cases = [
            ("12,900", false, Some((('.', Some(',')), "12900"))),
            ("1.234", false, Some((('.', Some(',')), "1.234"))),
            ("1.234", true, Some(((',', Some('.')), "1234"))),
            ("1 234,50", true, Some(((',', Some(' ')), "1234.5"))),
            ("1,234.00", false, Some((('.', Some(',')), "1234"))),
            ("2 12,900", false, None),
        ];
        for (raw, decimal_comma, expected) in cases {
            let format = infer_format(raw, decimal_comma);
            let actual = format.map(|format| {
                (
                    (format.decimal, format.thousands),
                    parse_number(raw, format).unwrap_or_default(),
                )
            });
            let expected = expected.map(|(format, value)| (format, value.to_owned()));
            assert_eq!(actual, expected, "{raw} decimal_comma={decimal_comma}");
        }
    }

    #[test]
    fn confirmed_format_rejects_other_separators_and_bad_groups() {
        let format = NumberFormat {
            decimal: '.',
            thousands: Some(','),
        };
        assert_eq!(parse_number("12.900,5", format), None);
        assert_eq!(parse_number("12,34", format), None);
        assert_eq!(parse_number("0012.50", format).as_deref(), Some("12.5"));
        assert_eq!(compare_canonical("9.99", "10"), Ordering::Less);
        assert_eq!(compare_canonical("12900", "12900.0"), Ordering::Equal);
    }
}
