//! HTTP/2 client connection (RFC 9113) over a [`Conn`].
//!
//! One [`H2Conn`] carries any number of requests to one origin, several at a
//! time: [`H2Conn::execute`] opens up to the peer's (and the client's) stream
//! limit at once and reads the interleaved responses. Flow control is
//! honoured in both directions, server push is disabled, and every protocol
//! violation by the peer ends the stream or the connection with the error
//! code RFC 9113 prescribes.

use crate::{
    conn::Conn,
    decode::ACCEPT_ENCODING,
    h1::RawResponse,
    hpack,
    http::{Config, ErrorKind, HttpError, Prepared, Version},
};
use std::collections::{HashMap, VecDeque};

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

const DATA: u8 = 0x0;
const HEADERS: u8 = 0x1;
const PRIORITY: u8 = 0x2;
const RST_STREAM: u8 = 0x3;
const SETTINGS: u8 = 0x4;
const PUSH_PROMISE: u8 = 0x5;
const PING: u8 = 0x6;
const GOAWAY: u8 = 0x7;
const WINDOW_UPDATE: u8 = 0x8;
const CONTINUATION: u8 = 0x9;

const END_STREAM: u8 = 0x1;
const ACK: u8 = 0x1;
const END_HEADERS: u8 = 0x4;
const PADDED: u8 = 0x8;
const PRIORITY_FLAG: u8 = 0x20;

const PROTOCOL_ERROR: u32 = 0x1;
const FLOW_CONTROL_ERROR: u32 = 0x3;
const FRAME_SIZE_ERROR: u32 = 0x6;
const REFUSED_STREAM: u32 = 0x7;
const CANCEL: u32 = 0x8;
const COMPRESSION_ERROR: u32 = 0x9;

const SETTINGS_ENABLE_PUSH: u16 = 0x2;
const SETTINGS_MAX_CONCURRENT_STREAMS: u16 = 0x3;
const SETTINGS_INITIAL_WINDOW_SIZE: u16 = 0x4;
const SETTINGS_MAX_FRAME_SIZE: u16 = 0x5;
const SETTINGS_MAX_HEADER_LIST_SIZE: u16 = 0x6;

const DEFAULT_WINDOW: i64 = 65_535;
const MAX_WINDOW: i64 = (1 << 31) - 1;
const MAX_STREAM_ID: u32 = (1 << 31) - 1;
/// Frames larger than the default maximum are refused (we never raise it).
const MAX_FRAME_SIZE: usize = 16_384;
/// Receive windows granted to the server.
const STREAM_WINDOW: u32 = 4 * 1024 * 1024;
const CONNECTION_WINDOW: u32 = 16 * 1024 * 1024;

/// Name of an HTTP/2 error code, for messages.
fn code_name(code: u32) -> String {
    match code {
        0x0 => "NO_ERROR".into(),
        0x1 => "PROTOCOL_ERROR".into(),
        0x2 => "INTERNAL_ERROR".into(),
        0x3 => "FLOW_CONTROL_ERROR".into(),
        0x4 => "SETTINGS_TIMEOUT".into(),
        0x5 => "STREAM_CLOSED".into(),
        0x6 => "FRAME_SIZE_ERROR".into(),
        0x7 => "REFUSED_STREAM".into(),
        0x8 => "CANCEL".into(),
        0x9 => "COMPRESSION_ERROR".into(),
        0xa => "CONNECT_ERROR".into(),
        0xb => "ENHANCE_YOUR_CALM".into(),
        0xc => "INADEQUATE_SECURITY".into(),
        0xd => "HTTP_1_1_REQUIRED".into(),
        other => format!("0x{other:x}"),
    }
}

/// A failure that ends the whole connection.
enum ConnectionError {
    /// The transport failed (closed, timed out, cancelled).
    Io(HttpError),
    /// The peer broke the protocol; a GOAWAY with this code is sent.
    Protocol(u32, String),
}

impl From<HttpError> for ConnectionError {
    fn from(error: HttpError) -> Self {
        Self::Io(error)
    }
}

struct Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Vec<u8>,
}

struct Stream {
    index: usize,
    head: bool,
    send_window: i64,
    body: Vec<u8>,
    sent: usize,
    end_sent: bool,
    response: Option<RawResponse>,
    interim: Vec<u16>,
    recv_pending: u32,
    done: bool,
    error: Option<HttpError>,
    /// Any frame arrived for this stream (a failure before that is retryable
    /// on a fresh connection).
    answered: bool,
}

impl Stream {
    fn fail(&mut self, error: HttpError) {
        if !self.done {
            self.error = Some(error);
            self.done = true;
        }
    }

    fn result(mut self) -> Result<RawResponse, HttpError> {
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        let mut response = self.response.take().ok_or_else(|| {
            HttpError::new(ErrorKind::Protocol, "the stream ended without a response")
        })?;
        response.interim = std::mem::take(&mut self.interim);
        Ok(response)
    }
}

pub(crate) struct H2Conn<'a> {
    conn: Conn<'a>,
    decoder: hpack::Decoder,
    out: Vec<u8>,
    next_stream_id: u32,
    peer_max_frame: usize,
    peer_initial_window: i64,
    peer_max_streams: usize,
    send_window: i64,
    recv_pending: u32,
    /// A header block waiting for CONTINUATION frames.
    continuation: Option<(u32, bool, Vec<u8>)>,
    goaway: Option<u32>,
    broken: bool,
    max_header_bytes: usize,
    /// At least one frame arrived from the server.
    peer_spoke: bool,
}

impl<'a> H2Conn<'a> {
    /// Send the connection preface and our settings. The server's settings
    /// are read along with the first responses.
    pub(crate) fn handshake(mut conn: Conn<'a>, config: &Config) -> Result<Self, HttpError> {
        let mut out = PREFACE.to_vec();
        let mut settings = Vec::new();
        for (id, value) in [
            (SETTINGS_ENABLE_PUSH, 0u32),
            (SETTINGS_INITIAL_WINDOW_SIZE, STREAM_WINDOW),
            (
                SETTINGS_MAX_HEADER_LIST_SIZE,
                u32::try_from(config.max_header_bytes).unwrap_or(u32::MAX),
            ),
        ] {
            settings.extend_from_slice(&id.to_be_bytes());
            settings.extend_from_slice(&value.to_be_bytes());
        }
        frame(&mut out, SETTINGS, 0, 0, &settings);
        frame(
            &mut out,
            WINDOW_UPDATE,
            0,
            0,
            &(CONNECTION_WINDOW - DEFAULT_WINDOW as u32).to_be_bytes(),
        );
        conn.write_all(&out)?;
        Ok(Self {
            conn,
            decoder: hpack::Decoder::new(hpack::DEFAULT_TABLE_SIZE, config.max_header_bytes),
            out: Vec::new(),
            next_stream_id: 1,
            peer_max_frame: MAX_FRAME_SIZE,
            peer_initial_window: DEFAULT_WINDOW,
            peer_max_streams: usize::MAX,
            send_window: DEFAULT_WINDOW,
            recv_pending: 0,
            continuation: None,
            goaway: None,
            broken: false,
            max_header_bytes: config.max_header_bytes,
            peer_spoke: false,
        })
    }

    pub(crate) fn info(&self) -> crate::conn::ConnInfo {
        self.conn.info.clone()
    }

    /// Requests sent on this connection so far.
    pub(crate) fn streams_opened(&self) -> u32 {
        self.next_stream_id / 2
    }

    /// Whether more requests may be sent on this connection.
    pub(crate) fn reusable(&self) -> bool {
        !self.broken
            && self.goaway.is_none()
            && self.conn.is_open()
            && self.next_stream_id < MAX_STREAM_ID - 2
    }

    /// Send every request, as many at a time as both sides allow, and
    /// collect the responses in request order.
    pub(crate) fn execute(
        &mut self,
        requests: &[&Prepared],
        config: &Config,
    ) -> Vec<Result<RawResponse, HttpError>> {
        let mut results: Vec<Option<Result<RawResponse, HttpError>>> =
            requests.iter().map(|_| None).collect();
        let mut queue: VecDeque<usize> = (0..requests.len()).collect();
        let mut streams: HashMap<u32, Stream> = HashMap::new();
        let limit = config.max_concurrent_streams.max(1);
        loop {
            while self.reusable() && streams.len() < limit.min(self.peer_max_streams) {
                let Some(index) = queue.pop_front() else {
                    break;
                };
                match self.open_stream(requests[index], index, config) {
                    Ok((id, stream)) => {
                        streams.insert(id, stream);
                    }
                    Err(error) => {
                        self.broken = true;
                        results[index] = Some(Err(error));
                    }
                }
            }
            if let Err(error) = self.send_data(&mut streams) {
                self.fail_connection(ConnectionError::Io(error), &mut streams);
            }
            let finished: Vec<u32> = streams
                .iter()
                .filter(|(_, s)| s.done)
                .map(|(id, _)| *id)
                .collect();
            for id in finished {
                if let Some(stream) = streams.remove(&id) {
                    let index = stream.index;
                    results[index] = Some(stream.result());
                }
            }
            if streams.is_empty() && (queue.is_empty() || !self.reusable()) {
                break;
            }
            match self.read_frame() {
                Ok(frame) => {
                    if let Err(error) = self.handle(frame, &mut streams, config) {
                        self.fail_connection(error, &mut streams);
                    }
                }
                Err(error) => self.fail_connection(error, &mut streams),
            }
        }
        // Requests never sent because the connection ended may be retried.
        for index in queue {
            let mut error = HttpError::new(
                ErrorKind::Closed,
                "the HTTP/2 connection ended before the request was sent",
            );
            error.retryable = true;
            results[index] = Some(Err(error));
        }
        let _ = self.flush();
        results
            .into_iter()
            .map(|r| {
                r.unwrap_or_else(|| Err(HttpError::new(ErrorKind::Other, "request was not run")))
            })
            .collect()
    }

    fn open_stream(
        &mut self,
        request: &Prepared,
        index: usize,
        config: &Config,
    ) -> Result<(u32, Stream), HttpError> {
        let id = self.next_stream_id;
        self.next_stream_id += 2;
        let has = |name: &str| {
            request
                .headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case(name))
        };
        let authority = request
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("host"))
            .map_or(request.authority.as_str(), |(_, v)| v.as_str());
        let mut fields: Vec<(String, Vec<u8>)> = vec![
            (":method".into(), request.method.clone().into_bytes()),
            (":scheme".into(), request.scheme.clone().into_bytes()),
            (":authority".into(), authority.as_bytes().to_vec()),
            (":path".into(), request.path.clone().into_bytes()),
        ];
        for (name, value) in &request.headers {
            let name = name.to_ascii_lowercase();
            // Connection-specific fields are not allowed in HTTP/2 (§8.2.2).
            let forbidden = matches!(
                name.as_str(),
                "host"
                    | "connection"
                    | "keep-alive"
                    | "proxy-connection"
                    | "transfer-encoding"
                    | "upgrade"
            ) || (name == "te" && !value.eq_ignore_ascii_case("trailers"));
            if !forbidden {
                fields.push((name, value.clone().into_bytes()));
            }
        }
        if !has("user-agent") && !config.user_agent.is_empty() {
            fields.push(("user-agent".into(), config.user_agent.clone().into_bytes()));
        }
        if !has("accept") {
            fields.push(("accept".into(), b"*/*".to_vec()));
        }
        if config.decompress && !has("accept-encoding") {
            fields.push((
                "accept-encoding".into(),
                ACCEPT_ENCODING.as_bytes().to_vec(),
            ));
        }
        if !request.body.is_empty() && !has("content-length") {
            fields.push((
                "content-length".into(),
                request.body.len().to_string().into_bytes(),
            ));
        }
        let mut block = Vec::new();
        hpack::Encoder.encode(
            fields.iter().map(|(n, v)| (n.as_str(), v.as_slice())),
            &mut block,
        );
        let end_stream = request.body.is_empty();
        let mut chunks = block.chunks(self.peer_max_frame).peekable();
        let first = chunks.next().unwrap_or_default();
        let mut flags = if end_stream { END_STREAM } else { 0 };
        if chunks.peek().is_none() {
            flags |= END_HEADERS;
        }
        frame(&mut self.out, HEADERS, flags, id, first);
        while let Some(chunk) = chunks.next() {
            let flags = if chunks.peek().is_none() {
                END_HEADERS
            } else {
                0
            };
            frame(&mut self.out, CONTINUATION, flags, id, chunk);
        }
        self.flush()?;
        Ok((
            id,
            Stream {
                index,
                head: request.method == "HEAD",
                send_window: self.peer_initial_window,
                body: request.body.clone(),
                sent: 0,
                end_sent: end_stream,
                response: None,
                interim: Vec::new(),
                recv_pending: 0,
                done: false,
                error: None,
                answered: false,
            },
        ))
    }

    /// Send as much request body as the flow-control windows allow.
    fn send_data(&mut self, streams: &mut HashMap<u32, Stream>) -> Result<(), HttpError> {
        let mut ids: Vec<u32> = streams.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let Some(stream) = streams.get_mut(&id) else {
                continue;
            };
            while !stream.end_sent && !stream.done {
                let remaining = stream.body.len() - stream.sent;
                let window = stream.send_window.min(self.send_window);
                if window <= 0 {
                    break;
                }
                let size = remaining.min(window as usize).min(self.peer_max_frame);
                let end = size == remaining;
                frame(
                    &mut self.out,
                    DATA,
                    if end { END_STREAM } else { 0 },
                    id,
                    &stream.body[stream.sent..stream.sent + size],
                );
                stream.sent += size;
                stream.send_window -= size as i64;
                self.send_window -= size as i64;
                stream.end_sent = end;
                if self.out.len() >= 256 * 1024 {
                    self.flush()?;
                }
            }
        }
        self.flush()
    }

    fn flush(&mut self) -> Result<(), HttpError> {
        if self.out.is_empty() {
            return Ok(());
        }
        let out = std::mem::take(&mut self.out);
        self.conn.write_all(&out)
    }

    fn read_frame(&mut self) -> Result<Frame, ConnectionError> {
        self.flush()?;
        let header = self.conn.read_exact(9)?;
        if !self.peer_spoke && header.starts_with(b"HTTP/") {
            // An HTTP/1.x server answering our preface (h2c to a server
            // that does not support it).
            self.broken = true;
            return Err(ConnectionError::Io(HttpError::new(
                ErrorKind::Protocol,
                "the server answered in HTTP/1.x: it does not support HTTP/2 on this connection",
            )));
        }
        self.peer_spoke = true;
        let length =
            usize::from(header[0]) << 16 | usize::from(header[1]) << 8 | usize::from(header[2]);
        if length > MAX_FRAME_SIZE {
            return Err(ConnectionError::Protocol(
                FRAME_SIZE_ERROR,
                format!("frame of {length} bytes exceeds the {MAX_FRAME_SIZE}-byte maximum"),
            ));
        }
        let payload = self.conn.read_exact(length)?;
        Ok(Frame {
            kind: header[3],
            flags: header[4],
            stream: u32::from_be_bytes([header[5], header[6], header[7], header[8]])
                & MAX_STREAM_ID,
            payload,
        })
    }

    /// End the connection: tell the server why (when it broke the protocol)
    /// and fail every stream still open.
    fn fail_connection(&mut self, error: ConnectionError, streams: &mut HashMap<u32, Stream>) {
        self.broken = true;
        let error = match error {
            ConnectionError::Io(error) => error,
            ConnectionError::Protocol(code, message) => {
                let mut payload = 0u32.to_be_bytes().to_vec();
                payload.extend_from_slice(&code.to_be_bytes());
                frame(&mut self.out, GOAWAY, 0, 0, &payload);
                let _ = self.flush();
                HttpError::new(
                    ErrorKind::Protocol,
                    format!("HTTP/2 connection error {}: {message}", code_name(code)),
                )
            }
        };
        for stream in streams.values_mut() {
            let mut error = error.clone();
            // A connection that closed before answering this stream (typically an
            // idle connection the server dropped) may be retried on a new one.
            error.retryable =
                error.retryable || (error.kind == ErrorKind::Closed && !stream.answered);
            stream.fail(error);
        }
        self.conn.close();
    }

    fn reset(&mut self, id: u32, code: u32) {
        frame(&mut self.out, RST_STREAM, 0, id, &code.to_be_bytes());
    }

    /// A frame for a stream this client does not track: legal for a stream
    /// that already ended, a protocol error for one never opened.
    fn check_idle(&self, id: u32) -> Result<(), ConnectionError> {
        if id.is_multiple_of(2) || id >= self.next_stream_id {
            Err(ConnectionError::Protocol(
                PROTOCOL_ERROR,
                format!("frame for idle stream {id}"),
            ))
        } else {
            Ok(())
        }
    }

    fn handle(
        &mut self,
        incoming: Frame,
        streams: &mut HashMap<u32, Stream>,
        config: &Config,
    ) -> Result<(), ConnectionError> {
        if let Some((expected, _, _)) = &self.continuation
            && (incoming.kind != CONTINUATION || incoming.stream != *expected)
        {
            return Err(ConnectionError::Protocol(
                PROTOCOL_ERROR,
                "expected a CONTINUATION frame".into(),
            ));
        }
        let Frame {
            kind,
            flags,
            stream: id,
            payload,
        } = incoming;
        match kind {
            DATA => self.on_data(id, flags, payload, streams, config),
            HEADERS => {
                if id == 0 {
                    return Err(protocol_error("HEADERS on stream 0"));
                }
                let mut block = strip_padding(flags, payload)?;
                if flags & PRIORITY_FLAG != 0 {
                    if block.len() < 5 {
                        return Err(ConnectionError::Protocol(
                            FRAME_SIZE_ERROR,
                            "HEADERS priority field is truncated".into(),
                        ));
                    }
                    block.drain(..5);
                }
                let end_stream = flags & END_STREAM != 0;
                if flags & END_HEADERS != 0 {
                    self.on_header_block(id, end_stream, &block, streams)
                } else {
                    self.continuation = Some((id, end_stream, block));
                    Ok(())
                }
            }
            CONTINUATION => {
                let (stream, end_stream, mut block) = self
                    .continuation
                    .take()
                    .ok_or_else(|| protocol_error("unexpected CONTINUATION frame"))?;
                block.extend_from_slice(&payload);
                if block.len() > self.max_header_bytes.saturating_mul(2).max(MAX_FRAME_SIZE) {
                    return Err(protocol_error("header block is too large"));
                }
                if flags & END_HEADERS != 0 {
                    self.on_header_block(stream, end_stream, &block, streams)
                } else {
                    self.continuation = Some((stream, end_stream, block));
                    Ok(())
                }
            }
            PRIORITY => {
                if id == 0 {
                    return Err(protocol_error("PRIORITY on stream 0"));
                }
                Ok(())
            }
            RST_STREAM => {
                if id == 0 {
                    return Err(protocol_error("RST_STREAM on stream 0"));
                }
                if payload.len() != 4 {
                    return Err(ConnectionError::Protocol(
                        FRAME_SIZE_ERROR,
                        "RST_STREAM must be 4 bytes".into(),
                    ));
                }
                let code = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                match streams.get_mut(&id) {
                    Some(stream) => {
                        let mut error = HttpError::new(
                            ErrorKind::Closed,
                            format!("the server reset the stream ({})", code_name(code)),
                        );
                        error.retryable = code == REFUSED_STREAM;
                        stream.fail(error);
                        Ok(())
                    }
                    None => self.check_idle(id),
                }
            }
            SETTINGS => self.on_settings(id, flags, &payload, streams),
            PUSH_PROMISE => Err(protocol_error("PUSH_PROMISE although push is disabled")),
            PING => {
                if id != 0 {
                    return Err(protocol_error("PING on a stream"));
                }
                if payload.len() != 8 {
                    return Err(ConnectionError::Protocol(
                        FRAME_SIZE_ERROR,
                        "PING must be 8 bytes".into(),
                    ));
                }
                if flags & ACK == 0 {
                    frame(&mut self.out, PING, ACK, 0, &payload);
                }
                Ok(())
            }
            GOAWAY => {
                if id != 0 {
                    return Err(protocol_error("GOAWAY on a stream"));
                }
                if payload.len() < 8 {
                    return Err(ConnectionError::Protocol(
                        FRAME_SIZE_ERROR,
                        "GOAWAY is truncated".into(),
                    ));
                }
                let last = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]])
                    & MAX_STREAM_ID;
                let code = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                let detail =
                    String::from_utf8_lossy(&payload[8..payload.len().min(8 + 200)]).into_owned();
                self.goaway = Some(last);
                for (stream_id, stream) in streams.iter_mut() {
                    if *stream_id > last {
                        let mut error = HttpError::new(
                            ErrorKind::Closed,
                            format!(
                                "the server is closing the connection ({}{}) and did not process the request",
                                code_name(code),
                                if detail.is_empty() {
                                    String::new()
                                } else {
                                    format!(": {detail}")
                                }
                            ),
                        );
                        error.retryable = true;
                        stream.fail(error);
                    }
                }
                Ok(())
            }
            WINDOW_UPDATE => {
                if payload.len() != 4 {
                    return Err(ConnectionError::Protocol(
                        FRAME_SIZE_ERROR,
                        "WINDOW_UPDATE must be 4 bytes".into(),
                    ));
                }
                let increment = i64::from(
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]])
                        & MAX_STREAM_ID,
                );
                if id == 0 {
                    if increment == 0 {
                        return Err(protocol_error("WINDOW_UPDATE with a zero increment"));
                    }
                    self.send_window += increment;
                    if self.send_window > MAX_WINDOW {
                        return Err(ConnectionError::Protocol(
                            FLOW_CONTROL_ERROR,
                            "connection window overflow".into(),
                        ));
                    }
                    return Ok(());
                }
                match streams.get_mut(&id) {
                    Some(stream) => {
                        stream.send_window += increment;
                        if increment == 0 || stream.send_window > MAX_WINDOW {
                            let code = if increment == 0 {
                                PROTOCOL_ERROR
                            } else {
                                FLOW_CONTROL_ERROR
                            };
                            stream.fail(HttpError::new(
                                ErrorKind::Protocol,
                                format!("invalid WINDOW_UPDATE ({})", code_name(code)),
                            ));
                            self.reset(id, code);
                        }
                        Ok(())
                    }
                    None => self.check_idle(id),
                }
            }
            // Unknown frame types are ignored (§4.1).
            _ => Ok(()),
        }
    }

    fn on_data(
        &mut self,
        id: u32,
        flags: u8,
        payload: Vec<u8>,
        streams: &mut HashMap<u32, Stream>,
        config: &Config,
    ) -> Result<(), ConnectionError> {
        if id == 0 {
            return Err(protocol_error("DATA on stream 0"));
        }
        // The whole frame, padding included, counts against flow control.
        let counted = payload.len() as u32;
        self.recv_pending += counted;
        let data = strip_padding(flags, payload)?;
        match streams.get_mut(&id) {
            Some(stream) if !stream.done => {
                stream.answered = true;
                let Some(response) = stream.response.as_mut() else {
                    stream.fail(HttpError::new(
                        ErrorKind::Protocol,
                        "the server sent DATA before the response headers",
                    ));
                    self.reset(id, PROTOCOL_ERROR);
                    return self.update_connection_window();
                };
                response.wire_bytes += data.len() as u64;
                let room = config.max_body_bytes.saturating_sub(response.body.len());
                if data.len() > room {
                    response.body.extend_from_slice(&data[..room]);
                    response.truncated = true;
                    // We have what we keep; stop the transfer.
                    stream.done = true;
                    self.reset(id, CANCEL);
                    return self.update_connection_window();
                }
                response.body.extend_from_slice(&data);
                stream.recv_pending += counted;
                if flags & END_STREAM != 0 {
                    self.finish(id, stream);
                } else if stream.recv_pending >= STREAM_WINDOW / 2 {
                    frame(
                        &mut self.out,
                        WINDOW_UPDATE,
                        0,
                        id,
                        &stream.recv_pending.to_be_bytes(),
                    );
                    stream.recv_pending = 0;
                }
            }
            Some(_) => {}
            None => self.check_idle(id)?,
        }
        self.update_connection_window()
    }

    fn update_connection_window(&mut self) -> Result<(), ConnectionError> {
        if self.recv_pending >= CONNECTION_WINDOW / 2 {
            frame(
                &mut self.out,
                WINDOW_UPDATE,
                0,
                0,
                &self.recv_pending.to_be_bytes(),
            );
            self.recv_pending = 0;
        }
        Ok(())
    }

    fn on_settings(
        &mut self,
        id: u32,
        flags: u8,
        payload: &[u8],
        streams: &mut HashMap<u32, Stream>,
    ) -> Result<(), ConnectionError> {
        if id != 0 {
            return Err(protocol_error("SETTINGS on a stream"));
        }
        if flags & ACK != 0 {
            return if payload.is_empty() {
                Ok(())
            } else {
                Err(ConnectionError::Protocol(
                    FRAME_SIZE_ERROR,
                    "SETTINGS acknowledgement with a payload".into(),
                ))
            };
        }
        if !payload.len().is_multiple_of(6) {
            return Err(ConnectionError::Protocol(
                FRAME_SIZE_ERROR,
                "SETTINGS length is not a multiple of 6".into(),
            ));
        }
        for setting in payload.chunks(6) {
            let key = u16::from_be_bytes([setting[0], setting[1]]);
            let value = u32::from_be_bytes([setting[2], setting[3], setting[4], setting[5]]);
            match key {
                SETTINGS_ENABLE_PUSH if value > 1 => {
                    return Err(protocol_error("invalid SETTINGS_ENABLE_PUSH"));
                }
                SETTINGS_MAX_CONCURRENT_STREAMS => {
                    self.peer_max_streams = value as usize;
                }
                SETTINGS_INITIAL_WINDOW_SIZE => {
                    let value = i64::from(value);
                    if value > MAX_WINDOW {
                        return Err(ConnectionError::Protocol(
                            FLOW_CONTROL_ERROR,
                            "SETTINGS_INITIAL_WINDOW_SIZE is too large".into(),
                        ));
                    }
                    let delta = value - self.peer_initial_window;
                    self.peer_initial_window = value;
                    for stream in streams.values_mut() {
                        stream.send_window += delta;
                        if stream.send_window > MAX_WINDOW {
                            return Err(ConnectionError::Protocol(
                                FLOW_CONTROL_ERROR,
                                "stream window overflow".into(),
                            ));
                        }
                    }
                }
                SETTINGS_MAX_FRAME_SIZE => {
                    if !(16_384..=16_777_215).contains(&value) {
                        return Err(protocol_error("invalid SETTINGS_MAX_FRAME_SIZE"));
                    }
                    self.peer_max_frame = value as usize;
                }
                // Header table size: this encoder never uses the dynamic
                // table. Max header list size: advisory. Others: unknown.
                _ => {}
            }
        }
        frame(&mut self.out, SETTINGS, ACK, 0, &[]);
        Ok(())
    }

    fn on_header_block(
        &mut self,
        id: u32,
        end_stream: bool,
        block: &[u8],
        streams: &mut HashMap<u32, Stream>,
    ) -> Result<(), ConnectionError> {
        // Decode even for streams we no longer track: the dynamic table must
        // stay in step with the server's encoder.
        let fields = self
            .decoder
            .decode(block)
            .map_err(|e| ConnectionError::Protocol(COMPRESSION_ERROR, e.to_string()))?;
        let Some(stream) = streams.get_mut(&id).filter(|s| !s.done) else {
            return if streams.contains_key(&id) {
                Ok(())
            } else {
                self.check_idle(id)
            };
        };
        stream.answered = true;
        let mut status = None;
        let mut headers = Vec::new();
        let mut malformed = None;
        for (name, value) in fields {
            let name = String::from_utf8_lossy(&name).into_owned();
            let value = String::from_utf8_lossy(&value).into_owned();
            if let Some(pseudo) = name.strip_prefix(':') {
                if pseudo == "status"
                    && status.is_none()
                    && headers.is_empty()
                    && stream.response.is_none()
                {
                    status = Some(value);
                } else {
                    malformed = Some(format!("unexpected pseudo-header :{pseudo}"));
                }
            } else if name.bytes().any(|b| b.is_ascii_uppercase()) {
                malformed = Some(format!("upper-case header name {name}"));
            } else {
                headers.push((name, value));
            }
        }
        if let Some(reason) = malformed {
            stream.fail(HttpError::new(
                ErrorKind::Protocol,
                format!("malformed HTTP/2 response: {reason}"),
            ));
            self.reset(id, PROTOCOL_ERROR);
            return Ok(());
        }
        if let Some(response) = stream.response.as_mut() {
            // A second header block is the trailer section.
            if !end_stream {
                stream.fail(HttpError::new(
                    ErrorKind::Protocol,
                    "trailers without END_STREAM",
                ));
                self.reset(id, PROTOCOL_ERROR);
                return Ok(());
            }
            response.trailers = headers;
            self.finish(id, stream);
            return Ok(());
        }
        let status = status
            .as_deref()
            .filter(|s| s.len() == 3)
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|s| (100..=999).contains(s));
        let Some(status) = status else {
            stream.fail(HttpError::new(
                ErrorKind::Protocol,
                "malformed HTTP/2 response: missing or invalid :status",
            ));
            self.reset(id, PROTOCOL_ERROR);
            return Ok(());
        };
        if (100..200).contains(&status) {
            if end_stream {
                stream.fail(HttpError::new(
                    ErrorKind::Protocol,
                    "an informational response ended the stream",
                ));
                self.reset(id, PROTOCOL_ERROR);
            } else if stream.interim.len() < 16 {
                stream.interim.push(status);
            }
            return Ok(());
        }
        stream.response = Some(RawResponse {
            version: Some(Version::Http2),
            status,
            headers,
            ..RawResponse::default()
        });
        if end_stream {
            self.finish(id, stream);
        }
        Ok(())
    }

    /// The server ended the stream: check the body against Content-Length
    /// and stop sending a request body it no longer wants.
    fn finish(&mut self, id: u32, stream: &mut Stream) {
        if let Some(response) = &stream.response {
            let bodiless = stream.head || matches!(response.status, 204 | 304);
            if !bodiless
                && !response.truncated
                && let Some(length) = response
                    .header("content-length")
                    .and_then(|v| v.trim().parse::<u64>().ok())
                && length != response.wire_bytes
            {
                stream.fail(HttpError::new(
                    ErrorKind::Protocol,
                    format!(
                        "the body has {} bytes but Content-Length says {length}",
                        response.wire_bytes
                    ),
                ));
                return;
            }
        }
        if !stream.end_sent {
            // The response is complete before the request body (§8.1).
            self.reset(id, CANCEL);
            stream.end_sent = true;
        }
        stream.done = true;
    }
}

fn protocol_error(message: &str) -> ConnectionError {
    ConnectionError::Protocol(PROTOCOL_ERROR, message.into())
}

/// Remove the padding of a PADDED frame.
fn strip_padding(flags: u8, mut payload: Vec<u8>) -> Result<Vec<u8>, ConnectionError> {
    if flags & PADDED == 0 {
        return Ok(payload);
    }
    let pad = usize::from(
        *payload
            .first()
            .ok_or_else(|| protocol_error("padded frame without a pad length"))?,
    );
    if pad >= payload.len() {
        return Err(protocol_error("padding exceeds the frame"));
    }
    payload.truncate(payload.len() - pad);
    payload.remove(0);
    Ok(payload)
}

/// Append one frame to `out`.
pub(crate) fn frame(out: &mut Vec<u8>, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
    let length = payload.len() as u32;
    out.extend_from_slice(&length.to_be_bytes()[1..]);
    out.push(kind);
    out.push(flags);
    out.extend_from_slice(&(stream & MAX_STREAM_ID).to_be_bytes());
    out.extend_from_slice(payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_padding_and_rejects_oversized_padding() {
        assert_eq!(
            strip_padding(PADDED, vec![2, b'a', b'b', 0, 0]).ok(),
            Some(b"ab".to_vec())
        );
        assert!(strip_padding(PADDED, vec![5, b'a']).is_err());
        assert!(strip_padding(PADDED, vec![]).is_err());
        assert_eq!(strip_padding(0, vec![1, 2]).ok(), Some(vec![1, 2]));
    }

    #[test]
    fn encodes_frame_headers() {
        let mut out = Vec::new();
        frame(&mut out, SETTINGS, ACK, 0, &[]);
        assert_eq!(out, [0, 0, 0, 4, 1, 0, 0, 0, 0]);
        out.clear();
        frame(&mut out, DATA, END_STREAM, 3, b"hi");
        assert_eq!(out, [0, 0, 2, 0, 1, 0, 0, 0, 3, b'h', b'i']);
        assert_eq!(code_name(0), "NO_ERROR");
        assert_eq!(code_name(0xff), "0xff");
    }
}
