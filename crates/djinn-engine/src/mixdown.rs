//! Maps any channel layout to stereo.

use symphonia::core::audio::{Channels, Position};

const H: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Per-input-channel (left, right) gains.
#[derive(Clone, Debug, PartialEq)]
pub struct StereoMap {
    gains: Vec<(f32, f32)>,
}

impl StereoMap {
    pub fn new(channels: &Channels) -> Self {
        let count = channels.count();
        let gains = match (channels, count) {
            // Mono plays at full level on both sides.
            (_, 1) => vec![(1.0, 1.0)],
            (Channels::Positioned(mask), _) => positioned(*mask),
            // Unlabelled layouts: assume the first two are left and right.
            (_, n) => {
                let mut gains = vec![(0.0, 0.0); n];
                if let Some(g) = gains.get_mut(0) {
                    *g = (1.0, 0.0);
                }
                if let Some(g) = gains.get_mut(1) {
                    *g = (0.0, 1.0);
                }
                gains
            }
        };
        Self { gains }
    }

    pub fn input_channels(&self) -> usize {
        self.gains.len()
    }

    /// Appends interleaved stereo frames mixed from `planes` to `out`.
    pub fn mix(&self, planes: &[Vec<f32>], frames: usize, out: &mut Vec<f32>) {
        out.reserve(frames * 2);
        match (self.gains.as_slice(), planes) {
            ([_], [mono, ..]) => {
                for &s in &mono[..frames] {
                    out.extend_from_slice(&[s, s]);
                }
            }
            ([(1.0, 0.0), (0.0, 1.0)], [l, r, ..]) => {
                for (&l, &r) in l[..frames].iter().zip(&r[..frames]) {
                    out.extend_from_slice(&[l, r]);
                }
            }
            _ => {
                for i in 0..frames {
                    let (mut l, mut r) = (0.0, 0.0);
                    for (plane, &(gl, gr)) in planes.iter().zip(&self.gains) {
                        l += plane[i] * gl;
                        r += plane[i] * gr;
                    }
                    out.extend_from_slice(&[l, r]);
                }
            }
        }
    }
}

/// ITU-style downmix for positioned channels, in canonical (bit) order,
/// normalized so a full-scale signal on every channel cannot clip.
fn positioned(mask: Position) -> Vec<(f32, f32)> {
    let left = Position::FRONT_LEFT | Position::FRONT_LEFT_CENTER;
    let right = Position::FRONT_RIGHT | Position::FRONT_RIGHT_CENTER;
    let surround_left = Position::REAR_LEFT
        | Position::SIDE_LEFT
        | Position::TOP_FRONT_LEFT
        | Position::TOP_REAR_LEFT;
    let surround_right = Position::REAR_RIGHT
        | Position::SIDE_RIGHT
        | Position::TOP_FRONT_RIGHT
        | Position::TOP_REAR_RIGHT;

    let mut gains: Vec<(f32, f32)> = (0..64)
        .map(|bit| Position::from_bits_retain(1 << bit))
        .filter(|p| mask.contains(*p))
        .map(|p| {
            if left.contains(p) {
                (1.0, 0.0)
            } else if right.contains(p) {
                (0.0, 1.0)
            } else if surround_left.contains(p) {
                (H, 0.0)
            } else if surround_right.contains(p) {
                (0.0, H)
            } else if p == Position::LFE1 || p == Position::LFE2 {
                (0.0, 0.0)
            } else {
                // Centers and anything unrecognized go to both sides.
                (H, H)
            }
        })
        .collect();

    let is_plain_stereo = gains == [(1.0, 0.0), (0.0, 1.0)];
    if !is_plain_stereo {
        let peak = gains
            .iter()
            .fold((0.0f32, 0.0f32), |(l, r), &(gl, gr)| (l + gl, r + gr));
        let norm = peak.0.max(peak.1);
        if norm > 1.0 {
            for g in &mut gains {
                g.0 /= norm;
                g.1 /= norm;
            }
        }
    }
    gains
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mix(channels: Channels, planes: Vec<Vec<f32>>) -> Vec<f32> {
        let map = StereoMap::new(&channels);
        let mut out = Vec::new();
        map.mix(&planes, planes[0].len(), &mut out);
        out
    }

    #[test]
    fn mono_is_duplicated() {
        let out = mix(Position::FRONT_CENTER.into(), vec![vec![0.5, -0.25]]);
        assert_eq!(out, [0.5, 0.5, -0.25, -0.25]);
    }

    #[test]
    fn stereo_passes_through() {
        let out = mix(
            (Position::FRONT_LEFT | Position::FRONT_RIGHT).into(),
            vec![vec![0.1, 0.2], vec![0.3, 0.4]],
        );
        assert_eq!(out, [0.1, 0.3, 0.2, 0.4]);
    }

    #[test]
    fn surround_downmix_cannot_clip() {
        let layout = Position::FRONT_LEFT
            | Position::FRONT_RIGHT
            | Position::FRONT_CENTER
            | Position::LFE1
            | Position::REAR_LEFT
            | Position::REAR_RIGHT;
        let out = mix(layout.into(), vec![vec![1.0]; 6]);
        assert!(out.iter().all(|s| s.abs() <= 1.0 + 1e-6), "{out:?}");
        // Symmetric input stays centered.
        assert!((out[0] - out[1]).abs() < 1e-6);
    }

    #[test]
    fn lfe_is_dropped() {
        let layout = Position::FRONT_LEFT | Position::FRONT_RIGHT | Position::LFE1;
        let out = mix(layout.into(), vec![vec![0.0], vec![0.0], vec![1.0]]);
        assert_eq!(out, [0.0, 0.0]);
    }

    #[test]
    fn discrete_uses_first_two_channels() {
        let out = mix(Channels::Discrete(3), vec![vec![0.1], vec![0.2], vec![0.9]]);
        assert_eq!(out, [0.1, 0.2]);
    }
}
