//! The CLAP model: songs and text in, 512-number embeddings out.

use std::collections::HashMap;
use std::path::Path;

use tract_onnx::prelude::*;

use crate::mel::{MelSpectrogram, CLIP, FRAMES, MELS};
use crate::AiError;

pub use crate::mel::SAMPLE_RATE;

/// Length of every embedding.
pub const EMBEDDING_SIZE: usize = 512;
/// Clips listened to per song, spread through it.
const CLIPS: [f32; 3] = [0.25, 0.5, 0.75];

type Plan = std::sync::Arc<TypedSimplePlan>;

/// The loaded model. Safe to share between threads.
pub struct Clap {
    audio: Plan,
    text: Plan,
    tokenizer: tokenizers::Tokenizer,
    mel: MelSpectrogram,
}

impl Clap {
    /// Loads the model files from `dir` (see [`crate::models::CLAP`]).
    /// Takes a few seconds.
    pub fn load(dir: &Path) -> Result<Self, AiError> {
        let model_error = |e: TractError| AiError::Model(format!("{e:#}"));
        let audio = tract_onnx::onnx()
            .model_for_path(dir.join("audio_model.onnx"))
            .map_err(model_error)?
            .with_input_fact(0, f32::fact([1, 1, FRAMES, MELS]).into())
            .map_err(model_error)?
            .into_optimized()
            .map_err(model_error)?
            .into_runnable()
            .map_err(model_error)?;
        let text = tract_onnx::onnx()
            .model_for_path(dir.join("text_model_quantized.onnx"))
            .map_err(model_error)?;
        // The export leaves the batch size open; tract wants it fixed.
        let batch = text.symbols.sym("batch_size");
        let text = text
            .into_typed()
            .map_err(model_error)?
            .set_symbols(&HashMap::from([(batch, TDim::from(1))]))
            .map_err(model_error)?
            .into_optimized()
            .map_err(model_error)?
            .into_runnable()
            .map_err(model_error)?;
        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| AiError::Model(e.to_string()))?;
        Ok(Self {
            audio,
            text,
            tokenizer,
            mel: MelSpectrogram::new(),
        })
    }

    /// One song's embedding from its mono [`SAMPLE_RATE`] audio: three
    /// clips through it, averaged. `None` for silence.
    pub fn embed_song(&self, samples: &[f32]) -> Result<Option<Vec<f32>>, AiError> {
        if samples.iter().all(|s| s.abs() < 1e-6) {
            return Ok(None);
        }
        let mut sum = vec![0f32; EMBEDDING_SIZE];
        if samples.len() <= CLIP {
            // Short: repeat it to fill a clip, as CLAP was trained.
            let mut clip = Vec::with_capacity(CLIP);
            while clip.len() + samples.len() <= CLIP {
                clip.extend_from_slice(samples);
            }
            clip.resize(CLIP, 0.0);
            sum = self.embed_clip(&clip)?;
        } else {
            for at in CLIPS {
                let start = ((samples.len() - CLIP) as f32 * at) as usize;
                let clip = self.embed_clip(&samples[start..start + CLIP])?;
                for (s, v) in sum.iter_mut().zip(clip) {
                    *s += v;
                }
            }
        }
        normalize(&mut sum);
        Ok(Some(sum))
    }

    fn embed_clip(&self, clip: &[f32]) -> Result<Vec<f32>, AiError> {
        let features = self.mel.features(clip);
        let input: Tensor = tract_ndarray::Array4::from_shape_vec((1, 1, FRAMES, MELS), features)
            .map_err(|e| AiError::Inference(e.to_string()))?
            .into();
        let out = self
            .audio
            .run(tvec!(input.into()))
            .map_err(|e| AiError::Inference(format!("{e:#}")))?;
        let mut v = first_row(&out)?;
        normalize(&mut v);
        Ok(v)
    }

    /// A description's embedding.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>, AiError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| AiError::Inference(e.to_string()))?;
        let ids: Vec<i64> = encoding
            .get_ids()
            .iter()
            .take(77)
            .map(|&id| i64::from(id))
            .collect();
        let len = ids.len();
        let input: Tensor = tract_ndarray::Array2::from_shape_vec((1, len), ids)
            .map_err(|e| AiError::Inference(e.to_string()))?
            .into();
        let out = self
            .text
            .run(tvec!(input.into()))
            .map_err(|e| AiError::Inference(format!("{e:#}")))?;
        let mut v = first_row(&out)?;
        normalize(&mut v);
        Ok(v)
    }
}

fn first_row(out: &TVec<TValue>) -> Result<Vec<f32>, AiError> {
    let view = out
        .first()
        .ok_or_else(|| AiError::Inference("no output".into()))?
        .to_plain_array_view::<f32>()
        .map_err(|e| AiError::Inference(format!("{e:#}")))?;
    let v: Vec<f32> = view.iter().copied().collect();
    if v.len() == EMBEDDING_SIZE {
        Ok(v)
    } else {
        Err(AiError::Inference(format!(
            "unexpected output size {}",
            v.len()
        )))
    }
}

fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    for x in v {
        *x /= norm;
    }
}
