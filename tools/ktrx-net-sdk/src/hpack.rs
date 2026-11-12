//! HPACK header compression for HTTP/2 (RFC 7541).
//!
//! [`Decoder`] implements the full decoding side: static and dynamic tables,
//! table size updates, every literal representation, and Huffman-coded
//! strings, with bounds on the decoded header list. [`Encoder`] is stateless:
//! it never adds entries to the peer's dynamic table, which keeps it simple
//! and immune to table-state bugs while still using the static table and
//! Huffman coding to stay compact. Sensitive fields (`authorization`,
//! `cookie`, `proxy-authorization`, `set-cookie`) are sent as never-indexed
//! literals so intermediaries do not compress them either.

use crate::huffman_table::ENCODE_TABLE;
use std::{collections::VecDeque, fmt, sync::OnceLock};

/// RFC 7541 Appendix A.
pub const STATIC_TABLE: [(&str, &str); 61] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
    ("age", ""),
    ("allow", ""),
    ("authorization", ""),
    ("cache-control", ""),
    ("content-disposition", ""),
    ("content-encoding", ""),
    ("content-language", ""),
    ("content-length", ""),
    ("content-location", ""),
    ("content-range", ""),
    ("content-type", ""),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("expect", ""),
    ("expires", ""),
    ("from", ""),
    ("host", ""),
    ("if-match", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("if-range", ""),
    ("if-unmodified-since", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("max-forwards", ""),
    ("proxy-authenticate", ""),
    ("proxy-authorization", ""),
    ("range", ""),
    ("referer", ""),
    ("refresh", ""),
    ("retry-after", ""),
    ("server", ""),
    ("set-cookie", ""),
    ("strict-transport-security", ""),
    ("transfer-encoding", ""),
    ("user-agent", ""),
    ("vary", ""),
    ("via", ""),
    ("www-authenticate", ""),
];

/// The dynamic table size both sides start with (`SETTINGS_HEADER_TABLE_SIZE`).
pub const DEFAULT_TABLE_SIZE: usize = 4_096;

/// Per-entry overhead counted by RFC 7541 §4.1.
const ENTRY_OVERHEAD: usize = 32;

/// Why a header block could not be decoded. Any HPACK error is a connection
/// error (`COMPRESSION_ERROR`) in HTTP/2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HpackError(pub String);

impl fmt::Display for HpackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HPACK: {}", self.0)
    }
}

impl std::error::Error for HpackError {}

fn error<T>(message: impl Into<String>) -> Result<T, HpackError> {
    Err(HpackError(message.into()))
}

/// A decoded header field. Names and values are kept as bytes because HTTP
/// allows values that are not UTF-8.
pub type Field = (Vec<u8>, Vec<u8>);

/// Decoding context for one HTTP/2 connection (one per direction).
pub struct Decoder {
    /// Newest entry first.
    table: VecDeque<Field>,
    size: usize,
    /// Current maximum, changed by size updates in header blocks.
    max_size: usize,
    /// Upper bound for size updates: the `SETTINGS_HEADER_TABLE_SIZE` we
    /// advertised.
    limit: usize,
    /// Largest decoded header list accepted, counted like
    /// `SETTINGS_MAX_HEADER_LIST_SIZE` (name + value + 32 per field).
    max_list_size: usize,
}

impl Decoder {
    /// A decoder whose table may grow to `limit` octets and that refuses
    /// header lists larger than `max_list_size`.
    pub fn new(limit: usize, max_list_size: usize) -> Self {
        Self {
            table: VecDeque::new(),
            size: 0,
            max_size: limit,
            limit,
            max_list_size,
        }
    }

    /// Current dynamic table size in octets.
    pub fn table_size(&self) -> usize {
        self.size
    }

    /// Decode one complete header block (HEADERS plus its CONTINUATIONs).
    pub fn decode(&mut self, block: &[u8]) -> Result<Vec<Field>, HpackError> {
        let mut fields = Vec::new();
        let mut list_size = 0usize;
        let mut position = 0;
        let mut fields_started = false;
        while position < block.len() {
            let first = block[position];
            let field = if first & 0x80 != 0 {
                // Indexed header field (§6.1).
                let index = decode_integer(block, &mut position, 7)?;
                self.get(index)?
            } else if first & 0x40 != 0 {
                // Literal with incremental indexing (§6.2.1).
                let field = self.literal(block, &mut position, 6)?;
                self.insert(field.clone());
                field
            } else if first & 0x20 != 0 {
                // Dynamic table size update (§6.3): only before any field.
                if fields_started {
                    return error("table size update after a header field");
                }
                let size = decode_integer(block, &mut position, 5)?;
                if size > self.limit {
                    return error(format!(
                        "table size update to {size} exceeds the advertised {}",
                        self.limit
                    ));
                }
                self.max_size = size;
                self.evict(0);
                continue;
            } else {
                // Literal without indexing (0000) or never indexed (0001).
                self.literal(block, &mut position, 4)?
            };
            fields_started = true;
            list_size = list_size.saturating_add(field.0.len() + field.1.len() + ENTRY_OVERHEAD);
            if list_size > self.max_list_size {
                return error(format!("header list exceeds {} octets", self.max_list_size));
            }
            fields.push(field);
        }
        Ok(fields)
    }

    fn literal(&self, block: &[u8], position: &mut usize, prefix: u8) -> Result<Field, HpackError> {
        let index = decode_integer(block, position, prefix)?;
        let name = if index == 0 {
            decode_string(block, position)?
        } else {
            self.get(index)?.0
        };
        let value = decode_string(block, position)?;
        Ok((name, value))
    }

    fn get(&self, index: usize) -> Result<Field, HpackError> {
        match index {
            0 => error("index 0 is not valid"),
            1..=61 => {
                let (name, value) = STATIC_TABLE[index - 1];
                Ok((name.as_bytes().to_vec(), value.as_bytes().to_vec()))
            }
            _ => self
                .table
                .get(index - 62)
                .cloned()
                .ok_or_else(|| HpackError(format!("index {index} is outside the tables"))),
        }
    }

    fn insert(&mut self, field: Field) {
        let size = field.0.len() + field.1.len() + ENTRY_OVERHEAD;
        if size > self.max_size {
            // An entry larger than the table empties it (§4.4).
            self.table.clear();
            self.size = 0;
            return;
        }
        self.evict(size);
        self.size += size;
        self.table.push_front(field);
    }

    /// Drop the oldest entries until `incoming` more octets fit.
    fn evict(&mut self, incoming: usize) {
        while self.size + incoming > self.max_size {
            match self.table.pop_back() {
                Some((name, value)) => self.size -= name.len() + value.len() + ENTRY_OVERHEAD,
                None => break,
            }
        }
    }
}

/// Stateless HPACK encoder; see the module documentation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Encoder;

impl Encoder {
    /// Append the header block for `fields` to `out`. Names must already be
    /// lower case, as HTTP/2 requires.
    pub fn encode<'a>(
        &self,
        fields: impl IntoIterator<Item = (&'a str, &'a [u8])>,
        out: &mut Vec<u8>,
    ) {
        for (name, value) in fields {
            let mut name_index = 0;
            let mut full_match = 0;
            for (i, (static_name, static_value)) in STATIC_TABLE.iter().enumerate() {
                if *static_name == name {
                    if name_index == 0 {
                        name_index = i + 1;
                    }
                    if static_value.as_bytes() == value {
                        full_match = i + 1;
                        break;
                    }
                }
            }
            let sensitive = matches!(
                name,
                "authorization" | "cookie" | "proxy-authorization" | "set-cookie"
            );
            if full_match != 0 && !sensitive {
                encode_integer(full_match, 7, 0x80, out);
                continue;
            }
            // Never indexed (0001) for secrets, otherwise without indexing (0000).
            let flags = if sensitive { 0x10 } else { 0x00 };
            encode_integer(name_index, 4, flags, out);
            if name_index == 0 {
                encode_string(name.as_bytes(), out);
            }
            encode_string(value, out);
        }
    }
}

/// RFC 7541 §5.1 integer with an `prefix`-bit prefix.
fn decode_integer(block: &[u8], position: &mut usize, prefix: u8) -> Result<usize, HpackError> {
    let mask = (1usize << prefix) - 1;
    let first = *block
        .get(*position)
        .ok_or(HpackError("truncated integer".into()))?;
    *position += 1;
    let mut value = usize::from(first) & mask;
    if value < mask {
        return Ok(value);
    }
    let mut shift = 0u32;
    loop {
        let byte = *block
            .get(*position)
            .ok_or(HpackError("truncated integer".into()))?;
        *position += 1;
        if shift > 28 {
            return error("integer is too large");
        }
        value = value
            .checked_add(usize::from(byte & 0x7f) << shift)
            .ok_or(HpackError("integer is too large".into()))?;
        shift += 7;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
}

fn encode_integer(value: usize, prefix: u8, flags: u8, out: &mut Vec<u8>) {
    let mask = (1usize << prefix) - 1;
    if value < mask {
        out.push(flags | value as u8);
        return;
    }
    out.push(flags | mask as u8);
    let mut rest = value - mask;
    while rest >= 0x80 {
        out.push((rest as u8 & 0x7f) | 0x80);
        rest >>= 7;
    }
    out.push(rest as u8);
}

/// Longest string literal accepted, well above any sane header.
const MAX_STRING: usize = 1 << 20;

fn decode_string(block: &[u8], position: &mut usize) -> Result<Vec<u8>, HpackError> {
    let huffman = block
        .get(*position)
        .ok_or(HpackError("truncated string".into()))?
        & 0x80
        != 0;
    let length = decode_integer(block, position, 7)?;
    if length > MAX_STRING {
        return error("string literal is too long");
    }
    let end = position
        .checked_add(length)
        .filter(|end| *end <= block.len())
        .ok_or(HpackError("truncated string".into()))?;
    let raw = &block[*position..end];
    *position = end;
    if huffman {
        huffman_decode(raw)
    } else {
        Ok(raw.to_vec())
    }
}

fn encode_string(value: &[u8], out: &mut Vec<u8>) {
    let huffman_len = value
        .iter()
        .map(|b| usize::from(ENCODE_TABLE[usize::from(*b)].0))
        .sum::<usize>()
        .div_ceil(8);
    if huffman_len < value.len() {
        encode_integer(huffman_len, 7, 0x80, out);
        huffman_encode(value, out);
    } else {
        encode_integer(value.len(), 7, 0x00, out);
        out.extend_from_slice(value);
    }
}

/// Huffman-encode `value`, padding the last octet with the EOS prefix (ones).
pub fn huffman_encode(value: &[u8], out: &mut Vec<u8>) {
    let mut bits: u64 = 0;
    let mut count: u32 = 0;
    for byte in value {
        let (length, code) = ENCODE_TABLE[usize::from(*byte)];
        bits = (bits << length) | u64::from(code);
        count += u32::from(length);
        while count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
        bits &= (1u64 << count) - 1;
    }
    if count > 0 {
        out.push(((bits << (8 - count)) | ((1u64 << (8 - count)) - 1)) as u8);
    }
}

/// A binary decoding tree: `nodes[i]` holds the two children of node `i`,
/// either another node (`< LEAF`) or `LEAF + symbol`.
struct Tree {
    nodes: Vec<[u16; 2]>,
}

const LEAF: u16 = 0x8000;
const EMPTY: u16 = u16::MAX;

fn tree() -> &'static Tree {
    static TREE: OnceLock<Tree> = OnceLock::new();
    TREE.get_or_init(|| {
        let mut nodes = vec![[EMPTY, EMPTY]];
        for (symbol, (length, code)) in ENCODE_TABLE.iter().enumerate() {
            let mut node = 0usize;
            for bit in (0..*length).rev() {
                let side = ((code >> bit) & 1) as usize;
                if bit == 0 {
                    nodes[node][side] = LEAF + symbol as u16;
                } else {
                    if nodes[node][side] == EMPTY {
                        nodes.push([EMPTY, EMPTY]);
                        nodes[node][side] = (nodes.len() - 1) as u16;
                    }
                    node = usize::from(nodes[node][side]);
                }
            }
        }
        Tree { nodes }
    })
}

/// Decode a Huffman-coded string, rejecting EOS and invalid padding (§5.2).
pub fn huffman_decode(encoded: &[u8]) -> Result<Vec<u8>, HpackError> {
    let tree = tree();
    let mut out = Vec::with_capacity(encoded.len() * 8 / 5);
    let mut node = 0usize;
    // Bits consumed since the last complete symbol, and whether all were ones.
    let mut pending_bits = 0u32;
    let mut pending_ones = true;
    for byte in encoded {
        for bit in (0..8).rev() {
            let side = usize::from((byte >> bit) & 1);
            pending_bits += 1;
            pending_ones &= side == 1;
            let next = tree.nodes[node][side];
            if next == EMPTY {
                return error("invalid Huffman code");
            }
            if next & LEAF != 0 {
                let symbol = next - LEAF;
                if symbol == 256 {
                    return error("Huffman-coded string contains EOS");
                }
                out.push(symbol as u8);
                node = 0;
                pending_bits = 0;
                pending_ones = true;
            } else {
                node = usize::from(next);
            }
        }
    }
    if pending_bits > 7 || !pending_ones {
        return error("invalid Huffman padding");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let digits: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        (0..digits.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap())
            .collect()
    }

    fn text(fields: &[Field]) -> Vec<(String, String)> {
        fields
            .iter()
            .map(|(n, v)| {
                (
                    String::from_utf8(n.clone()).unwrap(),
                    String::from_utf8(v.clone()).unwrap(),
                )
            })
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn integers_follow_the_rfc_examples() {
        // C.1.1-C.1.3
        let mut out = Vec::new();
        encode_integer(10, 5, 0, &mut out);
        assert_eq!(out, [0x0a]);
        out.clear();
        encode_integer(1337, 5, 0, &mut out);
        assert_eq!(out, [0x1f, 0x9a, 0x0a]);
        out.clear();
        encode_integer(42, 8, 0, &mut out);
        assert_eq!(out, [0x2a]);
        let mut position = 0;
        assert_eq!(
            decode_integer(&[0x1f, 0x9a, 0x0a], &mut position, 5),
            Ok(1337)
        );
        assert_eq!(position, 3);
        let mut position = 0;
        assert!(
            decode_integer(
                &[0x1f, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f],
                &mut position,
                5
            )
            .is_err()
        );
        let mut position = 0;
        assert!(decode_integer(&[0x1f, 0x9a], &mut position, 5).is_err());
    }

    #[test]
    fn decodes_the_rfc_request_examples_with_and_without_huffman() {
        // C.3: requests without Huffman coding.
        let mut decoder = Decoder::new(DEFAULT_TABLE_SIZE, 64 * 1024);
        let first = decoder
            .decode(&hex("828684410f7777772e6578616d706c652e636f6d"))
            .unwrap();
        assert_eq!(
            text(&first),
            pairs(&[
                (":method", "GET"),
                (":scheme", "http"),
                (":path", "/"),
                (":authority", "www.example.com"),
            ])
        );
        assert_eq!(decoder.table_size(), 57);
        let second = decoder
            .decode(&hex("828684be58086e6f2d6361636865"))
            .unwrap();
        assert_eq!(
            text(&second),
            pairs(&[
                (":method", "GET"),
                (":scheme", "http"),
                (":path", "/"),
                (":authority", "www.example.com"),
                ("cache-control", "no-cache"),
            ])
        );
        assert_eq!(decoder.table_size(), 110);

        // C.4: the same requests with Huffman coding.
        let mut decoder = Decoder::new(DEFAULT_TABLE_SIZE, 64 * 1024);
        let first = decoder
            .decode(&hex("828684418cf1e3c2e5f23a6ba0ab90f4ff"))
            .unwrap();
        assert_eq!(first[3].1, b"www.example.com");
        let second = decoder.decode(&hex("828684be5886a8eb10649cbf")).unwrap();
        assert_eq!(second[4], (b"cache-control".to_vec(), b"no-cache".to_vec()));
        let third = decoder
            .decode(&hex("828785bf408825a849e95ba97d7f8925a849e95bb8e8b4bf"))
            .unwrap();
        assert_eq!(
            text(&third),
            pairs(&[
                (":method", "GET"),
                (":scheme", "https"),
                (":path", "/index.html"),
                (":authority", "www.example.com"),
                ("custom-key", "custom-value"),
            ])
        );
        assert_eq!(decoder.table_size(), 164);
    }

    #[test]
    fn evicts_entries_like_the_rfc_response_examples() {
        // C.6: responses with Huffman coding and a 256-octet table.
        let mut decoder = Decoder::new(256, 64 * 1024);
        let first = decoder
            .decode(&hex(
                "488264025885aec3771a4b6196d07abe941054d444a8200595040b8166e082a62d1bff6e919d29ad171863c78f0b97c8e9ae82ae43d3",
            ))
            .unwrap();
        assert_eq!(
            text(&first),
            pairs(&[
                (":status", "302"),
                ("cache-control", "private"),
                ("date", "Mon, 21 Oct 2013 20:13:21 GMT"),
                ("location", "https://www.example.com"),
            ])
        );
        assert_eq!(decoder.table_size(), 222);
        let second = decoder.decode(&hex("4883640effc1c0bf")).unwrap();
        assert_eq!(second[0].1, b"307");
        assert_eq!(second[3].1, b"https://www.example.com");
        assert_eq!(decoder.table_size(), 222);
        let third = decoder
            .decode(&hex(
                "88c16196d07abe941054d444a8200595040b8166e084a62d1bffc05a839bd9ab77ad94e7821dd7f2e6c7b335dfdfcd5b3960d5af27087f3672c1ab270fb5291f9587316065c003ed4ee5b1063d5007",
            ))
            .unwrap();
        assert_eq!(
            text(&third),
            pairs(&[
                (":status", "200"),
                ("cache-control", "private"),
                ("date", "Mon, 21 Oct 2013 20:13:22 GMT"),
                ("location", "https://www.example.com"),
                ("content-encoding", "gzip"),
                (
                    "set-cookie",
                    "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1"
                ),
            ])
        );
        assert_eq!(decoder.table_size(), 215);
    }

    #[test]
    fn huffman_round_trips_every_octet() {
        let all: Vec<u8> = (0..=255).collect();
        let mut encoded = Vec::new();
        huffman_encode(&all, &mut encoded);
        assert_eq!(huffman_decode(&encoded).unwrap(), all);
        let mut encoded = Vec::new();
        huffman_encode(b"www.example.com", &mut encoded);
        assert_eq!(encoded, hex("f1e3c2e5f23a6ba0ab90f4ff"));
        // A full byte of padding and padding with a zero bit are both invalid.
        assert!(huffman_decode(&[0xff]).is_err());
        assert!(huffman_decode(&hex("f1e3c2e5f23a6ba0ab90f4fe")).is_err());
    }

    #[test]
    fn rejects_malformed_blocks() {
        let mut decoder = Decoder::new(DEFAULT_TABLE_SIZE, 64 * 1024);
        assert!(decoder.decode(&[0x80]).is_err(), "index 0");
        assert!(decoder.decode(&[0xbe]).is_err(), "empty dynamic table");
        assert!(decoder.decode(&[0x82, 0x20]).is_err(), "late size update");
        assert!(
            decoder.decode(&[0x3f, 0xe2, 0x1f]).is_err(),
            "size above limit"
        );
        assert!(
            decoder.decode(&[0x40, 0x05, b'a']).is_err(),
            "truncated name"
        );
        let mut small = Decoder::new(DEFAULT_TABLE_SIZE, 40);
        assert!(small.decode(&hex("828684")).is_err(), "list too large");
    }

    #[test]
    fn a_size_update_shrinks_the_table() {
        let mut decoder = Decoder::new(DEFAULT_TABLE_SIZE, 64 * 1024);
        decoder
            .decode(&hex("828684410f7777772e6578616d706c652e636f6d"))
            .unwrap();
        assert_eq!(decoder.table_size(), 57);
        // Size update to 0 empties the table; index 62 is then invalid.
        decoder.decode(&[0x20]).unwrap();
        assert_eq!(decoder.table_size(), 0);
        assert!(decoder.decode(&[0xbe]).is_err());
    }

    #[test]
    fn the_encoder_output_decodes_back() {
        let fields: Vec<(&str, &[u8])> = vec![
            (":method", b"GET"),
            (":scheme", b"https"),
            (":path", b"/search?q=ktrx"),
            (":authority", b"example.com"),
            ("user-agent", b"KTRX"),
            ("cookie", b"session=secret"),
            ("x-custom-header", b"\xffbinary\x00"),
            ("accept-encoding", b"gzip, deflate"),
        ];
        let mut block = Vec::new();
        Encoder.encode(fields.iter().copied(), &mut block);
        // Static full matches are one octet each.
        assert_eq!(block[0], 0x82);
        assert_eq!(block[1], 0x87);
        let decoded = Decoder::new(DEFAULT_TABLE_SIZE, 64 * 1024)
            .decode(&block)
            .unwrap();
        let expected: Vec<Field> = fields
            .iter()
            .map(|(n, v)| (n.as_bytes().to_vec(), v.to_vec()))
            .collect();
        assert_eq!(decoded, expected);
        // A cookie is a never-indexed literal (0001) with an indexed name (32),
        // even when its value matches the static entry.
        let mut cookie = Vec::new();
        Encoder.encode([("cookie", &b""[..])], &mut cookie);
        assert_eq!(cookie, [0x1f, 32 - 15, 0x00]);
    }
}
