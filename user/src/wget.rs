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
    path: &'a str,
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
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
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
    Ok(Url { host, port, path })
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

    let parts = [
        "GET ", url.path, " HTTP/1.0\r\nHost: ", url.host,
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

    let mut current = [0u8; MAX_URL];
    current[..arg.len()].copy_from_slice(arg.as_bytes());
    let mut current_len = arg.len();
    for _ in 0..=MAX_REDIRECTS {
        let text = core::str::from_utf8(&current[..current_len]).unwrap_or_else(|_| fail("bad redirect"));
        let url = parse_url(text).unwrap_or_else(|message| fail(message));
        let mut next = [0u8; MAX_URL];
        match fetch(&url, &mut next) {
            Outcome::Done(ok) => std::terminate_task(if ok { 0 } else { 1 }),
            Outcome::Redirect(len) => {
                // A relative Location keeps the scheme, host and port.
                if next[0] == b'/' {
                    let mut joined = [0u8; MAX_URL];
                    let mut n = 0;
                    for piece in [b"http://".as_slice(), url.host.as_bytes(), b":"] {
                        joined[n..n + piece.len()].copy_from_slice(piece);
                        n += piece.len();
                    }
                    let mut port = [0u8; 5];
                    let mut i = port.len();
                    let mut value = url.port;
                    loop {
                        i -= 1;
                        port[i] = b'0' + (value % 10) as u8;
                        value /= 10;
                        if value == 0 {
                            break;
                        }
                    }
                    let tail = [&port[i..], &next[..len]];
                    for piece in tail {
                        if n + piece.len() > MAX_URL {
                            fail("redirect target too long");
                        }
                        joined[n..n + piece.len()].copy_from_slice(piece);
                        n += piece.len();
                    }
                    current = joined;
                    current_len = n;
                } else {
                    current = next;
                    current_len = len;
                }
                eprint("Redirected to ");
                std::write(std::STDERR, &current[..current_len]);
                eprint("\n");
            }
        }
    }
    fail("too many redirects");
}
