//! Generic DNS query primitive (`dns.query`) exposed to sandboxed tools.
//!
//! The operating-system resolver only returns addresses, which hides CNAME
//! chains, MX, TXT, NS, SOA, CAA, SRV, and the response code. This module sends
//! one DNS question and returns the structured answer; deciding what a record
//! means (a dangling CNAME, a missing SPF policy, …) belongs to the tool.

use base64::Engine as _;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, UdpSocket},
    time::{Duration, Instant},
};

const MAX_SERVERS: usize = 4;
const MAX_NAME_JUMPS: usize = 64;
const MAX_RECORDS: usize = 512;
const UDP_BUFFER: usize = 4_096;
const EDNS_PAYLOAD: u16 = 1_232;

pub(crate) struct Answer {
    pub(crate) json: Value,
    pub(crate) records: Vec<Value>,
}

pub(crate) fn query(request: &Value) -> Result<String, String> {
    let name = request
        .get("name")
        .and_then(Value::as_str)
        .ok_or("request requires name")?;
    let record_type = request.get("type").and_then(Value::as_str).unwrap_or("A");
    let options = Options::from_request(request, "port")?;
    lookup(name, record_type, &options).map(|answer| answer.json.to_string())
}

pub(crate) struct Options {
    pub(crate) recurse: bool,
    pub(crate) dnssec: bool,
    pub(crate) tcp: bool,
    pub(crate) server: Option<IpAddr>,
    pub(crate) port: u16,
    pub(crate) timeout: Duration,
}

impl Options {
    /// Read the optional `server`, DNS server port (`port_key`), `timeout_ms`,
    /// `recurse`, `dnssec`, and `tcp` fields shared by `dns.query` and
    /// `network.resolve`.
    pub(crate) fn from_request(request: &Value, port_key: &str) -> Result<Self, String> {
        let server = match request.get("server").and_then(Value::as_str) {
            Some(text) => Some(
                text.parse::<IpAddr>()
                    .map_err(|_| "server must be an IP address")?,
            ),
            None => None,
        };
        let port = request
            .get(port_key)
            .and_then(Value::as_u64)
            .map(|p| {
                u16::try_from(p)
                    .ok()
                    .filter(|p| *p != 0)
                    .ok_or_else(|| format!("{port_key} must be between 1 and 65535"))
            })
            .transpose()?
            .unwrap_or(53);
        Ok(Self {
            recurse: request
                .get("recurse")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            dnssec: request
                .get("dnssec")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            tcp: request.get("tcp").and_then(Value::as_bool).unwrap_or(false),
            server,
            port,
            timeout: Duration::from_millis(
                request
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(3_000)
                    .clamp(100, 10_000),
            ),
        })
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            recurse: true,
            dnssec: false,
            tcp: false,
            server: None,
            port: 53,
            timeout: Duration::from_millis(3_000),
        }
    }
}

pub(crate) fn lookup(name: &str, record_type: &str, options: &Options) -> Result<Answer, String> {
    let qtype = type_code(record_type)?;
    let qname = if qtype == 12 {
        match name.parse::<IpAddr>() {
            Ok(address) => reverse_name(address),
            Err(_) => name.to_owned(),
        }
    } else {
        name.to_owned()
    };
    let servers: Vec<IpAddr> = match options.server {
        Some(server) => vec![server],
        None => system_dns_servers(),
    };
    if servers.is_empty() {
        return Err("no DNS server is configured on this host; pass a server address".into());
    }
    let id = random_id();
    let packet = build_query(id, &qname, qtype, options.recurse, options.dnssec)?;
    let mut last_error = String::from("no DNS server answered");
    for server in servers.into_iter().take(MAX_SERVERS) {
        let address = SocketAddr::new(server, options.port);
        let started = Instant::now();
        let attempt = (|| -> Result<(Message, &'static str, usize), String> {
            if options.tcp {
                let bytes = exchange_tcp(address, &packet, options.timeout)?;
                return Ok((parse_message(&bytes)?, "tcp", bytes.len()));
            }
            let bytes = exchange_udp(address, &packet, id, options.timeout)?;
            let message = parse_message(&bytes)?;
            if message.flags & 0x0200 != 0 {
                let bytes = exchange_tcp(address, &packet, options.timeout)?;
                return Ok((parse_message(&bytes)?, "tcp", bytes.len()));
            }
            Ok((message, "udp", bytes.len()))
        })();
        match attempt {
            Ok((message, transport, size)) => {
                if message.id != id {
                    last_error = format!("{server}: response id does not match the question");
                    continue;
                }
                return Ok(render(
                    &qname,
                    qtype,
                    server,
                    transport,
                    size,
                    started.elapsed(),
                    message,
                ));
            }
            Err(error) => last_error = format!("{server}: {error}"),
        }
    }
    Err(format!("DNS query failed: {last_error}"))
}

fn render(
    name: &str,
    qtype: u16,
    server: IpAddr,
    transport: &str,
    size: usize,
    elapsed: Duration,
    message: Message,
) -> Answer {
    let flags = message.flags;
    let rcode = flags & 0x000f;
    let to_json = |records: &[Record]| -> Vec<Value> {
        records.iter().map(Record::to_json).collect::<Vec<_>>()
    };
    let answers = to_json(&message.answers);
    let json = json!({
        "name": name,
        "type": type_name(qtype),
        "server": server.to_string(),
        "transport": transport,
        "rcode": rcode_name(rcode),
        "rcode_code": rcode,
        "flags": {
            "authoritative": flags & 0x0400 != 0,
            "truncated": flags & 0x0200 != 0,
            "recursion_desired": flags & 0x0100 != 0,
            "recursion_available": flags & 0x0080 != 0,
            "authentic_data": flags & 0x0020 != 0,
            "checking_disabled": flags & 0x0010 != 0,
        },
        "answers": answers.clone(),
        "authority": to_json(&message.authority),
        "additional": to_json(&message.additional),
        "rtt_ms": elapsed.as_millis() as u64,
        "response_bytes": size,
    });
    Answer {
        json,
        records: answers,
    }
}

// ---------------------------------------------------------------- encoding

fn build_query(
    id: u16,
    name: &str,
    qtype: u16,
    recurse: bool,
    dnssec: bool,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&(if recurse { 0x0100u16 } else { 0 }).to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // questions
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // additional: EDNS OPT
    encode_name(name, &mut out)?;
    out.extend_from_slice(&qtype.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // class IN
    out.push(0); // OPT owner: root
    out.extend_from_slice(&41u16.to_be_bytes());
    out.extend_from_slice(&EDNS_PAYLOAD.to_be_bytes());
    let ttl: u32 = if dnssec { 0x0000_8000 } else { 0 };
    out.extend_from_slice(&ttl.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    Ok(out)
}

fn encode_name(name: &str, out: &mut Vec<u8>) -> Result<(), String> {
    let trimmed = name.trim_end_matches('.');
    if trimmed.is_empty() {
        out.push(0);
        return Ok(());
    }
    if !trimmed.is_ascii() {
        return Err("DNS names must be ASCII; convert internationalised names to punycode".into());
    }
    if trimmed.len() > 253 {
        return Err("DNS names are limited to 253 characters".into());
    }
    for label in trimmed.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err("DNS labels must be 1-63 characters".into());
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    Ok(())
}

fn reverse_name(address: IpAddr) -> String {
    match address {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(v6) => {
            let mut nibbles = Vec::with_capacity(32);
            for byte in v6.octets().iter().rev() {
                nibbles.push(format!("{:x}", byte & 0x0f));
                nibbles.push(format!("{:x}", byte >> 4));
            }
            format!("{}.ip6.arpa", nibbles.join("."))
        }
    }
}

fn random_id() -> u16 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64),
    );
    (hasher.finish() & 0xffff) as u16
}

// ------------------------------------------------------------------- types

const TYPE_NAMES: &[(&str, u16)] = &[
    ("A", 1),
    ("NS", 2),
    ("CNAME", 5),
    ("SOA", 6),
    ("PTR", 12),
    ("MX", 15),
    ("TXT", 16),
    ("AAAA", 28),
    ("SRV", 33),
    ("NAPTR", 35),
    ("DS", 43),
    ("SSHFP", 44),
    ("RRSIG", 46),
    ("NSEC", 47),
    ("DNSKEY", 48),
    ("TLSA", 52),
    ("SVCB", 64),
    ("HTTPS", 65),
    ("CAA", 257),
    ("ANY", 255),
];

fn type_code(name: &str) -> Result<u16, String> {
    let upper = name.trim().to_ascii_uppercase();
    if let Some((_, code)) = TYPE_NAMES.iter().find(|(label, _)| *label == upper) {
        return Ok(*code);
    }
    upper
        .strip_prefix("TYPE")
        .and_then(|digits| digits.parse::<u16>().ok())
        .ok_or_else(|| format!("unsupported record type '{name}'"))
}

fn type_name(code: u16) -> String {
    TYPE_NAMES
        .iter()
        .find(|(_, value)| *value == code)
        .map_or_else(|| format!("TYPE{code}"), |(label, _)| (*label).to_owned())
}

fn rcode_name(code: u16) -> String {
    match code {
        0 => "NOERROR".into(),
        1 => "FORMERR".into(),
        2 => "SERVFAIL".into(),
        3 => "NXDOMAIN".into(),
        4 => "NOTIMP".into(),
        5 => "REFUSED".into(),
        other => format!("RCODE{other}"),
    }
}

// ----------------------------------------------------------------- parsing

#[derive(Debug)]
pub(crate) struct Message {
    pub(crate) id: u16,
    pub(crate) flags: u16,
    pub(crate) answers: Vec<Record>,
    pub(crate) authority: Vec<Record>,
    pub(crate) additional: Vec<Record>,
}

#[derive(Debug)]
pub(crate) struct Record {
    name: String,
    rtype: u16,
    class: u16,
    ttl: u32,
    data: Value,
}

impl Record {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "type": type_name(self.rtype),
            "class": if self.class == 1 { "IN".to_owned() } else { format!("CLASS{}", self.class) },
            "ttl": self.ttl,
            "data": self.data,
        })
    }
}

fn parse_message(bytes: &[u8]) -> Result<Message, String> {
    if bytes.len() < 12 {
        return Err("DNS response is shorter than a header".into());
    }
    let id = u16::from_be_bytes([bytes[0], bytes[1]]);
    let flags = u16::from_be_bytes([bytes[2], bytes[3]]);
    if flags & 0x8000 == 0 {
        return Err("DNS packet is not a response".into());
    }
    let counts: Vec<usize> = (0..4)
        .map(|i| u16::from_be_bytes([bytes[4 + i * 2], bytes[5 + i * 2]]) as usize)
        .collect();
    if counts[1] + counts[2] + counts[3] > MAX_RECORDS {
        return Err("DNS response carries too many records".into());
    }
    let mut pos = 12;
    for _ in 0..counts[0] {
        read_name(bytes, &mut pos)?;
        pos = pos
            .checked_add(4)
            .filter(|p| *p <= bytes.len())
            .ok_or("truncated DNS question")?;
    }
    let mut sections = Vec::new();
    for count in &counts[1..] {
        let mut records = Vec::new();
        for _ in 0..*count {
            let record = read_record(bytes, &mut pos)?;
            if record.rtype != 41 {
                records.push(record);
            }
        }
        sections.push(records);
    }
    let additional = sections.pop().unwrap_or_default();
    let authority = sections.pop().unwrap_or_default();
    let answers = sections.pop().unwrap_or_default();
    Ok(Message {
        id,
        flags,
        answers,
        authority,
        additional,
    })
}

fn read_name(msg: &[u8], pos: &mut usize) -> Result<String, String> {
    let mut labels: Vec<String> = Vec::new();
    let mut cursor = *pos;
    let mut resume: Option<usize> = None;
    let mut jumps = 0usize;
    let mut total = 0usize;
    loop {
        let len = *msg.get(cursor).ok_or("truncated DNS name")? as usize;
        if len == 0 {
            cursor += 1;
            break;
        }
        if len & 0xc0 == 0xc0 {
            let low = *msg.get(cursor + 1).ok_or("truncated DNS pointer")? as usize;
            if resume.is_none() {
                resume = Some(cursor + 2);
            }
            jumps += 1;
            if jumps > MAX_NAME_JUMPS {
                return Err("DNS name compression loop".into());
            }
            cursor = ((len & 0x3f) << 8) | low;
            continue;
        }
        if len & 0xc0 != 0 {
            return Err("unsupported DNS label type".into());
        }
        let label = msg
            .get(cursor + 1..cursor + 1 + len)
            .ok_or("truncated DNS label")?;
        total += len + 1;
        if total > 255 {
            return Err("DNS name is too long".into());
        }
        labels.push(String::from_utf8_lossy(label).into_owned());
        cursor += 1 + len;
    }
    *pos = resume.unwrap_or(cursor);
    Ok(if labels.is_empty() {
        ".".into()
    } else {
        labels.join(".")
    })
}

fn read_record(msg: &[u8], pos: &mut usize) -> Result<Record, String> {
    let name = read_name(msg, pos)?;
    let header = msg.get(*pos..*pos + 10).ok_or("truncated DNS record")?;
    let rtype = u16::from_be_bytes([header[0], header[1]]);
    let class = u16::from_be_bytes([header[2], header[3]]);
    let ttl = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    let rdlen = u16::from_be_bytes([header[8], header[9]]) as usize;
    *pos += 10;
    let start = *pos;
    let end = start
        .checked_add(rdlen)
        .filter(|e| *e <= msg.len())
        .ok_or("truncated DNS record data")?;
    let data = parse_rdata(msg, rtype, start, end)?;
    *pos = end;
    Ok(Record {
        name,
        rtype,
        class,
        ttl,
        data,
    })
}

fn parse_rdata(msg: &[u8], rtype: u16, start: usize, end: usize) -> Result<Value, String> {
    let rdata = &msg[start..end];
    let u16_at = |offset: usize| -> Result<u16, String> {
        rdata
            .get(offset..offset + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .ok_or_else(|| "truncated DNS record data".to_owned())
    };
    let u32_at = |offset: usize| -> Result<u32, String> {
        rdata
            .get(offset..offset + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "truncated DNS record data".to_owned())
    };
    Ok(match rtype {
        1 if rdata.len() == 4 => {
            json!({"address": Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3]).to_string()})
        }
        28 if rdata.len() == 16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(rdata);
            json!({"address": Ipv6Addr::from(octets).to_string()})
        }
        2 | 5 | 12 => {
            let mut at = start;
            json!({"target": read_name(msg, &mut at)?})
        }
        15 => {
            let mut at = start + 2;
            json!({"preference": u16_at(0)?, "exchange": read_name(msg, &mut at)?})
        }
        16 => {
            let mut strings = Vec::new();
            let mut at = 0;
            while at < rdata.len() {
                let len = rdata[at] as usize;
                let chunk = rdata
                    .get(at + 1..at + 1 + len)
                    .ok_or("truncated TXT string")?;
                strings.push(String::from_utf8_lossy(chunk).into_owned());
                at += 1 + len;
            }
            json!({"text": strings.concat(), "strings": strings})
        }
        6 => {
            let mut at = start;
            let mname = read_name(msg, &mut at)?;
            let rname = read_name(msg, &mut at)?;
            let fixed = at - start;
            json!({
                "mname": mname,
                "rname": rname,
                "serial": u32_at(fixed)?,
                "refresh": u32_at(fixed + 4)?,
                "retry": u32_at(fixed + 8)?,
                "expire": u32_at(fixed + 12)?,
                "minimum": u32_at(fixed + 16)?,
            })
        }
        33 => {
            let mut at = start + 6;
            json!({
                "priority": u16_at(0)?,
                "weight": u16_at(2)?,
                "port": u16_at(4)?,
                "target": read_name(msg, &mut at)?,
            })
        }
        257 => {
            let flags = *rdata.first().ok_or("truncated CAA record")?;
            let tag_len = *rdata.get(1).ok_or("truncated CAA record")? as usize;
            let tag = rdata.get(2..2 + tag_len).ok_or("truncated CAA record")?;
            let value = rdata.get(2 + tag_len..).ok_or("truncated CAA record")?;
            json!({
                "flags": flags,
                "tag": String::from_utf8_lossy(tag),
                "value": String::from_utf8_lossy(value),
            })
        }
        _ => json!({"raw_base64": base64::engine::general_purpose::STANDARD.encode(rdata)}),
    })
}

// --------------------------------------------------------------- transport

fn exchange_udp(
    server: SocketAddr,
    packet: &[u8],
    id: u16,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let bind: SocketAddr = if server.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let socket = UdpSocket::bind(bind).map_err(|e| format!("cannot open UDP socket: {e}"))?;
    socket
        .connect(server)
        .map_err(|e| format!("cannot reach server: {e}"))?;
    socket
        .send(packet)
        .map_err(|e| format!("send failed: {e}"))?;
    let deadline = Instant::now() + timeout;
    let mut buffer = vec![0u8; UDP_BUFFER];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out".into());
        }
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        match socket.recv(&mut buffer) {
            Ok(count) if count >= 2 && u16::from_be_bytes([buffer[0], buffer[1]]) == id => {
                return Ok(buffer[..count].to_vec());
            }
            Ok(_) => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err("timed out".into());
            }
            Err(error) => return Err(format!("receive failed: {error}")),
        }
    }
}

fn exchange_tcp(server: SocketAddr, packet: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
    let mut stream = TcpStream::connect_timeout(&server, timeout)
        .map_err(|e| format!("TCP connect failed: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let mut framed = Vec::with_capacity(packet.len() + 2);
    framed.extend_from_slice(&(packet.len() as u16).to_be_bytes());
    framed.extend_from_slice(packet);
    stream
        .write_all(&framed)
        .map_err(|e| format!("send failed: {e}"))?;
    let mut length = [0u8; 2];
    stream
        .read_exact(&mut length)
        .map_err(|e| format!("receive failed: {e}"))?;
    let length = u16::from_be_bytes(length) as usize;
    let mut bytes = vec![0u8; length];
    stream
        .read_exact(&mut bytes)
        .map_err(|e| format!("receive failed: {e}"))?;
    Ok(bytes)
}

// ------------------------------------------------------------ system config

#[cfg(windows)]
pub(crate) fn system_dns_servers() -> Vec<IpAddr> {
    use std::ffi::CStr;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        FIXED_INFO_W2KSP1, GetNetworkParams, IP_ADDR_STRING,
    };
    let mut length = 0u32;
    // SAFETY: a null buffer asks Windows for the required size.
    unsafe { GetNetworkParams(std::ptr::null_mut(), &mut length) };
    if length == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u8; length as usize];
    let info = buffer.as_mut_ptr().cast::<FIXED_INFO_W2KSP1>();
    // SAFETY: the buffer is at least `length` bytes, as Windows requested.
    if unsafe { GetNetworkParams(info, &mut length) } != 0 {
        return Vec::new();
    }
    let mut servers = Vec::new();
    // SAFETY: the list is a NUL-terminated chain owned by `buffer`.
    unsafe {
        let mut node: *const IP_ADDR_STRING = &(*info).DnsServerList;
        while !node.is_null() {
            let text = CStr::from_ptr((*node).IpAddress.String.as_ptr().cast());
            if let Ok(address) = text.to_string_lossy().trim().parse::<IpAddr>() {
                if !servers.contains(&address) {
                    servers.push(address);
                }
            }
            node = (*node).Next;
        }
    }
    servers
}

#[cfg(not(windows))]
pub(crate) fn system_dns_servers() -> Vec<IpAddr> {
    std::fs::read_to_string("/etc/resolv.conf")
        .map(|text| parse_resolv_conf(&text))
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn parse_resolv_conf(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            (parts.next() == Some("nameserver"))
                .then(|| parts.next())
                .flatten()
                .and_then(|value| value.split('%').next()?.parse::<IpAddr>().ok())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_name(out: &mut Vec<u8>, name: &str) {
        encode_name(name, out).unwrap();
    }

    fn header(answers: u16) -> Vec<u8> {
        let mut out = vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0];
        out[6..8].copy_from_slice(&answers.to_be_bytes());
        put_name(&mut out, "www.example.com");
        out.extend_from_slice(&[0, 1, 0, 1]);
        out
    }

    fn record(out: &mut Vec<u8>, rtype: u16, rdata: &[u8]) {
        out.extend_from_slice(&[0xc0, 0x0c]);
        out.extend_from_slice(&rtype.to_be_bytes());
        out.extend_from_slice(&[0, 1]);
        out.extend_from_slice(&300u32.to_be_bytes());
        out.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        out.extend_from_slice(rdata);
    }

    #[test]
    fn encodes_a_query_with_edns() {
        let packet = build_query(0xabcd, "Example.com.", 15, true, true).unwrap();
        assert_eq!(&packet[..2], &[0xab, 0xcd]);
        assert_eq!(&packet[2..4], &[0x01, 0x00]);
        assert_eq!(&packet[4..12], &[0, 1, 0, 0, 0, 0, 0, 1]);
        assert_eq!(&packet[12..25], b"\x07Example\x03com\x00");
        assert_eq!(&packet[25..29], &[0, 15, 0, 1]);
        // OPT record with the DO bit.
        assert_eq!(&packet[29..], &[0, 0, 41, 0x04, 0xd0, 0, 0, 0x80, 0, 0, 0]);
    }

    #[test]
    fn rejects_invalid_names_and_types() {
        let mut out = Vec::new();
        assert!(encode_name(&"a".repeat(64), &mut out).is_err());
        assert!(encode_name("a..b", &mut out).is_err());
        assert!(encode_name("bücher.example", &mut out).is_err());
        assert_eq!(type_code("mx").unwrap(), 15);
        assert_eq!(type_code("TYPE999").unwrap(), 999);
        assert!(type_code("bogus").is_err());
        assert_eq!(type_name(257), "CAA");
        assert_eq!(type_name(999), "TYPE999");
    }

    #[test]
    fn builds_reverse_names() {
        assert_eq!(
            reverse_name("192.0.2.10".parse().unwrap()),
            "10.2.0.192.in-addr.arpa"
        );
        assert_eq!(
            reverse_name("2001:db8::1".parse().unwrap()),
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa"
        );
    }

    #[test]
    fn parses_a_cname_chain_with_name_compression() {
        let mut bytes = header(2);
        // CNAME to "cdn.example.com" using a pointer into the question's "example.com".
        record(&mut bytes, 5, &[3, b'c', b'd', b'n', 0xc0, 0x10]);
        record(&mut bytes, 1, &[192, 0, 2, 1]);
        let message = parse_message(&bytes).unwrap();
        assert_eq!(message.id, 0x1234);
        assert_eq!(message.answers.len(), 2);
        let cname = message.answers[0].to_json();
        assert_eq!(cname["name"], "www.example.com");
        assert_eq!(cname["type"], "CNAME");
        assert_eq!(cname["ttl"], 300);
        assert_eq!(cname["data"]["target"], "cdn.example.com");
        assert_eq!(message.answers[1].to_json()["data"]["address"], "192.0.2.1");
    }

    #[test]
    fn parses_mx_txt_soa_srv_caa_and_aaaa() {
        let mut bytes = header(6);
        record(
            &mut bytes,
            15,
            &[0, 10, 4, b'm', b'a', b'i', b'l', 0xc0, 0x10],
        );
        record(&mut bytes, 16, &[3, b'a', b'=', b'1', 2, b'b', b'c']);
        let mut soa = Vec::new();
        put_name(&mut soa, "ns1.example.com");
        put_name(&mut soa, "hostmaster.example.com");
        for value in [2024010101u32, 7200, 3600, 1209600, 300] {
            soa.extend_from_slice(&value.to_be_bytes());
        }
        record(&mut bytes, 6, &soa);
        let mut srv = vec![0, 5, 0, 6, 0x01, 0xbb];
        put_name(&mut srv, "sip.example.com");
        record(&mut bytes, 33, &srv);
        let mut caa = vec![0, 5];
        caa.extend_from_slice(b"issue");
        caa.extend_from_slice(b"letsencrypt.org");
        record(&mut bytes, 257, &caa);
        record(&mut bytes, 28, &Ipv6Addr::LOCALHOST.octets());
        let message = parse_message(&bytes).unwrap();
        let data: Vec<Value> = message
            .answers
            .iter()
            .map(|r| r.to_json()["data"].clone())
            .collect();
        assert_eq!(
            data[0],
            json!({"preference": 10, "exchange": "mail.example.com"})
        );
        assert_eq!(data[1], json!({"text": "a=1bc", "strings": ["a=1", "bc"]}));
        assert_eq!(data[2]["mname"], "ns1.example.com");
        assert_eq!(data[2]["serial"], 2024010101u32);
        assert_eq!(data[2]["minimum"], 300);
        assert_eq!(
            data[3],
            json!({"priority": 5, "weight": 6, "port": 443, "target": "sip.example.com"})
        );
        assert_eq!(
            data[4],
            json!({"flags": 0, "tag": "issue", "value": "letsencrypt.org"})
        );
        assert_eq!(data[5]["address"], "::1");
    }

    #[test]
    fn keeps_unknown_record_data_as_base64() {
        let mut bytes = header(1);
        record(&mut bytes, 999, &[1, 2, 3]);
        let message = parse_message(&bytes).unwrap();
        let rr = message.answers[0].to_json();
        assert_eq!(rr["type"], "TYPE999");
        assert_eq!(rr["data"]["raw_base64"], "AQID");
    }

    #[test]
    fn rejects_compression_loops_and_truncation() {
        let mut bytes = header(1);
        // A name that points at itself.
        let here = bytes.len() as u16;
        bytes.extend_from_slice(&[0xc0 | (here >> 8) as u8, here as u8]);
        bytes.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 1, 2, 3, 4]);
        assert!(parse_message(&bytes).unwrap_err().contains("loop"));

        let mut short = header(1);
        record(&mut short, 1, &[1, 2, 3, 4]);
        short.truncate(short.len() - 2);
        assert!(parse_message(&short).is_err());
        assert!(parse_message(&[0; 5]).is_err());
        // A query (QR bit clear) is not a response.
        assert!(parse_message(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn reports_response_codes_and_flags() {
        let mut bytes = header(0);
        bytes[2] = 0x85; // QR + AA + RD
        bytes[3] = 0x83; // RA + NXDOMAIN
        let message = parse_message(&bytes).unwrap();
        let answer = render(
            "missing.example.com",
            1,
            "192.0.2.53".parse().unwrap(),
            "udp",
            bytes.len(),
            Duration::from_millis(7),
            message,
        );
        assert_eq!(answer.json["rcode"], "NXDOMAIN");
        assert_eq!(answer.json["flags"]["authoritative"], true);
        assert_eq!(answer.json["flags"]["recursion_available"], true);
        assert!(answer.records.is_empty());
    }

    #[test]
    fn answers_a_query_from_a_local_udp_server() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut buffer = [0u8; 512];
            let (count, peer) = server.recv_from(&mut buffer).unwrap();
            // Echo the question back as a response carrying one A record.
            let mut response = buffer[..count].to_vec();
            response[2] = 0x81;
            response[3] = 0x80;
            response[6..8].copy_from_slice(&1u16.to_be_bytes());
            response[10..12].copy_from_slice(&0u16.to_be_bytes());
            response.truncate(count - 11); // drop the OPT record
            record(&mut response, 1, &[203, 0, 113, 9]);
            server.send_to(&response, peer).unwrap();
        });
        let options = Options {
            server: Some("127.0.0.1".parse().unwrap()),
            port,
            timeout: Duration::from_secs(3),
            ..Options::default()
        };
        let answer = lookup("www.example.com", "A", &options).unwrap();
        assert_eq!(answer.json["rcode"], "NOERROR");
        assert_eq!(answer.json["answers"][0]["data"]["address"], "203.0.113.9");
        assert_eq!(answer.json["transport"], "udp");
        handle.join().unwrap();
    }
}
