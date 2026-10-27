//! Reading free-form music requests ("something for a first dance at a
//! wedding, no rap") with a small local language model (Qwen2.5 1.5B
//! Instruct), which answers with the same things the phrase reader looks
//! for: genres, years, tempo, mood, artists, and a description of the
//! sound for the listening model.

use std::path::Path;

use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;

use crate::qwen2::Qwen2;
use crate::AiError;

const SYSTEM: &str = r#"You turn music requests into JSON for a party jukebox's search. Reply with one line of JSON and nothing else, leaving out fields that don't apply.
Fields: "genres": common genre names the request asks for or clearly implies. "not_genres": genres the request rules out ("no rap", "nothing country"). "years": [from, to]. "tempo": "slow", "medium" or "fast". "mood": "happy", "sad", "romantic", "dark", "energetic" or "calm". "like_artists": only artists named in the request. "sound": a few words describing the sound.
Words for activities or places (cleaning the house, a drive in the country) are not genres.
Example: 90s stuff like Nirvana, no ballads
{"genres":["grunge","alternative rock"],"not_genres":["ballads"],"years":[1990,1999],"like_artists":["Nirvana"],"sound":"loud distorted guitars"}"#;

/// Longest request read, in characters.
const MAX_REQUEST: usize = 200;
/// Longest answer, in tokens.
const MAX_ANSWER: usize = 120;

/// What a request asks for. Strings are trimmed and lowercased (except
/// artist names); empty ones are dropped.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub genres: Vec<String>,
    pub not_genres: Vec<String>,
    pub years: Option<(i64, i64)>,
    /// "slow", "medium", or "fast".
    pub tempo: Option<String>,
    pub mood: Option<String>,
    pub like_artists: Vec<String>,
    pub sound: Option<String>,
}

/// The loaded model, with the instructions already read.
pub struct Reader {
    model: Qwen2,
    tokenizer: Tokenizer,
    device: Device,
    end: u32,
    /// Tokens of instructions kept in the model's cache.
    prefix: usize,
}

impl Reader {
    /// Loads `model.gguf` and `tokenizer.json` from `dir`, on the GPU when
    /// DJINN was built with GPU support and one is there.
    pub fn load(dir: &Path) -> Result<Self, AiError> {
        let device = device();
        let path = dir.join(crate::models::READER_WEIGHTS);
        let mut file = std::fs::File::open(&path)
            .map_err(|e| AiError::Model(format!("{}: {e}", path.display())))?;
        let content =
            gguf_file::Content::read(&mut file).map_err(|e| AiError::Model(e.to_string()))?;
        let model = Qwen2::from_gguf(content, &mut file, &device)
            .map_err(|e| AiError::Model(e.to_string()))?;
        let tokenizer = Tokenizer::from_file(dir.join(crate::models::READER_TOKENIZER))
            .map_err(|e| AiError::Model(e.to_string()))?;
        let end = tokenizer
            .token_to_id("<|im_end|>")
            .ok_or_else(|| AiError::Model("tokenizer has no <|im_end|>".into()))?;
        let mut reader = Self {
            model,
            tokenizer,
            device,
            end,
            prefix: 0,
        };
        let instructions = format!("<|im_start|>system\n{SYSTEM}<|im_end|>\n<|im_start|>user\n");
        let tokens = reader.encode(&instructions)?;
        reader.feed(&tokens, 0)?;
        reader.prefix = tokens.len();
        Ok(reader)
    }

    /// True if it runs on a GPU.
    pub fn on_gpu(&self) -> bool {
        !self.device.is_cpu()
    }

    /// Reads one request. Takes a few seconds on a CPU.
    pub fn read(&mut self, request: &str) -> Result<Reading, AiError> {
        // No way to slip in chat markers.
        let request: String = request
            .replace("<|", " ")
            .replace("|>", " ")
            .chars()
            .take(MAX_REQUEST)
            .collect();
        let tokens = self.encode(&format!(
            "{}<|im_end|>\n<|im_start|>assistant\n",
            request.trim()
        ))?;
        self.model.rewind(self.prefix).map_err(inference)?;
        let mut logits = self.feed(&tokens, self.prefix)?;
        let mut answer = Vec::new();
        for pos in (self.prefix + tokens.len()..).take(MAX_ANSWER) {
            let next = logits
                .squeeze(0)
                .and_then(|l| l.argmax(0))
                .and_then(|t| t.to_scalar::<u32>())
                .map_err(inference)?;
            if next == self.end {
                break;
            }
            answer.push(next);
            logits = self.feed(&[next], pos)?;
        }
        let text = self
            .tokenizer
            .decode(&answer, true)
            .map_err(|e| AiError::Inference(e.to_string()))?;
        Ok(parse(&text))
    }

    fn encode(&self, text: &str) -> Result<Vec<u32>, AiError> {
        Ok(self
            .tokenizer
            .encode(text, false)
            .map_err(|e| AiError::Inference(e.to_string()))?
            .get_ids()
            .to_vec())
    }

    fn feed(&mut self, tokens: &[u32], pos: usize) -> Result<Tensor, AiError> {
        let input = Tensor::new(tokens, &self.device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(inference)?;
        self.model.forward(&input, pos).map_err(inference)
    }
}

fn inference(e: candle_core::Error) -> AiError {
    AiError::Inference(e.to_string())
}

/// The GPU if DJINN was built for one and it's there, else the CPU.
fn device() -> Device {
    #[cfg(feature = "cuda")]
    if let Ok(device) = Device::new_cuda(0) {
        return device;
    }
    #[cfg(feature = "metal")]
    if let Ok(device) = Device::new_metal(0) {
        return device;
    }
    Device::Cpu
}

/// What the model said, loosely: fields of the wrong shape are skipped.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Answer {
    genres: serde_json::Value,
    not_genres: serde_json::Value,
    years: serde_json::Value,
    tempo: serde_json::Value,
    mood: serde_json::Value,
    like_artists: serde_json::Value,
    sound: serde_json::Value,
}

/// Reads the model's answer: the first `{` to the last `}`, so stray text
/// around it doesn't matter.
fn parse(text: &str) -> Reading {
    let json = match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if end > start => &text[start..=end],
        _ => return Reading::default(),
    };
    let Ok(answer) = serde_json::from_str::<Answer>(json) else {
        return Reading::default();
    };
    let one = |v: &serde_json::Value| -> Option<String> {
        v.as_str()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
    };
    let many = |v: &serde_json::Value, lower: bool| -> Vec<String> {
        let items: Vec<&serde_json::Value> = match v {
            serde_json::Value::Array(items) => items.iter().collect(),
            single @ serde_json::Value::String(_) => vec![single],
            _ => Vec::new(),
        };
        let mut out: Vec<String> = Vec::new();
        for s in items.into_iter().filter_map(|i| i.as_str()) {
            let s = s.trim();
            let s = if lower {
                s.to_lowercase()
            } else {
                s.to_owned()
            };
            if !s.is_empty() && !out.contains(&s) {
                out.push(s);
            }
        }
        out
    };
    let year = |v: &serde_json::Value| v.as_i64().filter(|y| (1900..=2100).contains(y));
    let years = match &answer.years {
        serde_json::Value::Array(pair) => match pair.as_slice() {
            [from] => year(from).map(|f| (f, f)),
            [from, to, ..] => year(from).zip(year(to)).filter(|(f, t)| f <= t),
            _ => None,
        },
        single => year(single).map(|y| (y, y)),
    };
    let tempo = one(&answer.tempo).filter(|t| ["slow", "medium", "fast"].contains(&t.as_str()));
    let genres = many(&answer.genres, true);
    // A genre both asked for and ruled out isn't ruled out.
    let not_genres = many(&answer.not_genres, true)
        .into_iter()
        .filter(|g| !genres.contains(g))
        .collect();
    Reading {
        genres,
        not_genres,
        years,
        tempo,
        mood: one(&answer.mood),
        like_artists: many(&answer.like_artists, false),
        sound: one(&answer.sound),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_compact_answers() {
        let r = parse(
            r#"{"genres":["hair metal"," 80's Rock"],"years":[1980,1989],"tempo":"fast","like_artists":["Mötley Crüe"],"sound":"big guitars"}"#,
        );
        assert_eq!(r.genres, ["hair metal", "80's rock"]);
        assert_eq!(r.years, Some((1980, 1989)));
        assert_eq!(r.tempo.as_deref(), Some("fast"));
        assert_eq!(r.like_artists, ["Mötley Crüe"]);
        assert_eq!(r.sound.as_deref(), Some("big guitars"));
    }

    #[test]
    fn tolerates_odd_answers() {
        let r = parse(
            "Sure! Here it is:\n{\n  \"genres\": \"pop\",\n  \"years\": [],\n  \"tempo\": \"quick\",\n  \"mood\": \"\",\n  \"not_genres\": [\"rap\", \"pop\"],\n  \"like_artists\": null\n}\nEnjoy",
        );
        assert_eq!(r.genres, ["pop"]);
        assert_eq!(r.years, None, "empty years");
        assert_eq!(r.tempo, None, "not a tempo it knows");
        assert_eq!(r.mood, None);
        assert_eq!(r.not_genres, ["rap"], "pop was asked for");
        assert!(r.like_artists.is_empty());

        assert_eq!(parse(r#"{"years":[1999,1990]}"#).years, None);
        assert_eq!(parse(r#"{"years":1995}"#).years, Some((1995, 1995)));
        assert_eq!(parse("no json here"), Reading::default());
        assert_eq!(parse(r#"{"genres": ["#), Reading::default());
    }
}
