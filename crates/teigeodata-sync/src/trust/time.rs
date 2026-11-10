//! TUF's `expires`: `YYYY-MM-DDTHH:MM:SSZ` (UTC) as Unix seconds, with no date library.

use super::Error;

/// Unix seconds of `YYYY-MM-DDTHH:MM:SSZ`.
///
/// # Errors
/// [`Error::Format`] for any other form or an impossible date or time.
pub(super) fn parse(s: &str) -> Result<i64, Error> {
    let bad = Error::Format("expires is not YYYY-MM-DDTHH:MM:SSZ");
    let &[
        y0,
        y1,
        y2,
        y3,
        b'-',
        mo0,
        mo1,
        b'-',
        d0,
        d1,
        b'T',
        h0,
        h1,
        b':',
        mi0,
        mi1,
        b':',
        s0,
        s1,
        b'Z',
    ] = s.as_bytes()
    else {
        return Err(bad);
    };
    let num = |digits: &[u8]| -> Result<i64, Error> {
        digits.iter().try_fold(0_i64, |n, &c| {
            if c.is_ascii_digit() {
                Ok(n * 10 + i64::from(c - b'0'))
            } else {
                Err(bad)
            }
        })
    };
    let (year, month, day) = (num(&[y0, y1, y2, y3])?, num(&[mo0, mo1])?, num(&[d0, d1])?);
    let (hour, minute, second) = (num(&[h0, h1])?, num(&[mi0, mi1])?, num(&[s0, s1])?);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(bad),
    };
    if day < 1 || day > month_days || hour > 23 || minute > 59 || second > 59 {
        return Err(bad);
    }
    Ok(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::parse;

    #[test]
    fn known_instants() {
        assert_eq!(parse("1970-01-01T00:00:00Z").unwrap(), 0);
        assert_eq!(parse("2000-02-29T12:00:00Z").unwrap(), 951_825_600);
        assert_eq!(parse("2038-01-19T03:14:08Z").unwrap(), 2_147_483_648);
        assert_eq!(parse("2026-11-10T00:00:00Z").unwrap(), 1_794_268_800);
    }

    #[test]
    fn other_forms_are_refused() {
        for s in [
            "2026-11-10T00:00:00",
            "2026-11-10T00:00:00.5Z",
            "2026-11-10 00:00:00Z",
            "2026-02-29T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-11-10T24:00:00Z",
            "+026-11-10T00:00:00Z",
        ] {
            assert!(parse(s).is_err(), "{s}");
        }
    }
}
