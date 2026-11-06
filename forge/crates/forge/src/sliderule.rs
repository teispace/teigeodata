//! `SlideRule` response streams (elevation-reference note §3). A response is a sequence of
//! records: a big-endian header (`i16` version 2, `i16` type length, `i32` data length), a
//! NUL-terminated type name, then the data. `arrowrec.data` records carry the output file in
//! chunks after a 128-byte file name; `exceptrec` records carry server messages (`i32` code,
//! `u32` level, text from byte 8). A response with any error message is rejected, so a tile
//! with a failed granule or beam is never measured as if it were complete.

/// Message levels at or above this are errors (`SlideRule`: 3 = error, 4 = critical).
const ERROR_LEVEL: u32 = 3;
/// The file name field ahead of each data chunk.
const NAME_LEN: usize = 128;

/// A checked response: the output file, and the beams the server could not read because the
/// granule has no `land_segments` group for them (no land was measured there).
pub(crate) struct Response {
    pub(crate) file: Vec<u8>,
    pub(crate) empty_beams: Vec<(String, String)>,
}

/// The output file carried by a response stream. Any server error rejects the response, except
/// the pair `SlideRule` reports for a beam with no `land_segments`: those beams are returned so
/// the caller can confirm them ([`beam_absent`]) before trusting the file.
pub(crate) fn read(stream: &[u8]) -> Result<Response, String> {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut at = 0;
    while at < stream.len() {
        let field = |from: usize, n: usize| stream.get(from..from + n).ok_or("a truncated record");
        let head = field(at, 8)?;
        let num = |a: usize, b: usize| head.get(a..b).unwrap_or_default();
        let version = i16::from_be_bytes(num(0, 2).try_into().map_err(|_| "header")?);
        let type_len = u16::from_be_bytes(num(2, 4).try_into().map_err(|_| "header")?);
        let data_len = u32::from_be_bytes(num(4, 8).try_into().map_err(|_| "header")?);
        if version != 2 {
            return Err(format!("record version {version}, expected 2"));
        }
        let (type_len, data_len) = (usize::from(type_len), data_len as usize);
        let name = field(at + 8, type_len)?;
        let name = name.split(|&b| b == 0).next().unwrap_or_default();
        let data = field(at + 8 + type_len, data_len)?;
        match name {
            b"arrowrec.data" => out.extend_from_slice(data.get(NAME_LEN..).unwrap_or_default()),
            b"exceptrec" => {
                let word = |a: usize| {
                    data.get(a..a + 4)
                        .and_then(|w| w.try_into().ok())
                        .map(u32::from_le_bytes)
                };
                let (code, level) = (word(0).unwrap_or(0), word(4).unwrap_or(0));
                // The code is an `i32`; negative codes are failures.
                if level >= ERROR_LEVEL || code >= 0x8000_0000 {
                    let text = data.get(8..).unwrap_or_default();
                    let text = text.split(|&b| b == 0).next().unwrap_or_default();
                    errors.push(String::from_utf8_lossy(text).into_owned());
                }
            }
            _ => {}
        }
        at += 8 + type_len + data_len;
    }
    let empty_beams = classify(&errors)?;
    if out.is_empty() {
        return Err("the response carries no output file".into());
    }
    Ok(Response {
        file: out,
        empty_beams,
    })
}

/// The (granule, beam) pairs behind errors that only say a beam has no `land_segments`; any
/// other error is returned as the failure.
fn classify(errors: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut beams: Vec<(String, String)> = Vec::new();
    let mut unpaired = Vec::new();
    for e in errors {
        if let Some(rest) = e.strip_prefix("Failure on resource ")
            && let Some((granule, rest)) = rest.split_once(" beam ")
            && let Some((beam, rest)) = rest.split_once(": H5Coro::Future read failure on ")
            && rest.starts_with(&format!("{beam}/land_segments/"))
        {
            beams.push((granule.to_string(), beam.to_string()));
        } else {
            unpaired.push(e);
        }
    }
    // The follow-up message names the granule only; it is accepted with its beam failure.
    let other: Vec<&&String> = unpaired
        .iter()
        .filter(|e| {
            e.strip_prefix("Failed to send data in ")
                .and_then(|r| r.split_once(": invalid dataframe"))
                .is_none_or(|(g, _)| !beams.iter().any(|(bg, _)| bg == g))
        })
        .collect();
    if let Some(first) = other.first() {
        return Err(format!("{} server errors, the first: {first}", other.len()));
    }
    beams.sort();
    beams.dedup();
    Ok(beams)
}

/// Whether a granule truly has no `<beam>/land_segments` group, asked of the server's `h5p`
/// endpoint (`service` is the url up to `/source/`). An unreadable group that exists is a
/// transient failure, so this must return `Ok(true)` before a response is trusted.
pub(crate) fn beam_absent(
    service: &str,
    asset: &str,
    granule: &str,
    beam: &str,
) -> Result<bool, String> {
    let body = format!(
        r#"{{"asset":"{asset}","resource":"{granule}","datasets":[{{"dataset":"/{beam}/land_segments/latitude","datatype":0,"col":0,"startrow":0,"numrows":1}},{{"dataset":"/{beam}/land_segments/delta_time","datatype":0,"col":0,"startrow":0,"numrows":1}}]}}"#
    );
    let mut resp = ureq::post(&format!("{service}/source/h5p"))
        .header("User-Agent", crate::fetch::USER_AGENT)
        .header("Content-Type", "application/json")
        .send(&body)
        .map_err(|e| e.to_string())?;
    let bytes = resp.body_mut().read_to_vec().map_err(|e| e.to_string())?;
    // A dataset that exists comes back as an `h5file` record.
    Ok(!bytes.windows(7).any(|w| w == b"h5file\0"))
}

#[cfg(test)]
mod tests {
    use super::{classify, read};

    fn record(name: &str, data: &[u8]) -> Vec<u8> {
        let mut t = name.as_bytes().to_vec();
        t.push(0);
        let mut r = 2_i16.to_be_bytes().to_vec();
        r.extend(u16::try_from(t.len()).unwrap_or(0).to_be_bytes());
        r.extend(u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        r.extend(t);
        r.extend(data);
        r
    }

    fn message(level: u32, text: &str) -> Vec<u8> {
        let mut d = 0_u32.to_le_bytes().to_vec();
        d.extend(level.to_le_bytes());
        d.extend(text.as_bytes());
        d.push(0);
        record("exceptrec", &d)
    }

    #[test]
    fn extracts_chunks_and_rejects_errors() {
        let chunk = |b: &[u8]| {
            let mut d = vec![0_u8; 128];
            d.extend(b);
            record("arrowrec.data", &d)
        };
        let mut s = message(1, "retrieved 6 resources");
        s.extend(record("arrowrec.meta", &[0; 136]));
        s.extend(chunk(b"PAR1ab"));
        s.extend(chunk(b"cdPAR1"));
        assert_eq!(
            read(&s).ok().map(|r| r.file),
            Some(b"PAR1abcdPAR1".to_vec())
        );
        let mut bad = s.clone();
        bad.extend(message(4, "failed to create dataframe for beam gt1l"));
        assert!(read(&bad).is_err_and(|e| e.contains("gt1l")));
        assert!(
            read(s.get(..s.len() - 1).unwrap_or_default()).is_err(),
            "truncated"
        );
        assert!(read(&message(1, "x")).is_err(), "no file");
    }

    #[test]
    fn accepts_only_missing_land_segments() {
        let g = "ATL08_20230823205956_09882001_007_01.h5";
        let fail = |beam: &str, path: &str| {
            format!(
                "Failure on resource {g} beam {beam}: H5Coro::Future read failure on {beam}/{path}"
            )
        };
        let sent = format!("Failed to send data in {g}: invalid dataframe");
        let empty = [
            fail("gt3l", "land_segments/latitude"),
            sent.clone(),
            fail("gt3r", "land_segments/latitude"),
            sent.clone(),
        ];
        assert_eq!(
            classify(&empty).ok(),
            Some(vec![
                (g.to_string(), "gt3l".to_string()),
                (g.to_string(), "gt3r".to_string())
            ])
        );
        // Another dataset, or a follow-up with no beam failure, stays an error.
        assert!(classify(&[fail("gt1l", "signal_photons/x")]).is_err());
        assert!(classify(&[sent]).is_err());
        assert!(classify(&["timeout".to_string()]).is_err());
    }

    /// Needs the network: `cargo test --release -- --ignored beam_absent`.
    #[test]
    #[ignore = "queries the public SlideRule service"]
    fn beam_absent_asks_the_server() {
        let (service, g) = (
            "https://sliderule.slideruleearth.io",
            "ATL08_20230823205956_09882001_007_01.h5",
        );
        assert_eq!(
            super::beam_absent(service, "icesat2-atl08", g, "gt3r").ok(),
            Some(true)
        );
        assert_eq!(
            super::beam_absent(service, "icesat2-atl08", g, "gt1r").ok(),
            Some(false)
        );
    }
}
