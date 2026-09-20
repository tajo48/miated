use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::OnceLock;
use std::thread;

/* Every asset the site consists of, baked into the binary. */
const INDEX: &[u8] = include_bytes!("../index.html");
const SW_JS: &[u8] = include_bytes!("../sw.js");
const MANIFEST: &[u8] = include_bytes!("../manifest.json");
const FAVICON_SVG: &[u8] = include_bytes!("../favicon.svg");
const FAVICON_PNG: &[u8] = include_bytes!("../favicon-32.png");
const APPLE_TOUCH: &[u8] = include_bytes!("../apple-touch-icon.png");
const ICON_192: &[u8] = include_bytes!("../icon-192.png");
const ICON_512: &[u8] = include_bytes!("../icon-512.png");
const ICON_192_MASKABLE: &[u8] = include_bytes!("../icon-192-maskable.png");
const ICON_512_MASKABLE: &[u8] = include_bytes!("../icon-512-maskable.png");
const NOT_FOUND: &[u8] = b"not found\n";

const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_PORT: u16 = 3004;

const USAGE: &str = "\
miated — serves the miated pwa

USAGE: miated [OPTIONS] [PORT]

ARGS:
    PORT                Port to listen on (default: 3004)

OPTIONS:
    -p, --port <PORT>   Port to listen on
    -H, --host <HOST>   Address to bind (default: 0.0.0.0, all interfaces)
    -h, --help          Print this help

HOST/PORT environment variables provide the defaults; arguments win.";

/* Stamp for the service worker version. GitHub Actions replaces it with the
commit sha when deploying; locally we replace it with a hash of the served
assets so every rebuild yields a fresh sw.js the browser must re-fetch. */
const SW_VERSION_PLACEHOLDER: &str = "@@VERSION@@";

static SW_JS_SERVED: OnceLock<Vec<u8>> = OnceLock::new();

fn sw_js_served() -> &'static [u8] {
    SW_JS_SERVED.get_or_init(|| {
        // FNV-1a over every baked asset: any content change → new version.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for asset in [
            INDEX,
            SW_JS,
            MANIFEST,
            FAVICON_SVG,
            FAVICON_PNG,
            APPLE_TOUCH,
            ICON_192,
            ICON_512,
            ICON_192_MASKABLE,
            ICON_512_MASKABLE,
        ] {
            for &byte in asset {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        let version = format!("{hash:016x}");
        std::str::from_utf8(SW_JS)
            .expect("sw.js must be valid UTF-8")
            .replace(SW_VERSION_PLACEHOLDER, &version)
            .into_bytes()
    })
}

fn main() -> std::io::Result<()> {
    let (host, port) = parse_args();

    let listener = match TcpListener::bind((host.as_str(), port)) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("miated: failed to bind {host}:{port}: {err}");
            return Err(err);
        }
    };
    // 0.0.0.0 listens on every interface; show the LAN IP as the URL to use.
    let display_host = if host == "0.0.0.0" {
        local_ip().unwrap_or(host)
    } else {
        host
    };
    eprintln!("miated listening on http://{display_host}:{port}");
    if display_host != "localhost" && !display_host.starts_with("127.") {
        eprintln!(
            "note: over http://LAN-IP, Chrome blocks tab capture & GPS (secure-context only). \
Use http://localhost:{port} on this machine, or deploy for https."
        );
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || handle(stream));
            }
            Err(err) => eprintln!("accept failed: {err}"),
        }
    }

    Ok(())
}

fn local_ip() -> Option<String> {
    // "Connect" a UDP socket to a public address (no packets sent) to learn
    // which local IP faces the outside world.
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

fn parse_args() -> (String, u16) {
    let mut host = std::env::var("HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let mut port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "-p" | "--port" => match args.next() {
                Some(v) => port = parse_port(&v),
                None => fail("missing value for --port"),
            },
            "-H" | "--host" => match args.next() {
                Some(v) => host = v,
                None => fail("missing value for --host"),
            },
            other => {
                if let Some(v) = other.strip_prefix("--port=") {
                    port = parse_port(v);
                } else if let Some(v) = other.strip_prefix("--host=") {
                    host = v.to_string();
                } else {
                    port = parse_port(other);
                }
            }
        }
    }

    (host, port)
}

fn parse_port(s: &str) -> u16 {
    s.parse()
        .unwrap_or_else(|_| fail(&format!("invalid port: {s:?}")))
}

fn fail(msg: &str) -> ! {
    eprintln!("miated: {msg}\n\n{USAGE}");
    std::process::exit(2);
}

fn handle(mut stream: TcpStream) {
    let mut request = Vec::new();
    let mut buf = [0u8; 4096];

    // Requests here are tiny GETs; read until the end of the headers.
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return,
        };
        request.extend_from_slice(&buf[..n]);
        if request.len() > 65_536 || request.windows(4).any(|w| w == b"\r\n\r\n".as_slice()) {
            break;
        }
    }

    let head = String::from_utf8_lossy(&request);
    let mut parts = head.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();

    let (status, content_type, body): (&str, &str, &[u8]) = if method == "GET" || method == "HEAD" {
        match path {
            "/" | "/index.html" => ("200 OK", "text/html; charset=utf-8", INDEX),
            "/sw.js" => ("200 OK", "text/javascript; charset=utf-8", sw_js_served()),
            "/manifest.json" => (
                "200 OK",
                "application/manifest+json; charset=utf-8",
                MANIFEST,
            ),
            "/favicon.svg" => ("200 OK", "image/svg+xml", FAVICON_SVG),
            "/favicon-32.png" => ("200 OK", "image/png", FAVICON_PNG),
            "/apple-touch-icon.png" => ("200 OK", "image/png", APPLE_TOUCH),
            "/icon-192.png" => ("200 OK", "image/png", ICON_192),
            "/icon-512.png" => ("200 OK", "image/png", ICON_512),
            "/icon-192-maskable.png" => ("200 OK", "image/png", ICON_192_MASKABLE),
            "/icon-512-maskable.png" => ("200 OK", "image/png", ICON_512_MASKABLE),
            _ => ("404 Not Found", "text/plain; charset=utf-8", NOT_FOUND),
        }
    } else {
        ("404 Not Found", "text/plain; charset=utf-8", NOT_FOUND)
    };

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    if method != "HEAD" {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}
