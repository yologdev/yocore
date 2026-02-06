//! Embeddings module for vector search
//!
//! Uses all-MiniLM-L6-v2 model for generating 384-dimensional sentence embeddings.
//! Ported from desktop/src-tauri/src/embeddings/mod.rs.

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use hf_hub::{api::sync::Api, Repo, RepoType};
use std::sync::OnceLock;
use tokenizers::Tokenizer;

/// Model identifier on HuggingFace
const MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";

/// Embedding dimension for all-MiniLM-L6-v2
pub const EMBEDDING_DIM: usize = 384;

/// Global embedding model instance (lazy loaded on first use)
static EMBEDDING_MODEL: OnceLock<Result<EmbeddingModel, String>> = OnceLock::new();

/// Sentence embedding model wrapper
pub struct EmbeddingModel {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl EmbeddingModel {
    /// Load the embedding model from HuggingFace Hub
    /// Downloads ~90MB of model files on first use, cached afterwards.
    pub fn load() -> Result<Self, String> {
        tracing::info!("Loading embedding model: {}", MODEL_ID);

        let device = Device::Cpu;

        // Download model files from HuggingFace
        let api = Api::new().map_err(|e| format!("Failed to create HF API: {}", e))?;
        let repo = api.repo(Repo::new(MODEL_ID.to_string(), RepoType::Model));

        let config_path = repo
            .get("config.json")
            .map_err(|e| format!("Failed to download config: {}", e))?;
        let tokenizer_path = repo
            .get("tokenizer.json")
            .map_err(|e| format!("Failed to download tokenizer: {}", e))?;
        let weights_path = repo
            .get("model.safetensors")
            .map_err(|e| format!("Failed to download weights: {}", e))?;

        // Load config
        let config_str = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config: {}", e))?;
        let config: Config =
            serde_json::from_str(&config_str).map_err(|e| format!("Failed to parse config: {}", e))?;

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| format!("Failed to load tokenizer: {}", e))?;

        // Load model weights
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)
                .map_err(|e| format!("Failed to load weights: {}", e))?
        };

        let model =
            BertModel::load(vb, &config).map_err(|e| format!("Failed to load model: {}", e))?;

        tracing::info!("Embedding model loaded successfully");

        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }

    /// Generate embedding for a single text
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let embeddings = self.embed_batch(&[text])?;
        Ok(embeddings.into_iter().next().unwrap())
    }

    /// Generate embeddings for multiple texts in a single forward pass
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // Tokenize all texts
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| format!("Tokenization failed: {}", e))?;

        let max_len = encodings.iter().map(|e| e.get_ids().len()).max().unwrap_or(0);
        let batch_size = texts.len();

        // Build padded input tensors
        let mut input_ids_vec = Vec::with_capacity(batch_size * max_len);
        let mut attention_mask_vec = Vec::with_capacity(batch_size * max_len);
        let mut token_type_ids_vec = Vec::with_capacity(batch_size * max_len);

        for encoding in &encodings {
            let mut ids = encoding.get_ids().to_vec();
            let mut mask = encoding.get_attention_mask().to_vec();
            let mut type_ids = encoding.get_type_ids().to_vec();

            ids.resize(max_len, 0);
            mask.resize(max_len, 0);
            type_ids.resize(max_len, 0);

            input_ids_vec.extend(ids);
            attention_mask_vec.extend(mask);
            token_type_ids_vec.extend(type_ids);
        }

        let input_ids = Tensor::from_vec(
            input_ids_vec.iter().map(|&x| x as i64).collect(),
            (batch_size, max_len),
            &self.device,
        )
        .map_err(|e| format!("Failed to create input_ids tensor: {}", e))?;

        let attention_mask = Tensor::from_vec(
            attention_mask_vec.iter().map(|&x| x as i64).collect(),
            (batch_size, max_len),
            &self.device,
        )
        .map_err(|e| format!("Failed to create attention_mask tensor: {}", e))?;

        let token_type_ids = Tensor::from_vec(
            token_type_ids_vec.iter().map(|&x| x as i64).collect(),
            (batch_size, max_len),
            &self.device,
        )
        .map_err(|e| format!("Failed to create token_type_ids tensor: {}", e))?;

        // Forward pass
        let output = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))
            .map_err(|e| format!("Model forward pass failed: {}", e))?;

        // Mean pooling over non-padded tokens
        let mask_f = attention_mask
            .to_dtype(DType::F32)
            .map_err(|e| format!("Failed to convert mask: {}", e))?;

        let mask_expanded = mask_f
            .unsqueeze(2)
            .map_err(|e| format!("Failed to expand mask: {}", e))?
            .broadcast_as(output.shape())
            .map_err(|e| format!("Failed to broadcast mask: {}", e))?;

        let sum_embeddings = (output.clone() * mask_expanded.clone())
            .map_err(|e| format!("Failed to apply mask: {}", e))?
            .sum(1)
            .map_err(|e| format!("Failed to sum: {}", e))?;

        let sum_mask = mask_expanded
            .sum(1)
            .map_err(|e| format!("Failed to sum mask: {}", e))?
            .clamp(1e-9, f64::MAX)
            .map_err(|e| format!("Failed to clamp: {}", e))?;

        let mean_embeddings = sum_embeddings
            .broadcast_div(&sum_mask)
            .map_err(|e| format!("Failed to divide: {}", e))?;

        // L2 normalization
        let norms = mean_embeddings
            .sqr()
            .map_err(|e| format!("Failed to square: {}", e))?
            .sum_keepdim(1)
            .map_err(|e| format!("Failed to sum for norm: {}", e))?
            .sqrt()
            .map_err(|e| format!("Failed to sqrt: {}", e))?
            .clamp(1e-9, f64::MAX)
            .map_err(|e| format!("Failed to clamp norm: {}", e))?;

        let normalized = mean_embeddings
            .broadcast_div(&norms)
            .map_err(|e| format!("Failed to normalize: {}", e))?;

        // Extract as Vec<Vec<f32>>
        let flat: Vec<f32> = normalized
            .to_vec2()
            .map_err(|e| format!("Failed to extract embeddings: {}", e))?
            .into_iter()
            .flatten()
            .collect();

        Ok(flat.chunks(EMBEDDING_DIM).map(|c| c.to_vec()).collect())
    }
}

/// Get or initialize the global embedding model (lazy loaded)
pub fn get_model() -> Result<&'static EmbeddingModel, String> {
    EMBEDDING_MODEL
        .get_or_init(|| EmbeddingModel::load())
        .as_ref()
        .map_err(|e| e.clone())
}

/// Generate embedding for text (uses global model)
pub fn embed_text(text: &str) -> Result<Vec<f32>, String> {
    get_model()?.embed(text)
}

/// Generate embeddings for multiple texts (uses global model)
pub fn embed_texts(texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
    get_model()?.embed_batch(texts)
}

/// Cosine similarity between two embeddings (-1 to 1)
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a < 1e-9 || norm_b < 1e-9 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

/// Serialize embedding to bytes for database BLOB storage
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Deserialize embedding from database BLOB bytes
pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_embedding_serialization_roundtrip() {
        let embedding = vec![0.1, 0.2, 0.3, 0.4, -0.5];
        let bytes = embedding_to_bytes(&embedding);
        let restored = bytes_to_embedding(&bytes);
        assert_eq!(embedding.len(), restored.len());
        for (a, b) in embedding.iter().zip(restored.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }
}
