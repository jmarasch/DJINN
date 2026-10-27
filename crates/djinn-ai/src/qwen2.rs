//! Quantized Qwen2 (GGUF), adapted from candle-transformers'
//! `quantized_qwen2` (MIT OR Apache-2.0, Copyright the candle authors).
//! The changes: [`Qwen2::rewind`] keeps the first part of the attention
//! cache, so a fixed system prompt is read once instead of per request,
//! and the token table is kept as f16 (half the memory, about 450 MB).

use candle_core::quantized::{gguf_file, QMatMul};
use candle_core::{DType, Device, IndexOp, Module, Result, Tensor};
use candle_nn::Embedding;
use candle_transformers::quantized_nn::RmsNorm;
use candle_transformers::utils::{build_causal_mask, repeat_kv};

struct Mlp {
    gate: QMatMul,
    down: QMatMul,
    up: QMatMul,
}

impl Mlp {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let gate = self.gate.forward(xs)?;
        let up = self.up.forward(xs)?;
        self.down.forward(&(candle_nn::ops::silu(&gate)? * up)?)
    }
}

struct Layer {
    wq: QMatMul,
    wk: QMatMul,
    wv: QMatMul,
    bq: Tensor,
    bk: Tensor,
    bv: Tensor,
    wo: QMatMul,
    attention_norm: RmsNorm,
    mlp: Mlp,
    ffn_norm: RmsNorm,
    n_head: usize,
    n_kv_head: usize,
    head_dim: usize,
    cos: Tensor,
    sin: Tensor,
    neg_inf: Tensor,
    kv_cache: Option<(Tensor, Tensor)>,
}

impl Layer {
    fn rotary(&self, x: &Tensor, index_pos: usize) -> Result<Tensor> {
        let (_, _, seq_len, _) = x.dims4()?;
        let cos = self.cos.narrow(0, index_pos, seq_len)?;
        let sin = self.sin.narrow(0, index_pos, seq_len)?;
        candle_nn::rotary_emb::rope(&x.contiguous()?, &cos, &sin)
    }

    fn attention(&mut self, x: &Tensor, mask: Option<&Tensor>, index_pos: usize) -> Result<Tensor> {
        let (b_sz, seq_len, n_embd) = x.dims3()?;
        let heads = |t: Tensor, n: usize| -> Result<Tensor> {
            t.reshape((b_sz, seq_len, n, self.head_dim))?
                .transpose(1, 2)?
                .contiguous()
        };
        let q = heads(self.wq.forward(x)?.broadcast_add(&self.bq)?, self.n_head)?;
        let k = heads(self.wk.forward(x)?.broadcast_add(&self.bk)?, self.n_kv_head)?;
        let v = heads(self.wv.forward(x)?.broadcast_add(&self.bv)?, self.n_kv_head)?;
        let q = self.rotary(&q, index_pos)?;
        let k = self.rotary(&k, index_pos)?;

        let (k, v) = match &self.kv_cache {
            Some((k_cache, v_cache)) if index_pos > 0 => (
                Tensor::cat(&[k_cache, &k], 2)?,
                Tensor::cat(&[v_cache, &v], 2)?,
            ),
            _ => (k, v),
        };
        self.kv_cache = Some((k.clone(), v.clone()));

        let k = repeat_kv(k, self.n_head / self.n_kv_head)?;
        let v = repeat_kv(v, self.n_head / self.n_kv_head)?;
        let att = (q.matmul(&k.t()?)? / (self.head_dim as f64).sqrt())?;
        let att = match mask {
            None => att,
            Some(mask) => {
                let mask = mask.broadcast_as(att.shape())?;
                mask.where_cond(&self.neg_inf.broadcast_as(att.shape().dims())?, &att)?
            }
        };
        let att = candle_nn::ops::softmax_last_dim(&att)?;
        let y = att.matmul(&v.contiguous()?)?;
        let y = y.transpose(1, 2)?.reshape(&[b_sz, seq_len, n_embd])?;
        self.wo.forward(&y)
    }
}

pub struct Qwen2 {
    embeddings: Embedding,
    layers: Vec<Layer>,
    norm: RmsNorm,
    output: QMatMul,
}

impl Qwen2 {
    pub fn from_gguf<R: std::io::Seek + std::io::Read>(
        ct: gguf_file::Content,
        reader: &mut R,
        device: &Device,
    ) -> Result<Self> {
        let get = |s: &str| match ct.metadata.get(s) {
            None => candle_core::bail!("cannot find {s} in metadata"),
            Some(v) => Ok(v),
        };
        let head_count = get("qwen2.attention.head_count")?.to_u32()? as usize;
        let head_count_kv = get("qwen2.attention.head_count_kv")?.to_u32()? as usize;
        let embedding_length = get("qwen2.embedding_length")?.to_u32()? as usize;
        let context_length = get("qwen2.context_length")?.to_u32()? as usize;
        let block_count = get("qwen2.block_count")?.to_u32()? as usize;
        let eps = get("qwen2.attention.layer_norm_rms_epsilon")?.to_f32()? as f64;
        let rope_base = get("qwen2.rope.freq_base")
            .and_then(|m| m.to_f32())
            .unwrap_or(10_000.0);
        let head_dim = embedding_length / head_count;
        let neg_inf = Tensor::new(f32::NEG_INFINITY, device)?;

        let embeddings = ct
            .tensor(reader, "token_embd.weight", device)?
            .dequantize(device)?
            .to_dtype(DType::F16)?;
        let norm = RmsNorm::from_qtensor(ct.tensor(reader, "output_norm.weight", device)?, eps)?;
        let output = match ct.tensor(reader, "output.weight", device) {
            Ok(v) => QMatMul::from_qtensor(v)?,
            // Tied word embeddings.
            Err(_) => QMatMul::from_qtensor(ct.tensor(reader, "token_embd.weight", device)?)?,
        };
        let (cos, sin) = rope_tables(head_dim, rope_base, context_length, device)?;

        let mut layers = Vec::with_capacity(block_count);
        for i in 0..block_count {
            let mut tensor = |name: &str| ct.tensor(reader, &format!("blk.{i}.{name}"), device);
            let matmul = |t| QMatMul::from_qtensor(t);
            layers.push(Layer {
                wq: matmul(tensor("attn_q.weight")?)?,
                wk: matmul(tensor("attn_k.weight")?)?,
                wv: matmul(tensor("attn_v.weight")?)?,
                bq: tensor("attn_q.bias")?.dequantize(device)?,
                bk: tensor("attn_k.bias")?.dequantize(device)?,
                bv: tensor("attn_v.bias")?.dequantize(device)?,
                wo: matmul(tensor("attn_output.weight")?)?,
                attention_norm: RmsNorm::from_qtensor(tensor("attn_norm.weight")?, eps)?,
                mlp: Mlp {
                    gate: matmul(tensor("ffn_gate.weight")?)?,
                    down: matmul(tensor("ffn_down.weight")?)?,
                    up: matmul(tensor("ffn_up.weight")?)?,
                },
                ffn_norm: RmsNorm::from_qtensor(tensor("ffn_norm.weight")?, eps)?,
                n_head: head_count,
                n_kv_head: head_count_kv,
                head_dim,
                cos: cos.clone(),
                sin: sin.clone(),
                neg_inf: neg_inf.clone(),
                kv_cache: None,
            });
        }
        Ok(Self {
            embeddings: Embedding::new(embeddings, embedding_length),
            layers,
            norm,
            output,
        })
    }

    /// Forgets everything after the first `len` tokens.
    pub fn rewind(&mut self, len: usize) -> Result<()> {
        for layer in &mut self.layers {
            layer.kv_cache = match layer.kv_cache.take() {
                Some((k, v)) if len > 0 => Some((k.narrow(2, 0, len)?, v.narrow(2, 0, len)?)),
                _ => None,
            };
        }
        Ok(())
    }

    /// Reads `x` (tokens, shape (1, n)) placed after `index_pos` cached
    /// tokens; returns the logits for the next token.
    pub fn forward(&mut self, x: &Tensor, index_pos: usize) -> Result<Tensor> {
        let (_, seq_len) = x.dims2()?;
        let mask = if seq_len == 1 {
            None
        } else {
            Some(build_causal_mask(seq_len, index_pos, x.device())?)
        };
        let mut hidden = self.embeddings.forward(x)?.to_dtype(DType::F32)?;
        for layer in &mut self.layers {
            let attn = layer.attention(
                &layer.attention_norm.forward(&hidden)?,
                mask.as_ref(),
                index_pos,
            )?;
            let x = (attn + &hidden)?;
            let mlp = layer.mlp.forward(&layer.ffn_norm.forward(&x)?)?;
            hidden = (mlp + x)?;
        }
        let x = self.norm.forward(&hidden)?;
        self.output.forward(&x.i((.., seq_len - 1, ..))?)
    }
}

fn rope_tables(
    head_dim: usize,
    base: f32,
    context_length: usize,
    device: &Device,
) -> Result<(Tensor, Tensor)> {
    let theta: Vec<f32> = (0..head_dim)
        .step_by(2)
        .map(|i| 1.0 / base.powf(i as f32 / head_dim as f32))
        .collect();
    let theta = Tensor::new(theta.as_slice(), device)?;
    let angles = Tensor::arange(0, context_length as u32, device)?
        .to_dtype(DType::F32)?
        .reshape((context_length, 1))?
        .matmul(&theta.reshape((1, theta.elem_count()))?)?;
    Ok((angles.cos()?, angles.sin()?))
}
