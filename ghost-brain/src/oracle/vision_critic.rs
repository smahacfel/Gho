//! VisionCritic - AI-Powered Meme Quality Assessment
//!
//! This module provides AI-powered analysis of meme coin images to assess their
//! viral potential. It integrates with LLM Vision APIs (OpenAI GPT-4o-mini or
//! Anthropic Claude) to evaluate meme quality.
//!
//! # Key Features
//! - Fetch token metadata (URI → JSON → Image URL)
//! - AI-powered meme image analysis using Vision APIs
//! - Viral score calculation (0-10 scale)
//! - Feature flag for enabling/disabling API calls
//! - Configurable timeout and retry logic
//!
//! # Signal Interpretation
//! - Score < 3: Weak signal (Generic AI art, pixel art from generator)
//! - Score 3-7: Neutral signal (Average meme quality)
//! - Score > 8: Strong signal (Original, funny, trend-aware)
//!
//! # Feature Flag
//! The VisionCritic is disabled by default to prevent API costs.
//! Enable via configuration when you want AI-powered analysis.
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use ghost_brain::oracle::vision_critic::{VisionCritic, VisionCriticConfig, LlmProvider};
//! use reqwest::Client;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = VisionCriticConfig {
//!     enabled: true,
//!     provider: LlmProvider::OpenAI,
//!     api_key: Some("your-api-key".to_string()),
//!     ..Default::default()
//! };
//! let client = Client::new();
//! let critic = VisionCritic::new(config, client);
//!
//! let result = critic.analyze_meme_image("https://example.com/token.json").await?;
//! println!("Viral Score: {}/10 - {}", result.viral_score, result.reason);
//! # Ok(())
//! # }
//! ```

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// LLM Provider selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmProvider {
    /// OpenAI GPT-4o-mini (fast, cheap)
    OpenAI,
    /// Anthropic Claude Haiku (fast, cheap)
    Anthropic,
}

impl Default for LlmProvider {
    fn default() -> Self {
        LlmProvider::OpenAI
    }
}

/// Signal strength classification based on viral score
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalStrength {
    /// Score < 3: Generic AI art, pixel art from generator
    Weak,
    /// Score 3-7: Average meme quality
    Neutral,
    /// Score > 8: Original, funny, trend-aware
    Strong,
}

impl SignalStrength {
    /// Classify signal strength based on viral score
    pub fn from_score(score: u8) -> Self {
        if score < 3 {
            SignalStrength::Weak
        } else if score > 8 {
            SignalStrength::Strong
        } else {
            SignalStrength::Neutral
        }
    }
}

/// Configuration for VisionCritic
#[derive(Debug, Clone)]
pub struct VisionCriticConfig {
    /// Feature flag: Enable/disable VisionCritic (default: false)
    /// When disabled, no API calls are made and default scores are returned
    pub enabled: bool,
    /// LLM provider to use
    pub provider: LlmProvider,
    /// API key for the LLM provider
    pub api_key: Option<String>,
    /// API request timeout in seconds
    pub api_timeout_secs: u64,
    /// Maximum retries for API calls
    pub max_retries: usize,
    /// Retry delay in milliseconds
    pub retry_delay_ms: u64,
    /// Metadata fetch timeout in seconds
    pub metadata_timeout_secs: u64,
    /// Maximum image size in bytes (to avoid huge payloads)
    pub max_image_size_bytes: usize,
    /// OpenAI API endpoint (can be customized for proxies)
    pub openai_endpoint: String,
    /// Anthropic API endpoint
    pub anthropic_endpoint: String,
    /// Model name for OpenAI
    pub openai_model: String,
    /// Model name for Anthropic
    pub anthropic_model: String,
}

impl Default for VisionCriticConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default to prevent API costs
            provider: LlmProvider::OpenAI,
            api_key: None,
            api_timeout_secs: 30,
            max_retries: 2,
            retry_delay_ms: 500,
            metadata_timeout_secs: 10,
            max_image_size_bytes: 5 * 1024 * 1024, // 5MB
            openai_endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
            anthropic_endpoint: "https://api.anthropic.com/v1/messages".to_string(),
            openai_model: "gpt-4o-mini".to_string(),
            anthropic_model: "claude-3-haiku-20240307".to_string(),
        }
    }
}

/// Result of VisionCritic analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionCriticResult {
    /// Viral score from 0 to 10
    pub viral_score: u8,
    /// Human-readable explanation
    pub reason: String,
    /// Signal strength classification
    pub signal_strength: SignalStrength,
    /// Image URL that was analyzed (if available)
    pub image_url: Option<String>,
    /// Whether the analysis was performed by AI (false if disabled or fallback)
    pub ai_analyzed: bool,
    /// Analysis timestamp (Unix seconds)
    pub analyzed_at: u64,
    /// Analysis duration in milliseconds
    pub analysis_time_ms: u64,
}

impl Default for VisionCriticResult {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            viral_score: 5, // Default neutral score
            reason: "VisionCritic disabled or analysis failed".to_string(),
            signal_strength: SignalStrength::Neutral,
            image_url: None,
            ai_analyzed: false,
            analyzed_at: now,
            analysis_time_ms: 0,
        }
    }
}

/// Token metadata structure (from off-chain JSON)
#[derive(Debug, Clone, Deserialize)]
struct TokenMetadata {
    /// Token name
    #[serde(default)]
    name: Option<String>,
    /// Token symbol
    #[serde(default)]
    symbol: Option<String>,
    /// Token description
    #[serde(default)]
    description: Option<String>,
    /// Image URL
    #[serde(default)]
    image: Option<String>,
}

/// OpenAI API request structure
#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    content: Vec<OpenAIContent>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum OpenAIContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlData },
}

#[derive(Debug, Serialize)]
struct ImageUrlData {
    url: String,
}

/// OpenAI API response structure
#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessageResponse,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessageResponse {
    content: String,
}

/// Anthropic API request structure
#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContent>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum AnthropicContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
}

#[derive(Debug, Serialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

/// Anthropic API response structure
#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentResponse>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentResponse {
    text: String,
}

/// LLM response parsed from JSON output
#[derive(Debug, Deserialize)]
struct LlmScoreResponse {
    score: u8,
    reason: String,
}

/// VisionCritic - AI-Powered Meme Quality Assessor
pub struct VisionCritic {
    config: VisionCriticConfig,
    http_client: Client,
}

impl VisionCritic {
    /// Create a new VisionCritic instance
    pub fn new(config: VisionCriticConfig, http_client: Client) -> Self {
        if config.enabled {
            info!(
                "VisionCritic initialized: provider={:?}, model={}",
                config.provider,
                match config.provider {
                    LlmProvider::OpenAI => &config.openai_model,
                    LlmProvider::Anthropic => &config.anthropic_model,
                }
            );
        } else {
            info!("VisionCritic initialized but DISABLED (feature flag off)");
        }

        Self {
            config,
            http_client,
        }
    }

    /// Analyze a meme coin image for viral potential
    ///
    /// # Arguments
    /// * `metadata_uri` - The URI to the token's metadata JSON
    ///
    /// # Returns
    /// VisionCriticResult with viral score (0-10) and analysis details
    pub async fn analyze_meme_image(&self, metadata_uri: &str) -> Result<VisionCriticResult> {
        let start_time = std::time::Instant::now();

        // Check if VisionCritic is enabled
        if !self.config.enabled {
            debug!("VisionCritic disabled, returning default result");
            return Ok(VisionCriticResult {
                reason: "VisionCritic feature is disabled".to_string(),
                ..Default::default()
            });
        }

        // Check for API key
        if self.config.api_key.is_none() {
            warn!("VisionCritic enabled but no API key configured");
            return Ok(VisionCriticResult {
                reason: "No API key configured".to_string(),
                ..Default::default()
            });
        }

        // Step 1: Fetch metadata JSON
        let metadata = self
            .fetch_metadata(metadata_uri)
            .await
            .context("Failed to fetch token metadata")?;

        // Step 2: Extract image URL
        let image_url = metadata
            .image
            .ok_or_else(|| anyhow!("No image URL found in metadata"))?;

        debug!("Analyzing image: {}", image_url);

        // Step 3: Call LLM Vision API
        let result = self
            .analyze_with_llm(&image_url)
            .await
            .context("Failed to analyze image with LLM")?;

        let analysis_time_ms = start_time.elapsed().as_millis() as u64;

        let final_result = VisionCriticResult {
            viral_score: result.score.min(10), // Clamp to 0-10
            reason: result.reason,
            signal_strength: SignalStrength::from_score(result.score),
            image_url: Some(image_url),
            ai_analyzed: true,
            analyzed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            analysis_time_ms,
        };

        info!(
            "VisionCritic analysis complete: score={}/10, signal={:?}, time={}ms",
            final_result.viral_score, final_result.signal_strength, analysis_time_ms
        );

        Ok(final_result)
    }

    /// Analyze image directly from URL (bypasses metadata fetch)
    pub async fn analyze_image_url(&self, image_url: &str) -> Result<VisionCriticResult> {
        let start_time = std::time::Instant::now();

        if !self.config.enabled {
            return Ok(VisionCriticResult {
                reason: "VisionCritic feature is disabled".to_string(),
                ..Default::default()
            });
        }

        if self.config.api_key.is_none() {
            return Ok(VisionCriticResult {
                reason: "No API key configured".to_string(),
                ..Default::default()
            });
        }

        let result = self.analyze_with_llm(image_url).await?;
        let analysis_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(VisionCriticResult {
            viral_score: result.score.min(10),
            reason: result.reason,
            signal_strength: SignalStrength::from_score(result.score),
            image_url: Some(image_url.to_string()),
            ai_analyzed: true,
            analyzed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            analysis_time_ms,
        })
    }

    /// Fetch token metadata from URI
    async fn fetch_metadata(&self, uri: &str) -> Result<TokenMetadata> {
        debug!("Fetching metadata from: {}", uri);

        let response = tokio::time::timeout(
            Duration::from_secs(self.config.metadata_timeout_secs),
            self.http_client.get(uri).send(),
        )
        .await
        .context("Metadata fetch timeout")?
        .context("Failed to fetch metadata")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Metadata fetch failed with status: {}",
                response.status()
            ));
        }

        let metadata: TokenMetadata = response
            .json()
            .await
            .context("Failed to parse metadata JSON")?;

        debug!(
            "Metadata fetched: name={:?}, symbol={:?}, has_image={}",
            metadata.name,
            metadata.symbol,
            metadata.image.is_some()
        );

        Ok(metadata)
    }

    /// Call LLM Vision API to analyze the image
    async fn analyze_with_llm(&self, image_url: &str) -> Result<LlmScoreResponse> {
        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(self.config.retry_delay_ms)).await;
                debug!("Retrying LLM API call, attempt {}", attempt + 1);
            }

            let result = match self.config.provider {
                LlmProvider::OpenAI => self.call_openai(image_url).await,
                LlmProvider::Anthropic => self.call_anthropic(image_url).await,
            };

            match result {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!("LLM API call failed (attempt {}): {}", attempt + 1, e);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Unknown error in LLM API call")))
    }

    /// Call OpenAI Vision API
    async fn call_openai(&self, image_url: &str) -> Result<LlmScoreResponse> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow!("No API key configured"))?;

        let prompt = Self::build_analysis_prompt();

        let request = OpenAIRequest {
            model: self.config.openai_model.clone(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: vec![
                    OpenAIContent::Text { text: prompt },
                    OpenAIContent::ImageUrl {
                        image_url: ImageUrlData {
                            url: image_url.to_string(),
                        },
                    },
                ],
            }],
            max_tokens: 150,
            temperature: 0.3,
        };

        let response = tokio::time::timeout(
            Duration::from_secs(self.config.api_timeout_secs),
            self.http_client
                .post(&self.config.openai_endpoint)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&request)
                .send(),
        )
        .await
        .context("OpenAI API timeout")?
        .context("OpenAI API request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI API error: {} - {}", status, body));
        }

        let api_response: OpenAIResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        let content = api_response
            .choices
            .first()
            .map(|c| &c.message.content)
            .ok_or_else(|| anyhow!("Empty response from OpenAI"))?;

        Self::parse_llm_response(content)
    }

    /// Call Anthropic Claude Vision API
    async fn call_anthropic(&self, image_url: &str) -> Result<LlmScoreResponse> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow!("No API key configured"))?;

        // For Anthropic, we need to fetch the image and send as base64
        let image_data = self.fetch_image_as_base64(image_url).await?;
        let prompt = Self::build_analysis_prompt();

        let request = AnthropicRequest {
            model: self.config.anthropic_model.clone(),
            max_tokens: 150,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: vec![
                    AnthropicContent::Image {
                        source: AnthropicImageSource {
                            source_type: "base64".to_string(),
                            media_type: image_data.0,
                            data: image_data.1,
                        },
                    },
                    AnthropicContent::Text { text: prompt },
                ],
            }],
        };

        let response = tokio::time::timeout(
            Duration::from_secs(self.config.api_timeout_secs),
            self.http_client
                .post(&self.config.anthropic_endpoint)
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&request)
                .send(),
        )
        .await
        .context("Anthropic API timeout")?
        .context("Anthropic API request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Anthropic API error: {} - {}", status, body));
        }

        let api_response: AnthropicResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic response")?;

        let content = api_response
            .content
            .first()
            .map(|c| &c.text)
            .ok_or_else(|| anyhow!("Empty response from Anthropic"))?;

        Self::parse_llm_response(content)
    }

    /// Fetch image and convert to base64 for Anthropic API
    async fn fetch_image_as_base64(&self, image_url: &str) -> Result<(String, String)> {
        let response = tokio::time::timeout(
            Duration::from_secs(self.config.metadata_timeout_secs),
            self.http_client.get(image_url).send(),
        )
        .await
        .context("Image fetch timeout")?
        .context("Failed to fetch image")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Image fetch failed with status: {}",
                response.status()
            ));
        }

        // Determine media type from content-type header
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/png")
            .to_string();

        // Extract just the media type (e.g., "image/png" from "image/png; charset=utf-8")
        let media_type = content_type
            .split(';')
            .next()
            .unwrap_or("image/png")
            .trim()
            .to_string();

        let bytes = response
            .bytes()
            .await
            .context("Failed to read image bytes")?;

        // Check size limit
        if bytes.len() > self.config.max_image_size_bytes {
            return Err(anyhow!(
                "Image too large: {} bytes (max: {})",
                bytes.len(),
                self.config.max_image_size_bytes
            ));
        }

        use base64::Engine;
        let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);

        Ok((media_type, base64_data))
    }

    /// Build the analysis prompt for the LLM
    fn build_analysis_prompt() -> String {
        "Rate this meme coin image on a scale 1-10 for uniqueness and viral potential. \
         Consider: originality, humor, cultural relevance, meme potential, and production quality. \
         Generic AI art or basic pixel art should score low (1-3). \
         Original, funny, or trend-aware memes should score high (8-10). \
         Output JSON only: {\"score\": number, \"reason\": string}"
            .to_string()
    }

    /// Parse the LLM response to extract score and reason
    fn parse_llm_response(content: &str) -> Result<LlmScoreResponse> {
        // Try to find JSON in the response
        let json_start = content.find('{');
        let json_end = content.rfind('}');

        if let (Some(start), Some(end)) = (json_start, json_end) {
            let json_str = &content[start..=end];
            if let Ok(response) = serde_json::from_str::<LlmScoreResponse>(json_str) {
                return Ok(response);
            }
        }

        // Fallback: try to extract score from text
        warn!("Failed to parse JSON response, attempting text extraction");

        // Look for patterns like "score: 7", "7/10", or standalone single digits
        let score = Self::extract_score_from_text(content);

        Ok(LlmScoreResponse {
            score: score.min(10),
            reason: content.chars().take(200).collect(),
        })
    }

    /// Extract score from text using pattern matching
    fn extract_score_from_text(content: &str) -> u8 {
        let content_lower = content.to_lowercase();

        // Pattern 1: Look for "X/10" pattern
        if let Some(idx) = content.find("/10") {
            if idx > 0 {
                // Look for digits before "/10"
                let prefix = &content[..idx];
                let num_str: String = prefix
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit() || c.is_whitespace())
                    .collect::<String>()
                    .chars()
                    .rev()
                    .filter(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(score) = num_str.parse::<u8>() {
                    return score.min(10);
                }
            }
        }

        // Pattern 2: Look for "score:" or "score " followed by a number
        for pattern in &["score:", "score "] {
            if let Some(idx) = content_lower.find(pattern) {
                let after = &content[idx + pattern.len()..];
                let num_str: String = after
                    .chars()
                    .skip_while(|c| c.is_whitespace())
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(score) = num_str.parse::<u8>() {
                    return score.min(10);
                }
            }
        }

        // Pattern 3: Look for first single digit (1-9) as fallback
        for c in content.chars() {
            if c.is_ascii_digit() && c != '0' {
                if let Some(digit) = c.to_digit(10) {
                    return (digit as u8).min(10);
                }
            }
        }

        // Default neutral score if no pattern matched
        5
    }

    /// Check if VisionCritic is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the configured LLM provider
    pub fn provider(&self) -> LlmProvider {
        self.config.provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> VisionCriticConfig {
        VisionCriticConfig::default()
    }

    #[test]
    fn test_vision_critic_config_default() {
        let config = VisionCriticConfig::default();

        assert!(!config.enabled); // Should be disabled by default
        assert_eq!(config.provider, LlmProvider::OpenAI);
        assert!(config.api_key.is_none());
        assert_eq!(config.api_timeout_secs, 30);
        assert_eq!(config.max_retries, 2);
        assert_eq!(config.openai_model, "gpt-4o-mini");
        assert_eq!(config.anthropic_model, "claude-3-haiku-20240307");
    }

    #[test]
    fn test_vision_critic_result_default() {
        let result = VisionCriticResult::default();

        assert_eq!(result.viral_score, 5); // Default neutral score
        assert_eq!(result.signal_strength, SignalStrength::Neutral);
        assert!(!result.ai_analyzed);
        assert!(result.image_url.is_none());
    }

    #[test]
    fn test_signal_strength_from_score() {
        // Weak signal (< 3)
        assert_eq!(SignalStrength::from_score(0), SignalStrength::Weak);
        assert_eq!(SignalStrength::from_score(1), SignalStrength::Weak);
        assert_eq!(SignalStrength::from_score(2), SignalStrength::Weak);

        // Neutral signal (3-8)
        assert_eq!(SignalStrength::from_score(3), SignalStrength::Neutral);
        assert_eq!(SignalStrength::from_score(5), SignalStrength::Neutral);
        assert_eq!(SignalStrength::from_score(8), SignalStrength::Neutral);

        // Strong signal (> 8)
        assert_eq!(SignalStrength::from_score(9), SignalStrength::Strong);
        assert_eq!(SignalStrength::from_score(10), SignalStrength::Strong);
    }

    #[test]
    fn test_llm_provider_default() {
        let provider = LlmProvider::default();
        assert_eq!(provider, LlmProvider::OpenAI);
    }

    #[test]
    fn test_parse_llm_response_json() {
        let json_response = r#"{"score": 8, "reason": "Original meme with good humor"}"#;
        let result = VisionCritic::parse_llm_response(json_response).unwrap();

        assert_eq!(result.score, 8);
        assert_eq!(result.reason, "Original meme with good humor");
    }

    #[test]
    fn test_parse_llm_response_json_in_text() {
        let response =
            r#"Here is my analysis: {"score": 7, "reason": "Good meme potential"} Thanks!"#;
        let result = VisionCritic::parse_llm_response(response).unwrap();

        assert_eq!(result.score, 7);
        assert_eq!(result.reason, "Good meme potential");
    }

    #[test]
    fn test_parse_llm_response_fallback() {
        let response = "I give this a score of 6 out of 10";
        let result = VisionCritic::parse_llm_response(response).unwrap();

        // Should extract 6 from "score of 6"
        assert_eq!(result.score, 6);
        assert!(result.score <= 10);
    }

    #[test]
    fn test_extract_score_slash_pattern() {
        // Test "X/10" pattern
        assert_eq!(VisionCritic::extract_score_from_text("This is 7/10"), 7);
        assert_eq!(VisionCritic::extract_score_from_text("Rating: 8 / 10"), 8);
        assert_eq!(VisionCritic::extract_score_from_text("10/10 perfect"), 10);

        // Test clamping for values > 10
        assert_eq!(VisionCritic::extract_score_from_text("15/10"), 10);
    }

    #[test]
    fn test_extract_score_score_pattern() {
        // Test "score: X" pattern
        assert_eq!(VisionCritic::extract_score_from_text("score: 7"), 7);
        assert_eq!(VisionCritic::extract_score_from_text("My score: 9"), 9);
        assert_eq!(VisionCritic::extract_score_from_text("score 5"), 5);
    }

    #[test]
    fn test_extract_score_single_digit_fallback() {
        // Test single digit fallback
        assert_eq!(VisionCritic::extract_score_from_text("I rate this 8"), 8);
        assert_eq!(VisionCritic::extract_score_from_text("Pretty good, 7"), 7);
    }

    #[test]
    fn test_extract_score_no_valid_pattern() {
        // Should return default 5 if no pattern found
        assert_eq!(VisionCritic::extract_score_from_text("No numbers here"), 5);
        assert_eq!(VisionCritic::extract_score_from_text(""), 5);
    }

    #[test]
    fn test_build_analysis_prompt() {
        let prompt = VisionCritic::build_analysis_prompt();

        assert!(prompt.contains("1-10"));
        assert!(prompt.contains("viral potential"));
        assert!(prompt.contains("JSON"));
        assert!(prompt.contains("score"));
        assert!(prompt.contains("reason"));
    }

    #[test]
    fn test_vision_critic_result_serialization() {
        let result = VisionCriticResult {
            viral_score: 9,
            reason: "Excellent meme".to_string(),
            signal_strength: SignalStrength::Strong,
            image_url: Some("https://example.com/meme.png".to_string()),
            ai_analyzed: true,
            analyzed_at: 1234567890,
            analysis_time_ms: 1500,
        };

        let json = serde_json::to_string(&result).expect("Serialization failed");
        let deserialized: VisionCriticResult =
            serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(deserialized.viral_score, 9);
        assert_eq!(deserialized.signal_strength, SignalStrength::Strong);
        assert!(deserialized.ai_analyzed);
        assert_eq!(deserialized.analysis_time_ms, 1500);
    }

    #[test]
    fn test_signal_strength_serialization() {
        let strengths = vec![
            SignalStrength::Weak,
            SignalStrength::Neutral,
            SignalStrength::Strong,
        ];

        for strength in strengths {
            let json = serde_json::to_string(&strength).expect("Serialization failed");
            let deserialized: SignalStrength =
                serde_json::from_str(&json).expect("Deserialization failed");
            assert_eq!(strength, deserialized);
        }
    }

    #[test]
    fn test_llm_provider_serialization() {
        let providers = vec![LlmProvider::OpenAI, LlmProvider::Anthropic];

        for provider in providers {
            let json = serde_json::to_string(&provider).expect("Serialization failed");
            let deserialized: LlmProvider =
                serde_json::from_str(&json).expect("Deserialization failed");
            assert_eq!(provider, deserialized);
        }
    }

    #[tokio::test]
    async fn test_vision_critic_disabled() {
        let config = VisionCriticConfig {
            enabled: false,
            ..Default::default()
        };
        let client = Client::new();
        let critic = VisionCritic::new(config, client);

        assert!(!critic.is_enabled());

        let result = critic
            .analyze_meme_image("https://example.com/metadata.json")
            .await
            .unwrap();

        assert!(!result.ai_analyzed);
        assert_eq!(result.viral_score, 5); // Default neutral
        assert!(result.reason.contains("disabled"));
    }

    #[tokio::test]
    async fn test_vision_critic_no_api_key() {
        let config = VisionCriticConfig {
            enabled: true,
            api_key: None,
            ..Default::default()
        };
        let client = Client::new();
        let critic = VisionCritic::new(config, client);

        let result = critic
            .analyze_meme_image("https://example.com/metadata.json")
            .await
            .unwrap();

        assert!(!result.ai_analyzed);
        assert!(result.reason.contains("API key"));
    }

    #[test]
    fn test_config_custom_values() {
        let config = VisionCriticConfig {
            enabled: true,
            provider: LlmProvider::Anthropic,
            api_key: Some("test-key".to_string()),
            api_timeout_secs: 60,
            max_retries: 5,
            retry_delay_ms: 1000,
            openai_model: "gpt-4-vision-preview".to_string(),
            anthropic_model: "claude-3-opus-20240229".to_string(),
            ..Default::default()
        };

        assert!(config.enabled);
        assert_eq!(config.provider, LlmProvider::Anthropic);
        assert_eq!(config.api_key, Some("test-key".to_string()));
        assert_eq!(config.api_timeout_secs, 60);
        assert_eq!(config.max_retries, 5);
    }
}
