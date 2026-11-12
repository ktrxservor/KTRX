//! Content-Encoding decoding (`gzip`, `x-gzip`, `deflate`), bounded so a
//! small compressed body cannot expand without limit.

use std::io::Read;

/// The codings this client can decode, as sent in `Accept-Encoding`.
pub(crate) const ACCEPT_ENCODING: &str = "gzip, deflate";

/// Decode `body` according to a `Content-Encoding` value.
///
/// Returns `Ok(None)` when there is nothing to decode (`identity`, empty).
/// Otherwise returns the decoded bytes, cut to `limit`, and whether they were
/// cut (or the input itself was incomplete). An unknown coding or corrupt data
/// is an error; the caller then keeps the raw body.
pub(crate) fn content(
    body: &[u8],
    encoding: &str,
    limit: usize,
    input_truncated: bool,
) -> Result<Option<(Vec<u8>, bool)>, String> {
    let codings: Vec<String> = encoding
        .split(',')
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty() && c != "identity")
        .collect();
    if codings.is_empty() {
        return Ok(None);
    }
    let mut data = body.to_vec();
    let mut truncated = input_truncated;
    // Codings are listed in the order they were applied; undo them in reverse.
    for coding in codings.iter().rev() {
        let (decoded, cut) = match coding.as_str() {
            "gzip" | "x-gzip" => read_limited(
                flate2::read::MultiGzDecoder::new(&data[..]),
                limit,
                truncated,
            )?,
            "deflate" => {
                // RFC 9110 deflate is zlib-wrapped, but some servers send raw
                // deflate; try both.
                match read_limited(flate2::read::ZlibDecoder::new(&data[..]), limit, truncated) {
                    Ok(result) => result,
                    Err(_) => read_limited(
                        flate2::read::DeflateDecoder::new(&data[..]),
                        limit,
                        truncated,
                    )?,
                }
            }
            other => return Err(format!("unsupported content coding '{other}'")),
        };
        data = decoded;
        truncated |= cut;
    }
    Ok(Some((data, truncated)))
}

/// Read everything `reader` produces, keeping at most `limit` bytes. When the
/// input was known to be incomplete, the bytes decoded before the error are
/// kept instead of failing.
fn read_limited(
    mut reader: impl Read,
    limit: usize,
    input_truncated: bool,
) -> Result<(Vec<u8>, bool), String> {
    let mut out = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok((out, false)),
            Ok(count) => {
                let room = limit - out.len();
                if count > room {
                    out.extend_from_slice(&buffer[..room]);
                    return Ok((out, true));
                }
                out.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) if input_truncated && !out.is_empty() => return Ok((out, true)),
            Err(error) => return Err(format!("cannot decode the body: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::content;
    use std::io::Write;

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn decodes_gzip_and_both_deflate_forms() {
        let text = b"hello hello hello hello".to_vec();
        assert_eq!(
            content(&gzip(&text), "gzip", 1024, false).unwrap(),
            Some((text.clone(), false))
        );
        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zlib.write_all(&text).unwrap();
        assert_eq!(
            content(&zlib.finish().unwrap(), "deflate", 1024, false).unwrap(),
            Some((text.clone(), false))
        );
        let mut raw =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        raw.write_all(&text).unwrap();
        assert_eq!(
            content(&raw.finish().unwrap(), "Deflate", 1024, false).unwrap(),
            Some((text, false))
        );
    }

    #[test]
    fn undoes_stacked_codings_in_reverse_order() {
        let twice = gzip(&gzip(b"layers"));
        assert_eq!(
            content(&twice, "gzip, x-gzip", 1024, false).unwrap(),
            Some((b"layers".to_vec(), false))
        );
        assert_eq!(content(b"plain", "identity", 1024, false).unwrap(), None);
    }

    #[test]
    fn bounds_the_decoded_size() {
        let bomb = gzip(&vec![0u8; 1_000_000]);
        let (decoded, cut) = content(&bomb, "gzip", 4096, false).unwrap().unwrap();
        assert_eq!(decoded.len(), 4096);
        assert!(cut);
    }

    #[test]
    fn reports_unknown_and_corrupt_bodies() {
        assert!(content(b"x", "br", 1024, false).is_err());
        assert!(content(b"not gzip at all", "gzip", 1024, false).is_err());
        // A truncated download keeps what could be decoded.
        let data: Vec<u8> = (0..50_000u32).flat_map(|i| i.to_le_bytes()).collect();
        let full = gzip(&data);
        let (partial, cut) = content(&full[..full.len() / 2], "gzip", usize::MAX, true)
            .unwrap()
            .unwrap();
        assert!(cut && !partial.is_empty() && partial.len() < data.len());
        assert_eq!(&data[..partial.len()], &partial[..]);
    }
}
