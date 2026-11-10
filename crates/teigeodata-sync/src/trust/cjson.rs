//! Canonical JSON as TUF signs it: the OLPC form that `securesystemslib` (python-tuf) and
//! `olpc-cjson` (`tough`) write. Object keys sorted by their UTF-8 bytes, no whitespace, strings
//! with only `\` and `"` escaped and every other character written as itself, integers only.

use alloc::vec::Vec;

use serde_json::Value;

use super::Error;

/// The canonical bytes of `v`.
///
/// # Errors
/// [`Error::Format`] for a number that is not an integer: canonical JSON has no floats.
pub fn encode(v: &Value) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    write(v, &mut out)?;
    Ok(out)
}

fn write(v: &Value, out: &mut Vec<u8>) -> Result<(), Error> {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => {
            let text = if let Some(i) = n.as_i64() {
                alloc::format!("{i}")
            } else if let Some(u) = n.as_u64() {
                alloc::format!("{u}")
            } else {
                return Err(Error::Format("a number that is not an integer"));
            };
            out.extend_from_slice(text.as_bytes());
        }
        Value::String(s) => string(s, out),
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            // A `serde_json` map is a `BTreeMap` without `preserve_order`: sorted by key bytes,
            // which for UTF-8 is code point order, as Python sorts.
            out.push(b'{');
            for (i, (k, item)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                string(k, out);
                out.push(b':');
                write(item, out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

fn string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for &b in s.as_bytes() {
        if b == b'"' || b == b'\\' {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b'"');
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::encode;

    #[test]
    fn sorted_compact_and_minimally_escaped() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{ "b": [1, -2, true, null], "a": "q\"uo\\te\né", "A": {} }"#)
                .unwrap();
        // securesystemslib.formats.encode_canonical gives exactly these bytes: the newline and
        // the é stay raw.
        assert_eq!(
            encode(&v).unwrap(),
            "{\"A\":{},\"a\":\"q\\\"uo\\\\te\n\u{e9}\",\"b\":[1,-2,true,null]}".as_bytes()
        );
    }

    #[test]
    fn floats_are_refused() {
        let v: serde_json::Value = serde_json::from_str(r#"{"x": 1.5}"#).unwrap();
        assert!(encode(&v).is_err());
    }
}
