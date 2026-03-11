use core::cmp::Ordering;
use core::iter;

/// Generate a midpoint between two byte strings that can be used to sort an
/// element between two others.
///
/// If possible, this function will try to avoid growing the outgoing string if
/// a substitution is sufficient.
///
/// Of `lo` and `hi` are equal, a midpoint does not exist and the original
/// string is returned.
pub fn midpoint(lo: &[u8], hi: &[u8]) -> Vec<u8> {
    let (lo, hi) = match lo.cmp(hi) {
        Ordering::Less => (lo, hi),
        Ordering::Greater => (hi, lo),
        Ordering::Equal => return lo.to_vec(),
    };

    let len = lo.len().max(hi.len());
    midpoint_impl(len, lo, &mut hi.iter().copied())
}

/// Generate a byte string that sorts before the given string and tries to avoid
/// growing the string if possible.
///
/// The string is generated as the conceptual midpoint between the first sorted
/// string (b"") and the specifie string.
pub fn before(s: &[u8]) -> Vec<u8> {
    midpoint_impl(s.len(), b"", &mut s.iter().copied())
}

/// Generate a byte string that sorts after the given string and tries to avoid
/// growing the string if possible.
///
/// The string is generated as the conceptual midpoint between the last sorted
/// string (infinite \xff).
pub fn after(s: &[u8]) -> Vec<u8> {
    midpoint_impl(s.len(), s, &mut iter::repeat(u8::MAX))
}

fn midpoint_impl(len: usize, lo: &[u8], hi: &mut dyn Iterator<Item = u8>) -> Vec<u8> {
    // Try to find a position where lo and hi differ by at least 2.
    for i in 0..len {
        let a = lo.get(i).copied().unwrap_or(0) as u16;
        let b = hi.next().unwrap_or(u8::MAX) as u16;

        if a + 1 >= b {
            continue;
        }

        let extended = i + 1 < lo.len();
        let cap = if extended { lo.len() } else { i + 1 };

        let mut mid = Vec::with_capacity(cap);

        // We can increment a at this position to get a value strictly between lo and hi.
        if let Some(lo) = lo.get(..i) {
            mid.extend_from_slice(lo);
        } else {
            mid.extend(iter::repeat_n(0, i));
        }

        mid.push(((a + b) / 2) as u8);

        // Fill remaining bytes if needed to match lo's length.
        if extended {
            mid.extend_from_slice(lo.get(i + 1..).unwrap_or_default());
        }

        return mid;
    }

    // If no substitution exists, grow the low string.
    let mut mid = Vec::with_capacity(lo.len().saturating_add(1));
    mid.extend_from_slice(lo);
    mid.push(u8::MAX / 2);
    mid
}

#[cfg(test)]
mod tests {
    use super::*;

    use bstr::BStr;

    #[test]
    fn test_midpoint() {
        macro_rules! test_case {
            ($lo:literal, $op:tt, $after:tt, $hi:literal, $expected:literal) => {{
                assert!(&$lo[..] $op &$hi[..], "Expected {:?} {} {:?}", BStr::new($lo), stringify!($op), BStr::new($hi));

                let outcome = midpoint($lo, $hi);
                assert_eq!(outcome, $expected.to_vec(), "Expected midpoint of {:?} and {:?} to be {:?}, got {:?}", BStr::new($lo), BStr::new($hi), BStr::new($expected), BStr::new(&outcome));

                assert!(outcome $after $lo.to_vec(), "Expected {:?} {} {:?}", BStr::new(&outcome), stringify!($after), BStr::new($lo));
                assert!(outcome $op $hi.to_vec(), "Expected {:?} {} {:?}", BStr::new(&outcome), stringify!($op), BStr::new($hi));
            }};
        }

        test_case!(b"\xff", <, >, b"\xff\xff", b"\xff\x7f");
        test_case!(b"abc", <, >, b"abd", b"abc\x7f");
        test_case!(b"abc", <, >, b"abf", b"abd");
        test_case!(b"abc", <, >, b"abz", b"abn");
        test_case!(b"abc", <, >, b"ac", b"ab\xb1");
        test_case!(b"abc", >, <, b"", b"0");
        test_case!(b"", <, >, b"abc", b"0");
        test_case!(b"abc", ==, ==, b"abc", b"abc");
    }

    #[test]
    fn test_after() {
        macro_rules! test_case {
            ($input:literal, $expected:literal) => {{
                let outcome = after($input);

                assert_eq!(
                    outcome,
                    $expected.to_vec(),
                    "Expected after {:?} to be {:?}, got {:?}",
                    BStr::new($input),
                    BStr::new($expected),
                    BStr::new(&outcome)
                );

                assert!(
                    outcome > $input.to_vec(),
                    "Expected {:?} > {:?}",
                    BStr::new(&outcome),
                    BStr::new($input)
                );
            }};
        }

        test_case!(b"abc", b"\xb0bc");
        test_case!(b"\xff", b"\xff\x7f");
        test_case!(b"Owlbear10", b"\xa7wlbear10");
    }
}
