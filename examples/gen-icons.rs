/* One-shot generator for the miated PNG icons — no dependencies, so it lives
as a cargo-discovered bin and runs with: cargo run --bin gen-icons
It renders the favicon artwork (four rounded equalizer bars on the Frappé
crust background) at every manifest size, 3× supersampled, and writes it
with a minimal PNG encoder (RGBA8, zlib stored blocks). */

use std::fs;
use std::path::PathBuf;

/* ---------- tiny PNG encoder ---------- */

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    let mut chunks = raw.chunks(65_535).peekable();
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        out.push(u32::from(last) as u8);
        let len = chunk.len() as u16;
        let lb = len.to_le_bytes();
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&[!lb[0], !lb[1]]);
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let stride = w as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0); // filter: none
        raw.extend_from_slice(&rgba[y * stride..(y + 1) * stride]);
    }

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    push_chunk(&mut png, b"IHDR", &ihdr);
    push_chunk(&mut png, b"IDAT", &zlib_stored(&raw));
    push_chunk(&mut png, b"IEND", &[]);
    png
}

/* ---------- rendering ---------- */

const BG: [f32; 3] = [
    0x23 as f32 / 255.0,
    0x26 as f32 / 255.0,
    0x34 as f32 / 255.0,
]; // crust

/* Equalizer bars, mirroring favicon.svg: (color, cx, cy, half-width, half-height)
in 128-design-unit coordinates centered on the canvas (range −64..64).
rx = half-width → capsule ends. */
const BARS: [([f32; 3], f32, f32, f32, f32); 4] = [
    (
        [
            0xe7 as f32 / 255.0,
            0x82 as f32 / 255.0,
            0x84 as f32 / 255.0,
        ],
        -39.0,
        0.0,
        7.0,
        22.0,
    ),
    (
        [
            0xe5 as f32 / 255.0,
            0xc8 as f32 / 255.0,
            0x90 as f32 / 255.0,
        ],
        -13.0,
        0.0,
        7.0,
        38.0,
    ),
    (
        [
            0xa6 as f32 / 255.0,
            0xd1 as f32 / 255.0,
            0x89 as f32 / 255.0,
        ],
        13.0,
        0.0,
        7.0,
        29.0,
    ),
    (
        [
            0x8c as f32 / 255.0,
            0xaa as f32 / 255.0,
            0xee as f32 / 255.0,
        ],
        39.0,
        0.0,
        7.0,
        44.0,
    ),
];

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Coverage of a rounded rect centered at (cx, cy) with corner radius `rad`,
/// via the standard rounded-rect signed distance. A 1-design-unit feather
/// keeps the 3× supersampled edge smooth.
fn rounded_rect_cov(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, rad: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - rad);
    let qy = (py - cy).abs() - (hh - rad);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let inside = qx.max(qy).min(0.0);
    (0.5 - (outside + inside - rad)).clamp(0.0, 1.0)
}

fn sample(ux: f32, uy: f32, cov_bg: f32) -> [f32; 4] {
    // Background color (inside the rounded square it is always crust).
    let mut color = BG;

    for &(c, cx, cy, hw, hh) in &BARS {
        let cov = rounded_rect_cov(ux, uy, cx, cy, hw, hh, hw);
        if cov > 0.0 {
            color = mix(color, c, cov);
        }
    }

    [color[0], color[1], color[2], cov_bg]
}

fn render(size: u32, maskable: bool) -> Vec<u8> {
    const SS: usize = 3;
    let big = size as usize * SS;
    let mut img = vec![0u8; big * big * 4];

    // Maskable icons need the artwork inside the center ~80% safe zone,
    // while the background bleeds to a full square.
    let zoom = if maskable { 0.74 } else { 1.0 };

    for y in 0..big {
        for x in 0..big {
            let dx = (x as f32 + 0.5) * 128.0 / big as f32 - 64.0;
            let dy = (y as f32 + 0.5) * 128.0 / big as f32 - 64.0;
            let cov_bg = if maskable {
                1.0
            } else {
                rounded_rect_cov(dx, dy, 0.0, 0.0, 64.0, 64.0, 28.0)
            };
            let [r, g, b, a] = sample(dx / zoom, dy / zoom, cov_bg);
            let o = (y * big + x) * 4;
            img[o] = (r * 255.0).round() as u8;
            img[o + 1] = (g * 255.0).round() as u8;
            img[o + 2] = (b * 255.0).round() as u8;
            img[o + 3] = (a * 255.0).round() as u8;
        }
    }

    // Box-downsample, premultiplied so transparent edges don't fringe.
    let area = (SS * SS) as f32;
    let mut out = vec![0u8; size as usize * size as usize * 4];
    for y in 0..size as usize {
        for x in 0..size as usize {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let o = ((y * SS + sy) * big + (x * SS + sx)) * 4;
                    let al = img[o + 3] as f32 / 255.0;
                    r += img[o] as f32 / 255.0 * al;
                    g += img[o + 1] as f32 / 255.0 * al;
                    b += img[o + 2] as f32 / 255.0 * al;
                    a += al;
                }
            }
            r /= area;
            g /= area;
            b /= area;
            a /= area;
            let o = (y * size as usize + x) * 4;
            if a > 0.0 {
                out[o] = ((r / a).clamp(0.0, 1.0) * 255.0).round() as u8;
                out[o + 1] = ((g / a).clamp(0.0, 1.0) * 255.0).round() as u8;
                out[o + 2] = ((b / a).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            out[o + 3] = (a * 255.0).round() as u8;
        }
    }

    encode_png(size, size, &out)
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let icons = [
        ("favicon-32.png", 32, false),
        ("apple-touch-icon.png", 180, false),
        ("icon-192.png", 192, false),
        ("icon-512.png", 512, false),
        ("icon-192-maskable.png", 192, true),
        ("icon-512-maskable.png", 512, true),
    ];
    for (name, size, maskable) in icons {
        let png = render(size, maskable);
        let path = dir.join(name);
        fs::write(&path, png).unwrap_or_else(|e| panic!("write {name}: {e}"));
        println!("wrote {}", path.display());
    }
}
