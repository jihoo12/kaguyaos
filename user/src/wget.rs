#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    std::terminate_task(99);
}

mod std;

// wget [http://]host[:port][/path]
//
// Fetches the URL with HTTP/1.0 and writes the body to STDOUT, so it can be
// redirected or piped. Progress and errors go to STDERR. Follows up to
// MAX_REDIRECTS http:// redirects; exits 1 unless the final status is 2xx.

const MAX_REDIRECTS: usize = 5;
const MAX_URL: usize = 512;
const HEADER_CAP: usize = 4096;

fn eprint(s: &str) {
    std::write(std::STDERR, s.as_bytes());
}

fn eprint_num(mut value: usize) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    std::write(std::STDERR, &buf[i..]);
}

fn fail(message: &str) -> ! {
    eprint("wget: ");
    eprint(message);
    eprint("\n");
    std::terminate_task(1);
}

struct Url<'a> {
    host: &'a str,
    port: u16,
    /// Path and query, without any fragment. May be empty or start with '?'.
    target: &'a str,
}

fn parse_url(url: &str) -> Result<Url<'_>, &'static str> {
    let rest = if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else if url.starts_with("https://") {
        return Err("https is not supported");
    } else if url.contains("://") {
        return Err("only http:// URLs are supported");
    } else {
        url
    };
    let rest = rest.split('#').next().unwrap_or("");
    let (authority, target) = rest.split_at(rest.find(['/', '?']).unwrap_or(rest.len()));
    let (host, port) = match authority.rfind(':') {
        Some(i) => {
            let port = authority[i + 1..].parse::<u16>().map_err(|_| "bad port")?;
            (&authority[..i], port)
        }
        None => (authority, 80),
    };
    if host.is_empty() || port == 0 {
        return Err("missing host");
    }
    Ok(Url { host, port, target })
}

/// ":port", or nothing for the default HTTP port.
fn port_suffix(port: u16, buf: &mut [u8; 6]) -> &str {
    if port == 80 {
        return "";
    }
    let mut i = buf.len();
    let mut value = port;
    loop {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    i -= 1;
    buf[i] = b':';
    unsafe { core::str::from_utf8_unchecked(&buf[i..]) }
}

struct UrlBuf {
    bytes: [u8; MAX_URL],
    len: usize,
}

impl UrlBuf {
    fn new() -> Self {
        Self { bytes: [0; MAX_URL], len: 0 }
    }

    fn push(&mut self, piece: &[u8]) {
        if self.len + piece.len() > MAX_URL {
            fail("URL too long");
        }
        self.bytes[self.len..self.len + piece.len()].copy_from_slice(piece);
        self.len += piece.len();
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or_else(|_| fail("URL is not UTF-8"))
    }
}

/// Append an absolute path with "." and ".." segments removed.
fn push_normalized_path(out: &mut UrlBuf, path: &[u8]) {
    let mut segments: [&[u8]; 64] = [&[]; 64];
    let mut count = 0usize;
    let mut directory = false;
    let parts = path.strip_prefix(b"/").unwrap_or(path).split(|&b| b == b'/');
    for segment in parts {
        directory = matches!(segment, b"." | b"..");
        match segment {
            b"." => {}
            b".." => count = count.saturating_sub(1),
            _ => {
                if count == segments.len() {
                    fail("URL has too many path segments");
                }
                segments[count] = segment;
                count += 1;
            }
        }
    }
    out.push(b"/");
    for (i, segment) in segments[..count].iter().enumerate() {
        if i > 0 {
            out.push(b"/");
        }
        out.push(segment);
    }
    if directory && count > 0 {
        out.push(b"/");
    }
}

fn has_scheme(reference: &[u8]) -> bool {
    let end = reference.iter().position(|&b| matches!(b, b'/' | b'?' | b'#')).unwrap_or(reference.len());
    match reference[..end].iter().position(|&b| b == b':') {
        Some(colon) => {
            colon > 0
                && reference[0].is_ascii_alphabetic()
                && reference[..colon].iter().all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
        }
        None => false,
    }
}

/// Resolve a Location header against the URL that returned it.
fn resolve(base: &Url, location: &[u8]) -> UrlBuf {
    let mut out = UrlBuf::new();
    if has_scheme(location) {
        out.push(location);
        return out;
    }
    if location.starts_with(b"//") {
        out.push(b"http:");
        out.push(location);
        return out;
    }

    let mut port = [0u8; 6];
    out.push(b"http://");
    out.push(base.host.as_bytes());
    out.push(port_suffix(base.port, &mut port).as_bytes());

    let base_target = base.target.as_bytes();
    let base_path = base_target.split(|&b| b == b'?').next().unwrap_or(&[]);
    let base_path: &[u8] = if base_path.starts_with(b"/") { base_path } else { b"/" };
    let location = location.split(|&b| b == b'#').next().unwrap_or(&[]);

    let mut merged = UrlBuf::new();
    match location.first() {
        None => merged.push(if base_target.is_empty() { b"/" } else { base_target }),
        Some(b'/') => merged.push(location),
        Some(b'?') => {
            merged.push(base_path);
            merged.push(location);
        }
        Some(_) => {
            let dir = base_path.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1);
            merged.push(&base_path[..dir]);
            merged.push(location);
        }
    }
    let merged = &merged.bytes[..merged.len];
    let query = merged.iter().position(|&b| b == b'?').unwrap_or(merged.len());
    push_normalized_path(&mut out, &merged[..query]);
    out.push(&merged[query..]);
    out
}

fn parse_ip(s: &str) -> Option<[u8; 4]> {
    let mut ip = [0u8; 4];
    let mut parts = s.split('.');
    for byte in ip.iter_mut() {
        *byte = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(ip)
}

fn write_all(fd: usize, mut data: &[u8]) -> bool {
    while !data.is_empty() {
        let n = std::write(fd, data);
        if n <= 0 {
            return false;
        }
        data = &data[n as usize..];
    }
    true
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// The value of header `name` (case-insensitive) in a raw header block.
fn header<'a>(headers: &'a [u8], name: &str) -> Option<&'a [u8]> {
    for line in headers.split(|&b| b == b'\n').skip(1) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        if line[..colon].eq_ignore_ascii_case(name.as_bytes()) {
            return Some(line[colon + 1..].trim_ascii());
        }
    }
    None
}

fn parse_usize(bytes: &[u8]) -> Option<usize> {
    core::str::from_utf8(bytes).ok()?.parse().ok()
}

enum Outcome {
    Done(bool),
    Redirect(usize),
}

/// Fetch `url`. A redirect's target is copied into `next` (its length returned).
fn fetch(url: &Url, next: &mut [u8; MAX_URL]) -> Outcome {
    let ip = match parse_ip(url.host) {
        Some(ip) => ip,
        None => match std::dns_resolve(url.host) {
            Some(ip) => ip,
            None => fail("cannot resolve host"),
        },
    };
    eprint("Connecting to ");
    eprint(url.host);
    eprint(":");
    eprint_num(url.port as usize);
    eprint("...\n");

    let fd = std::socket(std::SOCK_TCP);
    if fd < 0 {
        fail("cannot create socket");
    }
    let fd = fd as usize;
    match std::connect(fd, ip, url.port) {
        0 => {}
        -14 => fail("connection refused"),
        -11 => fail("connection timed out"),
        _ => fail("cannot connect"),
    }

    let mut port = [0u8; 6];
    let slash = if url.target.starts_with('/') { "" } else { "/" };
    let parts = [
        "GET ", slash, url.target, " HTTP/1.0\r\nHost: ", url.host, port_suffix(url.port, &mut port),
        "\r\nUser-Agent: kaguyaOS-wget\r\nAccept: */*\r\nConnection: close\r\n\r\n",
    ];
    for part in parts {
        if !write_all(fd, part.as_bytes()) {
            fail("cannot send request");
        }
    }

    // Read until the end of the headers; the rest of the buffer is body.
    let mut buf = [0u8; HEADER_CAP];
    let mut len = 0;
    let header_end = loop {
        if let Some(i) = find(&buf[..len], b"\r\n\r\n") {
            break i + 4;
        }
        if len == buf.len() {
            fail("response headers too long");
        }
        let n = std::read(fd, &mut buf[len..]);
        if n <= 0 {
            fail("connection closed before the response headers");
        }
        len += n as usize;
    };
    let headers = &buf[..header_end];
    let status_line = headers.split(|&b| b == b'\r').next().unwrap_or(&[]);
    let status = status_line
        .split(|&b| b == b' ')
        .nth(1)
        .and_then(parse_usize)
        .unwrap_or_else(|| fail("malformed status line"));
    eprint("HTTP ");
    std::write(std::STDERR, status_line.get(9..).unwrap_or(&[]));
    eprint("\n");

    if (300..400).contains(&status) {
        if let Some(location) = header(headers, "Location") {
            if location.len() > MAX_URL {
                fail("redirect target too long");
            }
            next[..location.len()].copy_from_slice(location);
            std::close(fd);
            return Outcome::Redirect(location.len());
        }
    }

    let expected = header(headers, "Content-Length").and_then(parse_usize);
    let mut total = len - header_end;
    if !write_all(std::STDOUT, &buf[header_end..len]) {
        fail("cannot write output");
    }
    loop {
        let n = std::read(fd, &mut buf);
        if n < 0 {
            fail("connection reset");
        }
        if n == 0 {
            break;
        }
        if !write_all(std::STDOUT, &buf[..n as usize]) {
            fail("cannot write output");
        }
        total += n as usize;
    }
    std::close(fd);

    eprint_num(total);
    eprint(" bytes received\n");
    if expected.is_some_and(|expected| expected != total) {
        fail("body shorter than Content-Length");
    }
    Outcome::Done((200..300).contains(&status))
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(args_ptr: *const u8, args_len: usize) -> ! {
    let args = unsafe { core::slice::from_raw_parts(args_ptr, args_len) };
    let Ok(args) = core::str::from_utf8(args) else {
        fail("arguments are not UTF-8");
    };
    let arg = args.trim();
    if arg.is_empty() || arg.contains(' ') || arg.len() > MAX_URL {
        eprint("Usage: wget [http://]host[:port][/path]\n");
        std::terminate_task(1);
    }

    let mut current = UrlBuf::new();
    current.push(arg.as_bytes());
    for _ in 0..=MAX_REDIRECTS {
        let url = parse_url(current.as_str()).unwrap_or_else(|message| fail(message));
        let mut next = [0u8; MAX_URL];
        match fetch(&url, &mut next) {
            Outcome::Done(ok) => std::terminate_task(if ok { 0 } else { 1 }),
            Outcome::Redirect(len) => {
                let target = resolve(&url, &next[..len]);
                current = target;
                eprint("Redirected to ");
                eprint(current.as_str());
                eprint("\n");
            }
        }
    }
    fail("too many redirects");
}
