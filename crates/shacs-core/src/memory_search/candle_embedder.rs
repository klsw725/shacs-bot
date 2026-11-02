use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::{bert, xlm_roberta};
use tokenizers::{Encoding, PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

use super::embedder::{Embedder, EmbeddingError, EmbeddingKind};
use super::model_manifest::{ModelArchitecture, ModelManifest, Pooling};
use super::verified_model::VerifiedModel;

/// Texts embedded in one forward pass; bounds the memory a batch of long items takes.
const BATCH_SIZE: usize = 16;

/// A manifest model run in this process on the CPU with candle.
pub struct CandleEmbedder {
    manifest: &'static ModelManifest,
    encoder: Encoder,
    tokenizer: Tokenizer,
    device: Device,
}

enum Encoder {
    Bert(bert::BertModel),
    XlmRoberta(xlm_roberta::XLMRobertaModel),
}

impl CandleEmbedder {
    pub fn load(model: &VerifiedModel) -> Result<Self, EmbeddingError> {
        let manifest = model.manifest;
        let device = Device::Cpu;
        let weights = VarBuilder::from_slice_safetensors(&model.weights, DType::F32, &device)
            .map_err(load_error)?;
        let (encoder, pad_id) = match manifest.architecture {
            ModelArchitecture::Bert => {
                let config: bert::Config =
                    serde_json::from_slice(&model.config).map_err(load_error)?;
                let pad_id = u32::try_from(config.pad_token_id).map_err(load_error)?;
                let model = bert::BertModel::load(weights, &config).map_err(load_error)?;
                (Encoder::Bert(model), pad_id)
            }
            ModelArchitecture::XlmRoberta => {
                let config: xlm_roberta::Config =
                    serde_json::from_slice(&model.config).map_err(load_error)?;
                let weights =
                    if weights.contains_tensor("roberta.embeddings.word_embeddings.weight") {
                        weights.pp("roberta")
                    } else {
                        weights
                    };
                let model =
                    xlm_roberta::XLMRobertaModel::new(&config, weights).map_err(load_error)?;
                (Encoder::XlmRoberta(model), config.pad_token_id)
            }
        };
        let mut tokenizer = Tokenizer::from_bytes(&model.tokenizer).map_err(load_error)?;
        let pad_token = tokenizer
            .id_to_token(pad_id)
            .ok_or_else(|| EmbeddingError::Load(format!("tokenizer has no token {pad_id}")))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: manifest.max_tokens,
                ..TruncationParams::default()
            }))
            .map_err(load_error)?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            pad_id,
            pad_token,
            ..PaddingParams::default()
        }));
        Ok(Self {
            manifest,
            encoder,
            tokenizer,
            device,
        })
    }

    fn embed_batch(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let encodings = self
            .tokenizer
            .encode_batch(inputs, true)
            .map_err(compute_error)?;
        let ids = batch_tensor(&encodings, Encoding::get_ids, &self.device)?;
        let type_ids = batch_tensor(&encodings, Encoding::get_type_ids, &self.device)?;
        let mask = batch_tensor(&encodings, Encoding::get_attention_mask, &self.device)?;
        let hidden = match &self.encoder {
            Encoder::Bert(model) => model.forward(&ids, &type_ids, Some(&mask)),
            Encoder::XlmRoberta(model) => model.forward(&ids, &mask, &type_ids, None, None, None),
        }
        .map_err(compute_error)?;
        let pooled = match self.manifest.pooling {
            Pooling::Mean => mean_pool(&hidden, &mask),
            Pooling::Cls => hidden.i((.., 0)),
        }
        .map_err(compute_error)?;
        let norm = pooled
            .sqr()
            .and_then(|squares| squares.sum_keepdim(1))
            .and_then(|sum| sum.sqrt())
            .map_err(compute_error)?;
        pooled
            .broadcast_div(&norm)
            .and_then(|normalized| normalized.to_vec2::<f32>())
            .map_err(compute_error)
    }
}

impl Embedder for CandleEmbedder {
    fn model_id(&self) -> &str {
        self.manifest.id
    }

    fn revision(&self) -> &str {
        self.manifest.revision
    }

    fn dimension(&self) -> usize {
        self.manifest.dimension
    }

    fn embed(&self, kind: EmbeddingKind, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let prefix = match kind {
            EmbeddingKind::Query => self.manifest.query_prefix,
            EmbeddingKind::Passage => self.manifest.passage_prefix,
        };
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH_SIZE) {
            let inputs: Vec<String> = batch.iter().map(|text| format!("{prefix}{text}")).collect();
            vectors.extend(self.embed_batch(inputs)?);
        }
        Ok(vectors)
    }
}

/// A `[texts, tokens]` tensor of one value of every token; padding made the rows equal.
fn batch_tensor(
    encodings: &[Encoding],
    values: fn(&Encoding) -> &[u32],
    device: &Device,
) -> Result<Tensor, EmbeddingError> {
    let columns = encodings.first().map_or(0, Encoding::len);
    let flat: Vec<u32> = encodings
        .iter()
        .flat_map(|encoding| values(encoding).iter().copied())
        .collect();
    Tensor::from_vec(flat, (encodings.len(), columns), device).map_err(compute_error)
}

/// The mean of the token vectors the attention mask keeps, as sentence-transformers computes
/// it.
fn mean_pool(hidden: &Tensor, mask: &Tensor) -> candle_core::Result<Tensor> {
    let mask = mask.to_dtype(DType::F32)?.unsqueeze(2)?;
    let sum = hidden.broadcast_mul(&mask)?.sum(1)?;
    let count = mask.sum(1)?.maximum(1e-9)?;
    sum.broadcast_div(&count)
}

fn load_error(error: impl std::fmt::Display) -> EmbeddingError {
    EmbeddingError::Load(error.to_string())
}

fn compute_error(error: impl std::fmt::Display) -> EmbeddingError {
    EmbeddingError::Compute(error.to_string())
}
