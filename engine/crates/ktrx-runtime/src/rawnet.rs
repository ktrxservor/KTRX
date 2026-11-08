//! Privileged network primitives exposed to sandboxed tools behind the
//! `raw-net` permission: ICMP echo (with a settable TTL, which is all a
//! traceroute needs), ARP resolution, and TCP SYN probes.
//!
//! These calls may need elevated rights and are not available on every
//! operating system; an unsupported call fails with a clear message instead of
//! pretending to work. Scan strategy, traceroute loops, and interpretation
//! belong to the tool.

use serde_json::{Value, json};
use std::{
    net::{IpAddr, Ipv4Addr, ToSocketAddrs},
    time::Duration,
};

pub(crate) const OPERATIONS: [&str; 3] = ["icmp.ping", "arp.resolve", "tcp.syn_probe"];

const MAX_PAYLOAD: usize = 1_400;

pub(crate) fn request(operation: &str, input: &Value) -> Result<String, String> {
    match operation {
        "icmp.ping" => ping(input),
        "arp.resolve" => arp(input),
        "tcp.syn_probe" => syn_probe(input),
        other => Err(format!("unsupported raw-net capability '{other}'")),
    }
}

fn host_field(input: &Value, key: &str) -> Result<String, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("request requires {key}"))
}

fn ipv4_of(host: &str) -> Result<Ipv4Addr, String> {
    if let Ok(address) = host.parse::<IpAddr>() {
        return match address {
            IpAddr::V4(v4) => Ok(v4),
            IpAddr::V6(_) => Err("IPv6 destinations are not supported by raw-net yet".into()),
        };
    }
    (host, 0)
        .to_socket_addrs()
        .map_err(|e| format!("name resolution failed: {e}"))?
        .find_map(|a| match a.ip() {
            IpAddr::V4(v4) => Some(v4),
            IpAddr::V6(_) => None,
        })
        .ok_or_else(|| "hostname has no IPv4 address; IPv6 is not supported by raw-net yet".into())
}

fn millis(input: &Value, default: u64) -> Duration {
    Duration::from_millis(
        input
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(default)
            .clamp(100, 10_000),
    )
}

// -------------------------------------------------------------------- ICMP

#[derive(Debug, PartialEq)]
enum PingStatus {
    Reply,
    TtlExpired,
    Unreachable,
    Timeout,
}

impl PingStatus {
    fn name(&self) -> &'static str {
        match self {
            Self::Reply => "reply",
            Self::TtlExpired => "ttl_expired",
            Self::Unreachable => "unreachable",
            Self::Timeout => "timeout",
        }
    }
}

struct PingOutcome {
    status: PingStatus,
    from: Option<Ipv4Addr>,
    rtt_ms: Option<f64>,
    reply_ttl: Option<u8>,
    detail: Option<String>,
}

fn ping(input: &Value) -> Result<String, String> {
    let host = host_field(input, "host")?;
    let address = ipv4_of(&host)?;
    let ttl = input
        .get("ttl")
        .and_then(Value::as_u64)
        .map(|t| {
            u8::try_from(t)
                .ok()
                .filter(|t| *t != 0)
                .ok_or("ttl must be between 1 and 255")
        })
        .transpose()?
        .unwrap_or(64);
    let size = input
        .get("size")
        .and_then(Value::as_u64)
        .unwrap_or(32)
        .min(MAX_PAYLOAD as u64) as usize;
    let timeout = millis(input, 2_000);
    let payload: Vec<u8> = (0..size).map(|i| b'a' + (i % 23) as u8).collect();
    let outcome = platform::ping(address, ttl, timeout, &payload)?;
    Ok(json!({
        "host": host,
        "address": address.to_string(),
        "ttl_sent": ttl,
        "status": outcome.status.name(),
        "from": outcome.from.map(|a| a.to_string()),
        "rtt_ms": outcome.rtt_ms,
        "reply_ttl": outcome.reply_ttl,
        "bytes": size,
        "detail": outcome.detail,
    })
    .to_string())
}

pub(crate) fn icmp_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(2);
    for pair in &mut chunks {
        sum += u32::from(u16::from_be_bytes([pair[0], pair[1]]));
    }
    if let [last] = chunks.remainder() {
        sum += u32::from(*last) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg_attr(windows, allow(dead_code))]
fn build_echo_request(id: u16, seq: u16, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(8 + payload.len());
    packet.extend_from_slice(&[8, 0, 0, 0]);
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&seq.to_be_bytes());
    packet.extend_from_slice(payload);
    let checksum = icmp_checksum(&packet);
    packet[2..4].copy_from_slice(&checksum.to_be_bytes());
    packet
}

#[cfg_attr(windows, allow(dead_code))]
#[derive(Debug, PartialEq)]
enum IcmpEvent {
    EchoReply,
    TimeExceeded,
    Unreachable,
}

/// Split an optional IPv4 header off a received packet. Raw sockets deliver
/// the header; datagram ICMP sockets on Linux do not.
#[cfg_attr(windows, allow(dead_code))]
fn strip_ipv4_header(packet: &[u8]) -> (&[u8], Option<Ipv4Addr>) {
    if packet.len() >= 20 && packet[0] >> 4 == 4 && packet[9] == 1 {
        let header = usize::from(packet[0] & 0x0f) * 4;
        if header >= 20 && packet.len() >= header {
            let source = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
            return (&packet[header..], Some(source));
        }
    }
    (packet, None)
}

/// Classify a received ICMP packet that answers our echo request. With
/// `strict`, the identifier and sequence number must match ours.
#[cfg_attr(windows, allow(dead_code))]
fn classify_icmp(
    packet: &[u8],
    id: u16,
    seq: u16,
    strict: bool,
) -> Option<(IcmpEvent, Option<Ipv4Addr>)> {
    let (icmp, source) = strip_ipv4_header(packet);
    if icmp.len() < 8 {
        return None;
    }
    let matches = |bytes: &[u8]| {
        !strict
            || (u16::from_be_bytes([bytes[4], bytes[5]]) == id
                && u16::from_be_bytes([bytes[6], bytes[7]]) == seq)
    };
    match icmp[0] {
        0 if matches(icmp) => Some((IcmpEvent::EchoReply, source)),
        3 | 11 => {
            // The error quotes our original IP header plus the first 8 bytes.
            let inner = icmp.get(8..)?;
            let header = usize::from(inner.first()? & 0x0f) * 4;
            let quoted = inner.get(header..header + 8)?;
            if quoted[0] == 8 && matches(quoted) {
                let event = if icmp[0] == 11 {
                    IcmpEvent::TimeExceeded
                } else {
                    IcmpEvent::Unreachable
                };
                Some((event, source))
            } else {
                None
            }
        }
        _ => None,
    }
}

// --------------------------------------------------------------------- ARP

fn arp(input: &Value) -> Result<String, String> {
    let ip = host_field(input, "ip")?;
    let address: Ipv4Addr = ip
        .parse()
        .map_err(|_| "ip must be an IPv4 address on the local network".to_owned())?;
    let mac = platform::arp(address, millis(input, 2_000))?;
    Ok(json!({"ip": address.to_string(), "mac": mac}).to_string())
}

#[cfg_attr(windows, allow(dead_code))]
fn format_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(any(target_os = "linux", test))]
fn parse_proc_net_arp(text: &str, ip: Ipv4Addr) -> Option<String> {
    text.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let complete = fields.get(2).is_some_and(|flags| *flags != "0x0");
        let mac = fields.get(3).filter(|mac| **mac != "00:00:00:00:00:00");
        (fields.first() == Some(&ip.to_string().as_str()) && complete)
            .then(|| mac.map(|m| (*m).to_owned()))
            .flatten()
    })
}

// ---------------------------------------------------------------- TCP SYN

fn syn_probe(input: &Value) -> Result<String, String> {
    let host = host_field(input, "host")?;
    let address = ipv4_of(&host)?;
    let port = input
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .ok_or("port must be between 1 and 65535")?;
    let (state, rtt_ms) = platform::syn_probe(address, port, millis(input, 2_000))?;
    Ok(json!({
        "host": host,
        "address": address.to_string(),
        "port": port,
        "state": state,
        "method": "syn",
        "rtt_ms": rtt_ms,
    })
    .to_string())
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn tcp_checksum(source: Ipv4Addr, destination: Ipv4Addr, segment: &[u8]) -> u16 {
    let mut data = Vec::with_capacity(12 + segment.len());
    data.extend_from_slice(&source.octets());
    data.extend_from_slice(&destination.octets());
    data.extend_from_slice(&[0, 6]);
    data.extend_from_slice(&(segment.len() as u16).to_be_bytes());
    data.extend_from_slice(segment);
    icmp_checksum(&data)
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn build_syn(
    source: Ipv4Addr,
    destination: Ipv4Addr,
    source_port: u16,
    destination_port: u16,
    sequence: u32,
) -> Vec<u8> {
    let mut segment = Vec::with_capacity(24);
    segment.extend_from_slice(&source_port.to_be_bytes());
    segment.extend_from_slice(&destination_port.to_be_bytes());
    segment.extend_from_slice(&sequence.to_be_bytes());
    segment.extend_from_slice(&0u32.to_be_bytes()); // acknowledgement
    segment.push(6 << 4); // data offset: 24 bytes (with the MSS option)
    segment.push(0x02); // SYN
    segment.extend_from_slice(&1024u16.to_be_bytes()); // window
    segment.extend_from_slice(&[0, 0, 0, 0]); // checksum, urgent pointer
    segment.extend_from_slice(&[2, 4, 0x05, 0xb4]); // MSS 1460
    let checksum = tcp_checksum(source, destination, &segment);
    segment[16..18].copy_from_slice(&checksum.to_be_bytes());
    segment
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
#[derive(Debug, PartialEq)]
enum SynReply {
    Open,
    Closed,
}

/// Classify a received IPv4 packet as the answer to our SYN, or ignore it.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn classify_syn_reply(
    packet: &[u8],
    target: Ipv4Addr,
    target_port: u16,
    our_port: u16,
    our_sequence: u32,
) -> Option<SynReply> {
    if packet.len() < 20 || packet[0] >> 4 != 4 || packet[9] != 6 {
        return None;
    }
    let header = usize::from(packet[0] & 0x0f) * 4;
    let source = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
    if source != target {
        return None;
    }
    let tcp = packet.get(header..header + 20)?;
    let from_port = u16::from_be_bytes([tcp[0], tcp[1]]);
    let to_port = u16::from_be_bytes([tcp[2], tcp[3]]);
    if from_port != target_port || to_port != our_port {
        return None;
    }
    let acknowledged = u32::from_be_bytes([tcp[8], tcp[9], tcp[10], tcp[11]]);
    if acknowledged != our_sequence.wrapping_add(1) {
        return None;
    }
    let flags = tcp[13];
    if flags & 0x12 == 0x12 {
        Some(SynReply::Open)
    } else if flags & 0x04 != 0 {
        Some(SynReply::Closed)
    } else {
        None
    }
}

// --------------------------------------------------------------- platforms

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::{
        Foundation::{GetLastError, INVALID_HANDLE_VALUE},
        NetworkManagement::IpHelper::{
            ICMP_ECHO_REPLY, IP_OPTION_INFORMATION, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
            SendARP,
        },
    };

    const IP_DEST_NET_UNREACHABLE: u32 = 11002;
    const IP_DEST_HOST_UNREACHABLE: u32 = 11003;
    const IP_DEST_PROT_UNREACHABLE: u32 = 11004;
    const IP_DEST_PORT_UNREACHABLE: u32 = 11005;
    const IP_REQ_TIMED_OUT: u32 = 11010;
    const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;
    const IP_TTL_EXPIRED_REASSEM: u32 = 11014;

    pub(super) fn ping(
        address: Ipv4Addr,
        ttl: u8,
        timeout: Duration,
        payload: &[u8],
    ) -> Result<PingOutcome, String> {
        // SAFETY: plain Win32 calls; every pointer passed is valid for the call.
        unsafe {
            let handle = IcmpCreateFile();
            if handle == INVALID_HANDLE_VALUE {
                return Err(format!(
                    "cannot open an ICMP handle (error {})",
                    GetLastError()
                ));
            }
            let options = IP_OPTION_INFORMATION {
                Ttl: ttl,
                Tos: 0,
                Flags: 0,
                OptionsSize: 0,
                OptionsData: std::ptr::null_mut(),
            };
            let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 16];
            let destination = u32::from_ne_bytes(address.octets());
            let count = IcmpSendEcho(
                handle,
                destination,
                payload.as_ptr().cast::<c_void>(),
                payload.len() as u16,
                &options,
                reply.as_mut_ptr().cast::<c_void>(),
                reply.len() as u32,
                timeout.as_millis() as u32,
            );
            let error = GetLastError();
            IcmpCloseHandle(handle);
            if count == 0 {
                return if error == IP_REQ_TIMED_OUT {
                    Ok(PingOutcome {
                        status: PingStatus::Timeout,
                        from: None,
                        rtt_ms: None,
                        reply_ttl: None,
                        detail: None,
                    })
                } else {
                    Err(format!("ICMP echo failed (Windows error {error})"))
                };
            }
            let echo = std::ptr::read_unaligned(reply.as_ptr().cast::<ICMP_ECHO_REPLY>());
            let from = Ipv4Addr::from(echo.Address.to_ne_bytes());
            let status = match echo.Status {
                0 => PingStatus::Reply,
                IP_TTL_EXPIRED_TRANSIT | IP_TTL_EXPIRED_REASSEM => PingStatus::TtlExpired,
                IP_DEST_NET_UNREACHABLE
                | IP_DEST_HOST_UNREACHABLE
                | IP_DEST_PROT_UNREACHABLE
                | IP_DEST_PORT_UNREACHABLE => PingStatus::Unreachable,
                IP_REQ_TIMED_OUT => PingStatus::Timeout,
                other => {
                    return Err(format!("ICMP echo returned Windows status {other}"));
                }
            };
            Ok(PingOutcome {
                status,
                from: Some(from),
                rtt_ms: Some(f64::from(echo.RoundTripTime)),
                reply_ttl: Some(echo.Options.Ttl),
                detail: (echo.Status != 0).then(|| format!("Windows IP status {}", echo.Status)),
            })
        }
    }

    pub(super) fn arp(address: Ipv4Addr, _timeout: Duration) -> Result<String, String> {
        let mut mac = [0u8; 8];
        let mut length = mac.len() as u32;
        // SAFETY: `mac` and `length` outlive the call and `length` matches the buffer.
        let status = unsafe {
            SendARP(
                u32::from_ne_bytes(address.octets()),
                0,
                mac.as_mut_ptr().cast::<c_void>(),
                &mut length,
            )
        };
        match status {
            0 if length >= 6 => Ok(format_mac(&mac[..6])),
            0 => Err("ARP returned an empty hardware address".into()),
            31 | 67 => {
                Err("no ARP reply: the host is not on the local network segment or is down".into())
            }
            other => Err(format!("ARP resolution failed (Windows error {other})")),
        }
    }

    pub(super) fn syn_probe(
        _address: Ipv4Addr,
        _port: u16,
        _timeout: Duration,
    ) -> Result<(&'static str, Option<f64>), String> {
        Err("SYN probes need raw TCP sockets, which Windows does not give to applications; use socket.connect for a TCP connect probe".into())
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    use std::{
        mem::MaybeUninit,
        net::{SocketAddrV4, UdpSocket},
        time::Instant,
    };

    fn receive(socket: &Socket, buffer: &mut [MaybeUninit<u8>]) -> std::io::Result<usize> {
        socket.recv(buffer)
    }

    fn bytes(buffer: &[MaybeUninit<u8>], count: usize) -> Vec<u8> {
        // SAFETY: `recv` initialised the first `count` bytes.
        buffer[..count]
            .iter()
            .map(|b| unsafe { b.assume_init() })
            .collect()
    }

    pub(super) fn ping(
        address: Ipv4Addr,
        ttl: u8,
        timeout: Duration,
        payload: &[u8],
    ) -> Result<PingOutcome, String> {
        // A raw socket also sees "time exceeded" errors, which traceroute needs;
        // fall back to an unprivileged datagram socket for plain echo.
        let (socket, raw) = match Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4)) {
            Ok(socket) => (socket, true),
            Err(_) => (
                Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).map_err(|e| {
                    format!("cannot open an ICMP socket (elevated rights may be required): {e}")
                })?,
                false,
            ),
        };
        socket
            .set_ttl_v4(u32::from(ttl))
            .map_err(|e| format!("cannot set TTL: {e}"))?;
        let id = (std::process::id() & 0xffff) as u16;
        let sequence = 1u16;
        let request = build_echo_request(id, sequence, payload);
        let started = Instant::now();
        socket
            .send_to(&request, &SockAddr::from(SocketAddrV4::new(address, 0)))
            .map_err(|e| format!("ICMP send failed: {e}"))?;
        let mut buffer = [MaybeUninit::<u8>::uninit(); 2048];
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(PingOutcome {
                    status: PingStatus::Timeout,
                    from: None,
                    rtt_ms: None,
                    reply_ttl: None,
                    detail: (!raw).then(|| {
                        "datagram ICMP socket: time-exceeded replies are not delivered".to_owned()
                    }),
                });
            }
            socket
                .set_read_timeout(Some(remaining))
                .map_err(|e| e.to_string())?;
            let count = match receive(&socket, &mut buffer) {
                Ok(count) => count,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(format!("ICMP receive failed: {error}")),
            };
            let packet = bytes(&buffer, count);
            let ttl_of_reply = (packet.len() >= 9 && packet[0] >> 4 == 4).then(|| packet[8]);
            if let Some((event, source)) = classify_icmp(&packet, id, sequence, raw) {
                let status = match event {
                    IcmpEvent::EchoReply => PingStatus::Reply,
                    IcmpEvent::TimeExceeded => PingStatus::TtlExpired,
                    IcmpEvent::Unreachable => PingStatus::Unreachable,
                };
                return Ok(PingOutcome {
                    status,
                    from: source.or(Some(address)),
                    rtt_ms: Some(
                        (started.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0,
                    ),
                    reply_ttl: ttl_of_reply,
                    detail: None,
                });
            }
        }
    }

    pub(super) fn arp(address: Ipv4Addr, timeout: Duration) -> Result<String, String> {
        #[cfg(target_os = "linux")]
        {
            // Sending any datagram makes the kernel resolve the neighbour; then
            // read the neighbour table.
            if let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
                let _ = socket.send_to(&[], (address, 9));
            }
            let started = Instant::now();
            loop {
                if let Ok(table) = std::fs::read_to_string("/proc/net/arp") {
                    if let Some(mac) = parse_proc_net_arp(&table, address) {
                        return Ok(mac);
                    }
                }
                if started.elapsed() >= timeout {
                    return Err(
                        "no ARP entry: the host is not on the local network segment or is down"
                            .into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (address, timeout);
            Err("ARP resolution is only available on Windows and Linux".into())
        }
    }

    pub(super) fn syn_probe(
        address: Ipv4Addr,
        port: u16,
        timeout: Duration,
    ) -> Result<(&'static str, Option<f64>), String> {
        #[cfg(target_os = "linux")]
        {
            let source = {
                let probe =
                    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| e.to_string())?;
                probe
                    .connect((address, 9))
                    .map_err(|e| format!("no route to {address}: {e}"))?;
                match probe.local_addr().map_err(|e| e.to_string())?.ip() {
                    IpAddr::V4(v4) => v4,
                    IpAddr::V6(_) => return Err("no IPv4 source address".into()),
                }
            };
            let socket =
                Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::TCP)).map_err(|e| {
                    format!("cannot open a raw TCP socket (root or CAP_NET_RAW is required): {e}")
                })?;
            let seed = std::process::id()
                ^ std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.subsec_nanos());
            let our_port = 40_000 + (seed % 20_000) as u16;
            let sequence = seed.wrapping_mul(2_654_435_761);
            let packet = build_syn(source, address, our_port, port, sequence);
            let started = Instant::now();
            socket
                .send_to(&packet, &SockAddr::from(SocketAddrV4::new(address, 0)))
                .map_err(|e| format!("SYN send failed: {e}"))?;
            let mut buffer = [MaybeUninit::<u8>::uninit(); 2048];
            loop {
                let remaining = timeout.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    return Ok(("filtered", None));
                }
                socket
                    .set_read_timeout(Some(remaining))
                    .map_err(|e| e.to_string())?;
                match receive(&socket, &mut buffer) {
                    Ok(count) => {
                        let reply = bytes(&buffer, count);
                        if let Some(kind) =
                            classify_syn_reply(&reply, address, port, our_port, sequence)
                        {
                            let rtt =
                                (started.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0;
                            return Ok((
                                match kind {
                                    SynReply::Open => "open",
                                    SynReply::Closed => "closed",
                                },
                                Some(rtt),
                            ));
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(error) => return Err(format!("SYN receive failed: {error}")),
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (address, port, timeout);
            Err("SYN probes are only available on Linux; use socket.connect for a TCP connect probe".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_the_rfc_1071_checksum() {
        assert_eq!(
            icmp_checksum(&[0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7]),
            0x220d
        );
        // An odd trailing byte is padded with zero.
        assert_eq!(icmp_checksum(&[0x01]), !0x0100u16);
    }

    #[test]
    fn builds_a_valid_echo_request() {
        let packet = build_echo_request(0xbeef, 7, b"abcd");
        assert_eq!(&packet[..2], &[8, 0]);
        assert_eq!(&packet[4..8], &[0xbe, 0xef, 0x00, 0x07]);
        assert_eq!(&packet[8..], b"abcd");
        // Re-checksumming a packet that already carries its checksum yields zero.
        assert_eq!(icmp_checksum(&packet), 0);
    }

    fn ip_header(source: [u8; 4], protocol: u8, payload_len: usize) -> Vec<u8> {
        let mut header = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, protocol, 0, 0];
        let total = (20 + payload_len) as u16;
        header[2..4].copy_from_slice(&total.to_be_bytes());
        header.extend_from_slice(&source);
        header.extend_from_slice(&[192, 0, 2, 99]);
        header
    }

    #[test]
    fn recognises_echo_replies_and_time_exceeded() {
        let mut reply = ip_header([203, 0, 113, 5], 1, 8);
        reply.extend_from_slice(&[0, 0, 0, 0, 0x12, 0x34, 0x00, 0x01]);
        let (event, source) = classify_icmp(&reply, 0x1234, 1, true).unwrap();
        assert_eq!(event, IcmpEvent::EchoReply);
        assert_eq!(source, Some(Ipv4Addr::new(203, 0, 113, 5)));
        assert!(classify_icmp(&reply, 0x9999, 1, true).is_none());
        assert!(classify_icmp(&reply, 0x9999, 1, false).is_some());

        // Time exceeded from a router, quoting our original echo request.
        let mut quoted = ip_header([192, 0, 2, 99], 1, 8);
        quoted.extend_from_slice(&[8, 0, 0, 0, 0x12, 0x34, 0x00, 0x01]);
        let mut exceeded = ip_header([198, 51, 100, 1], 1, 8 + quoted.len());
        exceeded.extend_from_slice(&[11, 0, 0, 0, 0, 0, 0, 0]);
        exceeded.extend_from_slice(&quoted);
        let (event, source) = classify_icmp(&exceeded, 0x1234, 1, true).unwrap();
        assert_eq!(event, IcmpEvent::TimeExceeded);
        assert_eq!(source, Some(Ipv4Addr::new(198, 51, 100, 1)));
        assert!(classify_icmp(&exceeded, 0x4321, 1, true).is_none());

        exceeded[20] = 3; // destination unreachable
        assert_eq!(
            classify_icmp(&exceeded, 0x1234, 1, true).unwrap().0,
            IcmpEvent::Unreachable
        );
        assert!(classify_icmp(&[0, 1, 2], 1, 1, true).is_none());
    }

    #[test]
    fn datagram_sockets_deliver_icmp_without_an_ip_header() {
        let bare = [0, 0, 0, 0, 0x00, 0x01, 0x00, 0x01];
        let (event, source) = classify_icmp(&bare, 0xabcd, 1, false).unwrap();
        assert_eq!(event, IcmpEvent::EchoReply);
        assert_eq!(source, None);
    }

    #[test]
    fn builds_a_syn_segment_with_a_valid_checksum() {
        let source = Ipv4Addr::new(192, 0, 2, 10);
        let destination = Ipv4Addr::new(198, 51, 100, 20);
        let segment = build_syn(source, destination, 40_000, 443, 0xdead_beef);
        assert_eq!(segment.len(), 24);
        assert_eq!(&segment[..4], &[0x9c, 0x40, 0x01, 0xbb]);
        assert_eq!(segment[12], 0x60);
        assert_eq!(segment[13], 0x02);
        assert_eq!(tcp_checksum(source, destination, &segment), 0);
    }

    #[test]
    fn classifies_syn_ack_and_rst_replies() {
        let target = Ipv4Addr::new(198, 51, 100, 20);
        let build = |flags: u8, ack: u32, src_port: u16, dst_port: u16, source: [u8; 4]| {
            let mut packet = ip_header(source, 6, 20);
            let mut tcp = vec![0u8; 20];
            tcp[..2].copy_from_slice(&src_port.to_be_bytes());
            tcp[2..4].copy_from_slice(&dst_port.to_be_bytes());
            tcp[8..12].copy_from_slice(&ack.to_be_bytes());
            tcp[12] = 5 << 4;
            tcp[13] = flags;
            packet.extend_from_slice(&tcp);
            packet
        };
        let sequence = 1000u32;
        let open = build(0x12, 1001, 443, 40_000, [198, 51, 100, 20]);
        assert_eq!(
            classify_syn_reply(&open, target, 443, 40_000, sequence),
            Some(SynReply::Open)
        );
        let closed = build(0x14, 1001, 443, 40_000, [198, 51, 100, 20]);
        assert_eq!(
            classify_syn_reply(&closed, target, 443, 40_000, sequence),
            Some(SynReply::Closed)
        );
        // Wrong acknowledgement, port, or source are ignored.
        assert!(
            classify_syn_reply(
                &build(0x12, 5, 443, 40_000, [198, 51, 100, 20]),
                target,
                443,
                40_000,
                sequence
            )
            .is_none()
        );
        assert!(
            classify_syn_reply(
                &build(0x12, 1001, 80, 40_000, [198, 51, 100, 20]),
                target,
                443,
                40_000,
                sequence
            )
            .is_none()
        );
        assert!(
            classify_syn_reply(
                &build(0x12, 1001, 443, 40_000, [10, 0, 0, 1]),
                target,
                443,
                40_000,
                sequence
            )
            .is_none()
        );
    }

    #[test]
    fn reads_the_linux_neighbour_table() {
        let table = "IP address       HW type     Flags       HW address            Mask     Device\n\
                     192.168.1.1      0x1         0x2         aa:bb:cc:dd:ee:ff     *        eth0\n\
                     192.168.1.9      0x1         0x0         00:00:00:00:00:00     *        eth0\n";
        assert_eq!(
            parse_proc_net_arp(table, Ipv4Addr::new(192, 168, 1, 1)).as_deref(),
            Some("aa:bb:cc:dd:ee:ff")
        );
        assert_eq!(
            parse_proc_net_arp(table, Ipv4Addr::new(192, 168, 1, 9)),
            None
        );
        assert_eq!(parse_proc_net_arp(table, Ipv4Addr::new(10, 0, 0, 1)), None);
    }

    #[test]
    fn validates_requests_before_touching_the_network() {
        assert!(request("icmp.ping", &json!({})).is_err());
        assert!(request("icmp.ping", &json!({"host": "127.0.0.1", "ttl": 0})).is_err());
        assert!(request("icmp.ping", &json!({"host": "::1"})).is_err());
        assert!(request("arp.resolve", &json!({"ip": "not-an-ip"})).is_err());
        assert!(request("tcp.syn_probe", &json!({"host": "127.0.0.1", "port": 0})).is_err());
        assert!(request("icmp.flood", &json!({})).is_err());
        assert_eq!(format_mac(&[0, 0x1a, 0xff, 3, 4, 5]), "00:1a:ff:03:04:05");
    }

    #[test]
    fn pings_the_loopback_address_when_the_platform_allows_it() {
        // Loopback echo works unprivileged on Windows and, with datagram ICMP,
        // on most Linux hosts; where the OS forbids it the call must fail
        // cleanly rather than panic.
        match request(
            "icmp.ping",
            &json!({"host": "127.0.0.1", "timeout_ms": 1500}),
        ) {
            Ok(text) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(value["status"], "reply", "{value}");
                assert_eq!(value["address"], "127.0.0.1");
            }
            Err(error) => assert!(error.contains("ICMP"), "{error}"),
        }
    }
}
