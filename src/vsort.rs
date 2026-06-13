use std::cmp::Ordering;

/// Compare strings in version-fragment order.
///
/// Strings are split around runs matching `[0-9]+(?:\.[0-9]+)*`, like Ruby's
/// `split(/([0-9]+(?:\.[0-9]+)*)/)`.  Version fragments are compared as
/// dot-separated unsigned integer components, with the original fragment
/// string as a tie breaker; other fragments are compared as strings.
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let fa = fragments(a);
    let fb = fragments(b);
    for i in 0.. {
        match (fa.get(i), fb.get(i)) {
            (Some(x), Some(y)) => {
                let ord = match (x, y) {
                    (Fragment::Version(va), Fragment::Version(vb)) => cmp_versions(va, vb),
                    _ => x.raw().cmp(y.raw()),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => break,
        }
    }
    Ordering::Equal
}

#[derive(Debug)]
enum Fragment<'a> {
    Text(&'a str),
    Version(&'a str),
}

impl<'a> Fragment<'a> {
    fn raw(&self) -> &'a str {
        match self {
            Fragment::Text(s) | Fragment::Version(s) => s,
        }
    }
}

fn fragments(s: &str) -> Vec<Fragment<'_>> {
    let bytes = s.as_bytes();
    let mut frags = Vec::new();
    let mut pos = 0;
    let mut text_start = 0;
    while pos < bytes.len() {
        if !bytes[pos].is_ascii_digit() {
            pos += 1;
            continue;
        }
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        while pos + 1 < bytes.len() && bytes[pos] == b'.' && bytes[pos + 1].is_ascii_digit() {
            pos += 1;
            while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                pos += 1;
            }
        }
        if text_start < start {
            frags.push(Fragment::Text(&s[text_start..start]));
        }
        frags.push(Fragment::Version(&s[start..pos]));
        text_start = pos;
    }
    if text_start < s.len() {
        frags.push(Fragment::Text(&s[text_start..]));
    }
    frags
}

fn cmp_versions(a: &str, b: &str) -> Ordering {
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (Some(x), Some(y)) => {
                let ord = cmp_component(x, y);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return a.cmp(b),
        }
    }
}

/// Compare unsigned integer components of arbitrary length.
fn cmp_component(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lt(a: &str, b: &str) {
        assert_eq!(version_cmp(a, b), Ordering::Less, "{a:?} < {b:?}");
        assert_eq!(version_cmp(b, a), Ordering::Greater, "{b:?} > {a:?}");
    }

    #[test]
    fn numeric_order() {
        lt("29.9", "29.10");
        lt("1.2", "1.2.1");
        lt("v2", "v10");
        lt(
            "lib/python3.9/site-packages/",
            "lib/python3.10/site-packages/",
        );
    }

    #[test]
    fn text_order() {
        lt("abc", "abd");
        lt("a", "ab");
        assert_eq!(version_cmp("same", "same"), Ordering::Equal);
    }

    #[test]
    fn tie_breaker() {
        lt("1.02", "1.2");
        lt("a1.02b", "a1.2b");
        assert_eq!(version_cmp("1.2.3", "1.2.3"), Ordering::Equal);
    }

    #[test]
    fn mixed_fragments() {
        lt("1", "a");
        lt("9", "a");
        lt("10x", "10y");
    }
}
