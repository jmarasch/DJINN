//! Decodes one file to interleaved f32 stereo at its own sample rate.
//!
//! Encoder delay and padding are trimmed by symphonia's decoders where the
//! format supports it (MP3 with a LAME/Xing header, FLAC, Vorbis, WAV), so
//! consecutive tracks join without a gap.

use std::fs::File;
use std::path::{Path, PathBuf};

use symphonia::core::audio::Channels;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::mixdown::StereoMap;
use crate::EngineError;

/// Give up on a file after this many consecutive undecodable packets.
const MAX_CONSECUTIVE_ERRORS: u32 = 50;

pub struct TrackDecoder {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    sample_rate: u32,
    planes: Vec<Vec<f32>>,
    map: Option<(Channels, StereoMap)>,
    consecutive_errors: u32,
}

impl TrackDecoder {
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        let file = File::open(path).map_err(|source| EngineError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let probe_err = |source| EngineError::Probe {
            path: path.to_path_buf(),
            source,
        };
        let track_err = |reason: &str| EngineError::Track {
            path: path.to_path_buf(),
            reason: reason.to_owned(),
        };

        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(probe_err)?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| track_err("no audio track"))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| track_err("missing codec parameters"))?;
        let sample_rate = params
            .sample_rate
            .filter(|&r| r > 0)
            .ok_or_else(|| track_err("unknown sample rate"))?;
        let track_id = track.id;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(probe_err)?;

        Ok(Self {
            path: path.to_path_buf(),
            format,
            decoder,
            track_id,
            sample_rate,
            planes: Vec::new(),
            map: None,
            consecutive_errors: 0,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Decodes the next packet and appends interleaved stereo frames to
    /// `out`. Returns `Ok(false)` at the end of the stream.
    pub fn decode_next(&mut self, out: &mut Vec<f32>) -> Result<bool, EngineError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(false),
                // A chained stream changed its track list. Treat as the end.
                Err(SymphoniaError::ResetRequired) => return Ok(false),
                // Truncated files end with an unexpected EOF.
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(false)
                }
                Err(e) => return Err(EngineError::Decode(e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }

            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => {
                    self.consecutive_errors += 1;
                    if self.consecutive_errors > MAX_CONSECUTIVE_ERRORS {
                        return Err(EngineError::Track {
                            path: self.path.clone(),
                            reason: "too many corrupt packets".to_owned(),
                        });
                    }
                    continue;
                }
                Err(e) => return Err(EngineError::Decode(e)),
            };
            self.consecutive_errors = 0;

            let spec = decoded.spec();
            if spec.rate() != self.sample_rate {
                // Mid-stream rate changes are vanishingly rare; end the track
                // rather than play it at the wrong speed.
                return Ok(false);
            }
            let frames = decoded.frames();
            if frames == 0 {
                continue;
            }
            let channels = spec.channels();
            if self.map.as_ref().is_none_or(|(c, _)| c != channels) {
                self.map = Some((channels.clone(), StereoMap::new(channels)));
            }
            decoded.copy_to_vecs_planar::<f32>(&mut self.planes);
            if let Some((_, map)) = &self.map {
                if self.planes.len() < map.input_channels() {
                    continue;
                }
                map.mix(&self.planes, frames, out);
            }
            return Ok(true);
        }
    }
}
