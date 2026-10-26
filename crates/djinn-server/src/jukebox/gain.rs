//! ReplayGain: evens out loudness between songs.

use djinn_store::GainInfo;
use serde::{Deserialize, Serialize};

pub const REPLAY_GAIN_MODE: &str = "replaygain_mode";
pub const REPLAY_GAIN_PREAMP: &str = "replaygain_preamp_db";
/// Preamp range in dB.
pub const MAX_PREAMP_DB: f64 = 12.0;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGainMode {
    #[default]
    Off,
    /// Every song at the same loudness.
    Track,
    /// Albums keep their own loud and quiet songs; falls back to track gain.
    Album,
}

impl ReplayGainMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Track => "track",
            Self::Album => "album",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "track" => Self::Track,
            "album" => Self::Album,
            _ => Self::Off,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReplayGain {
    pub mode: ReplayGainMode,
    /// Added to every song's gain, in dB.
    pub preamp_db: f64,
}

impl ReplayGain {
    /// Linear gain for a song. Tags win over DJINN's own measurement, and
    /// the gain never pushes the song's peak past full scale. Songs with
    /// nothing to go on play unchanged.
    pub fn linear(&self, info: &GainInfo) -> f32 {
        let measured = |lufs: Option<f64>, peak: Option<f64>| {
            lufs.map(|lufs| (djinn_engine::REFERENCE_LUFS - lufs, peak))
        };
        let track = info
            .rg_track_gain
            .map(|g| (g, info.rg_track_peak))
            .or_else(|| measured(info.loudness_lufs, info.loudness_peak));
        let album = info
            .rg_album_gain
            .map(|g| (g, info.rg_album_peak.or(info.rg_track_peak)))
            .or_else(|| {
                // Measured album loudness only stands in for untagged songs.
                if info.rg_track_gain.is_none() {
                    measured(info.album_lufs, info.album_peak)
                } else {
                    None
                }
            });
        let chosen = match self.mode {
            ReplayGainMode::Off => None,
            ReplayGainMode::Track => track,
            ReplayGainMode::Album => album.or(track),
        };
        let Some((gain_db, peak)) = chosen else {
            return 1.0;
        };
        let preamp = self.preamp_db.clamp(-MAX_PREAMP_DB, MAX_PREAMP_DB);
        let mut linear = 10f64.powf((gain_db + preamp) / 20.0);
        if let Some(peak) = peak.filter(|p| *p > 0.0) {
            linear = linear.min(1.0 / peak);
        }
        linear as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(linear: f32) -> f64 {
        20.0 * f64::from(linear).log10()
    }

    #[test]
    fn tags_win_and_peaks_limit() {
        let info = GainInfo {
            rg_track_gain: Some(-6.0),
            rg_track_peak: Some(0.8),
            loudness_lufs: Some(-8.0),
            ..Default::default()
        };
        let track = ReplayGain {
            mode: ReplayGainMode::Track,
            preamp_db: 0.0,
        };
        assert!((db(track.linear(&info)) + 6.0).abs() < 1e-4);
        // +6 dB (about 2x) would push a peak of 0.8 past full scale, so it
        // stops at 1.25x.
        let loud = ReplayGain {
            preamp_db: 12.0,
            ..track
        };
        assert!((loud.linear(&info) - 1.25).abs() < 1e-4);
        let off = ReplayGain::default();
        assert_eq!(off.linear(&info), 1.0);
    }

    #[test]
    fn measured_loudness_fills_in() {
        let info = GainInfo {
            loudness_lufs: Some(-8.0),
            loudness_peak: Some(1.0),
            album_lufs: Some(-10.0),
            album_peak: Some(1.0),
            ..Default::default()
        };
        let track = ReplayGain {
            mode: ReplayGainMode::Track,
            preamp_db: 0.0,
        };
        assert!((db(track.linear(&info)) + 10.0).abs() < 1e-4, "-18 - -8");
        let album = ReplayGain {
            mode: ReplayGainMode::Album,
            ..track
        };
        assert!((db(album.linear(&info)) + 8.0).abs() < 1e-4, "-18 - -10");
        assert_eq!(track.linear(&GainInfo::default()), 1.0);
    }

    #[test]
    fn modes_round_trip() {
        for mode in [
            ReplayGainMode::Off,
            ReplayGainMode::Track,
            ReplayGainMode::Album,
        ] {
            assert_eq!(ReplayGainMode::parse(mode.as_str()), mode);
        }
        assert_eq!(ReplayGainMode::parse("bogus"), ReplayGainMode::Off);
    }
}
