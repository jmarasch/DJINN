//! The real length of an MP3 that has no length header.
//!
//! An MP3 normally starts with a Xing, Info or VBRI frame that says how long
//! it is. Files without one (a lot of rips) leave the tag reader guessing:
//! it divides the audio's size by the bit rate of the first frame, which is
//! right for constant bit rate files and several times wrong for variable bit
//! rate ones that open with a quiet, low bit rate stretch. Here such a file
//! is looked at: if its bit rate is the same everywhere it sampled, the guess
//! stands; otherwise every frame is counted, which is exact.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// Frames read at the start to see whether the bit rate holds.
const PROBE_FRAMES: usize = 200;
/// How far to search for the next frame header before giving up.
const RESYNC_BYTES: u64 = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    /// 3 for MPEG 2.5 as 0, 2 as 2, 1 as 3 (the header's own bits).
    version: u8,
    layer: u8,
    kbps: u32,
    sample_rate: u32,
    size: u64,
    samples: u32,
}

impl Frame {
    /// Same stream: version, layer and sample rate agree.
    fn same_stream(&self, other: &Frame) -> bool {
        self.version == other.version
            && self.layer == other.layer
            && self.sample_rate == other.sample_rate
    }
}

const BITRATES_V1: [[u32; 15]; 3] = [
    [
        0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
    ],
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ],
    [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ],
];
const BITRATES_V2: [[u32; 15]; 2] = [
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
    ],
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
];

/// Reads a frame header (MPEG 1, 2 and 2.5, layers I to III).
fn parse(h: [u8; 4]) -> Option<Frame> {
    if h[0] != 0xff || h[1] & 0xe0 != 0xe0 {
        return None;
    }
    let version = (h[1] >> 3) & 3; // 0: 2.5, 2: 2, 3: 1
    let layer_bits = (h[1] >> 1) & 3; // 1: III, 2: II, 3: I
    if version == 1 || layer_bits == 0 {
        return None;
    }
    let layer = 4 - layer_bits; // 1, 2 or 3
    let rate_index = usize::from(h[2] >> 4);
    let sr_index = usize::from((h[2] >> 2) & 3);
    if rate_index == 0 || rate_index == 15 || sr_index == 3 {
        return None;
    }
    let sample_rate = match version {
        3 => [44100, 48000, 32000][sr_index],
        2 => [22050, 24000, 16000][sr_index],
        _ => [11025, 12000, 8000][sr_index],
    };
    let kbps = if version == 3 {
        BITRATES_V1[usize::from(layer) - 1][rate_index]
    } else if layer == 1 {
        BITRATES_V2[0][rate_index]
    } else {
        BITRATES_V2[1][rate_index]
    };
    let padding = u64::from((h[2] >> 1) & 1);
    let bps = u64::from(kbps) * 1000;
    let rate = u64::from(sample_rate);
    let (size, samples) = match (layer, version) {
        (1, _) => ((12 * bps / rate + padding) * 4, 384),
        (2, _) => (144 * bps / rate + padding, 1152),
        (_, 3) => (144 * bps / rate + padding, 1152),
        _ => (72 * bps / rate + padding, 576),
    };
    if size < 4 {
        return None;
    }
    Some(Frame {
        version,
        layer,
        kbps,
        sample_rate,
        size,
        samples,
    })
}

fn header_at<R: Read + Seek>(r: &mut R, pos: u64) -> Option<Frame> {
    r.seek(SeekFrom::Start(pos)).ok()?;
    let mut h = [0u8; 4];
    r.read_exact(&mut h).ok()?;
    parse(h)
}

/// The first frame at or after `from`, accepted only when another frame of
/// the same stream follows it (so a stray byte pair in a picture isn't one).
fn find_frame<R: Read + Seek>(
    r: &mut R,
    from: u64,
    within: u64,
    like: Option<&Frame>,
) -> Option<(u64, Frame)> {
    let mut pos = from;
    let end = from.saturating_add(within);
    while pos < end {
        if let Some(frame) = header_at(r, pos) {
            let fits = like.is_none_or(|l| l.same_stream(&frame));
            if fits {
                if let Some(next) = header_at(r, pos + frame.size) {
                    if next.same_stream(&frame) {
                        return Some((pos, frame));
                    }
                }
            }
        }
        pos += 1;
    }
    None
}

/// Where the audio starts: after an ID3v2 tag if there is one.
fn audio_start<R: Read + Seek>(r: &mut R) -> Option<u64> {
    r.seek(SeekFrom::Start(0)).ok()?;
    let mut head = [0u8; 10];
    r.read_exact(&mut head).ok()?;
    if &head[..3] == b"ID3" {
        let size = (u64::from(head[6] & 0x7f) << 21)
            | (u64::from(head[7] & 0x7f) << 14)
            | (u64::from(head[8] & 0x7f) << 7)
            | u64::from(head[9] & 0x7f);
        let footer = if head[5] & 0x10 != 0 { 10 } else { 0 };
        return Some(10 + size + footer);
    }
    Some(0)
}

/// The exact length in milliseconds of an MP3 that has no length header and
/// whose bit rate varies. None when the file has a header, holds one bit
/// rate throughout (the tag reader's figure is right), or can't be read.
pub fn exact_duration_ms(path: &Path) -> Option<i64> {
    let mut r = BufReader::with_capacity(64 * 1024, File::open(path).ok()?);
    let len = r.get_ref().metadata().ok()?.len();
    let start = audio_start(&mut r)?;
    let (first_pos, first) = find_frame(&mut r, start, 64 * 1024, None)?;

    // A length header in the first frame: the tag reader uses it.
    r.seek(SeekFrom::Start(first_pos)).ok()?;
    let mut body = vec![0u8; usize::try_from(first.size.min(512)).ok()?];
    r.read_exact(&mut body).ok()?;
    let has_header = body
        .windows(4)
        .take(200)
        .any(|w| w == b"Xing" || w == b"Info" || w == b"VBRI");
    if has_header {
        return None;
    }

    // Does the bit rate hold? The first frames, then three places further in.
    let mut varies = false;
    let mut pos = first_pos;
    for _ in 0..PROBE_FRAMES {
        let Some(frame) = header_at(&mut r, pos) else {
            break;
        };
        if !first.same_stream(&frame) {
            break;
        }
        varies |= frame.kbps != first.kbps;
        pos += frame.size;
    }
    if !varies {
        for quarter in 1..=3u64 {
            let at = first_pos + (len - first_pos) * quarter / 4;
            if let Some((_, frame)) = find_frame(&mut r, at, RESYNC_BYTES, Some(&first)) {
                varies |= frame.kbps != first.kbps;
            }
        }
    }
    if !varies {
        return None;
    }

    // Count every frame.
    let mut samples: u64 = 0;
    let mut pos = first_pos;
    let mut frames = 0u64;
    while pos < len {
        match header_at(&mut r, pos) {
            Some(frame) if first.same_stream(&frame) => {
                samples += u64::from(frame.samples);
                frames += 1;
                pos += frame.size;
            }
            _ => match find_frame(&mut r, pos + 1, RESYNC_BYTES, Some(&first)) {
                Some((next, _)) => pos = next,
                None => break,
            },
        }
    }
    if frames < 10 {
        return None;
    }
    i64::try_from(samples * 1000 / u64::from(first.sample_rate)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A silent MPEG 1 Layer III frame at 44.1 kHz and this bit rate.
    fn frame(kbps: u32) -> Vec<u8> {
        let index = BITRATES_V1[2].iter().position(|&b| b == kbps).unwrap() as u8;
        let size = (144 * u64::from(kbps) * 1000 / 44100) as usize;
        let mut f = vec![0u8; size];
        f[..4].copy_from_slice(&[0xff, 0xfb, index << 4, 0x00]);
        f
    }

    fn file(name: &str, frames: &[Vec<u8>], tag_bytes: usize) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("djinn-mpeg-{name}-{}.mp3", std::process::id()));
        let mut out = File::create(&path).unwrap();
        if tag_bytes > 0 {
            let mut id3 = b"ID3\x04\x00\x00".to_vec();
            id3.extend([0, 0, (tag_bytes >> 7) as u8 & 0x7f, tag_bytes as u8 & 0x7f]);
            id3.extend(vec![0u8; tag_bytes]);
            out.write_all(&id3).unwrap();
        }
        for f in frames {
            out.write_all(f).unwrap();
        }
        path
    }

    #[test]
    fn a_variable_rate_file_without_a_header_is_counted_exactly() {
        // 100 quiet frames at 32 kbps, then 1000 at 192: about 28.7 s.
        let mut frames = vec![frame(32); 100];
        frames.extend(vec![frame(192); 1000]);
        let path = file("vbr", &frames, 300);
        let ms = exact_duration_ms(&path).unwrap();
        let expected = 1100 * 1152 * 1000 / 44100;
        assert!((ms - expected).abs() <= 1, "{ms} vs {expected}");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn a_constant_rate_file_keeps_the_tag_readers_figure() {
        let path = file("cbr", &vec![frame(128); 600], 0);
        assert_eq!(exact_duration_ms(&path), None);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn a_file_with_a_length_header_is_left_to_the_tag_reader() {
        let mut first = frame(128);
        first[36..40].copy_from_slice(b"Xing");
        let mut frames = vec![first];
        frames.extend(vec![frame(192); 300]);
        let path = file("xing", &frames, 0);
        assert_eq!(exact_duration_ms(&path), None);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn rubbish_is_not_a_song() {
        let path = std::env::temp_dir().join(format!("djinn-mpeg-junk-{}.mp3", std::process::id()));
        std::fs::write(&path, vec![0x55u8; 5000]).unwrap();
        assert_eq!(exact_duration_ms(&path), None);
        std::fs::remove_file(path).ok();
    }
}
