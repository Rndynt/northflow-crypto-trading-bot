//! Claude-style LLM engine with timeout + fallback.

use crate::errors::{Result, ScalperError};
use crate::llm::context_builder::MarketContext;
use crate::llm::prompts::ARIA_SYSTEM_PROMPT;
use crate::llm::response_parser::parse_trade_decision;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Decision {
    Go,
    NoGo,
    Wait,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeDecision {
    pub decision: Decision,
    pub direction: String,
    pub confidence: u8,
    pub entry_price: Option<f64>,
    pub sl_adjustment: Option<f64>,
    pub tp_adjustment: Option<f64>,
    /// LLM-recommended position size as fraction of max size (0.0 - 1.0)
    /// Based on conviction, Kelly criterion, and risk factors
    #[serde(default = "default_position_size")]
    pub position_size_pct: f64,
    pub reasoning: DecisionReasoning,
    pub market_context_score: ContextScore,
}

fn default_position_size() -> f64 {
    0.5 // Default 50% size if LLM doesn't specify
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionReasoning {
    pub summary: String,
    pub ta_analysis: String,
    /// Microstructure: OFI direction, VPIN level — replaces hallucination-prone
    /// "sentiment_analysis" and "fundamental_analysis" fields.
    #[serde(default, alias = "microstructure")]
    pub microstructure: String,
    pub risk_factors: String,
    pub invalidation: String,
    // Legacy fields — kept for backwards compat with old LLM responses,
    // ignored in new prompt.
    #[serde(default, skip_serializing)]
    pub sentiment_analysis: String,
    #[serde(default, skip_serializing)]
    pub fundamental_analysis: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct ContextScore {
    pub ta_score: u8,
    /// Microstructure score: OFI + VPIN quality.
    #[serde(default, alias = "microstructure_score")]
    pub microstructure_score: u8,
    #[serde(default)]
    pub sentiment_score: u8,
    pub risk_score: u8,
    pub composite_score: u8,
    // Legacy alias
    #[serde(default, skip_serializing)]
    pub fundamental_score: u8,
}

/// LLM provider — wire format differs between Anthropic-native and the
/// OpenAI-compatible APIs (OpenRouter, OpenAI, Together, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    Anthropic,
    OpenAiCompatible,
}

impl LlmProvider {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Self::Anthropic,
            // "openrouter", "openai", "together", "groq", ... — all share the
            // OpenAI chat-completions wire format.
            _ => Self::OpenAiCompatible,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmEngineConfig {
    pub provider: LlmProvider,
    pub api_key: String,
    pub api_base: String,
    pub model: String,
    pub timeout_secs: u64,
    pub max_tokens: u32,
    pub fallback_ta_threshold: u8,
    /// Optional HTTP-Referer/X-Title for OpenRouter rankings (free).
    pub http_referer: Option<String>,
    pub http_app_title: Option<String>,
}

pub struct LlmEngine {
    client: Client,
    cfg: LlmEngineConfig,
}

pub struct LlmCallResult {
    pub decision: TradeDecision,
    pub latency_ms: u64,
    pub offline_fallback: bool,
}

impl LlmEngine {
    pub fn new(cfg: LlmEngineConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(cfg.timeout_secs + 2))
            .user_agent("ARIA-Scalper/0.1")
            .pool_max_idle_per_host(4)
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { client, cfg }
    }

    pub async fn analyze(&self, ctx: &MarketContext) -> Result<LlmCallResult> {
        let t0 = Instant::now();
        if self.cfg.api_key.is_empty() {
            warn!("LLM api key empty — running TA-only fallback");
            return Ok(LlmCallResult {
                decision: Self::fallback_decision(ctx, self.cfg.fallback_ta_threshold),
                latency_ms: 0,
                offline_fallback: true,
            });
        }

        let prompt = ctx.build_prompt();
        let price = ctx.current_price;

        match timeout(
            Duration::from_secs(self.cfg.timeout_secs),
            self.call_api(&prompt, price),
        )
        .await
        {
            Ok(Ok(d)) => Ok(LlmCallResult {
                decision: d,
                latency_ms: t0.elapsed().as_millis() as u64,
                offline_fallback: false,
            }),
            Ok(Err(e)) => {
                // Log the FULL error so operator knows exactly why LLM failed
                warn!(
                    error = %e,
                    provider = ?self.cfg.provider,
                    model = %self.cfg.model,
                    api_base = %self.cfg.api_base,
                    timeout_secs = self.cfg.timeout_secs,
                    "❌ LLM call failed — check API key, model name, and network"
                );
                Ok(LlmCallResult {
                    decision: Self::fallback_decision(ctx, self.cfg.fallback_ta_threshold),
                    latency_ms: t0.elapsed().as_millis() as u64,
                    offline_fallback: true,
                })
            }
            Err(_) => {
                warn!(
                    timeout_secs = self.cfg.timeout_secs,
                    model = %self.cfg.model,
                    "❌ LLM timeout — API too slow or unreachable"
                );
                Ok(LlmCallResult {
                    decision: Self::fallback_decision(ctx, self.cfg.fallback_ta_threshold),
                    latency_ms: t0.elapsed().as_millis() as u64,
                    offline_fallback: true,
                })
            }
        }
    }

    async fn call_api(&self, prompt: &str, current_price: f64) -> Result<TradeDecision> {
        match self.cfg.provider {
            LlmProvider::Anthropic => self.call_anthropic(prompt, current_price).await,
            LlmProvider::OpenAiCompatible => self.call_openai_compat(prompt, current_price).await,
        }
    }

    /// Anthropic Messages API — `POST /v1/messages` with `x-api-key`.
    async fn call_anthropic(&self, prompt: &str, current_price: f64) -> Result<TradeDecision> {
        let body = serde_json::json!({
            "model": self.cfg.model,
            "max_tokens": self.cfg.max_tokens,
            "system": ARIA_SYSTEM_PROMPT,
            "messages": [{ "role": "user", "content": prompt }]
        });

        let resp: serde_json::Value = self
            .client
            .post(&self.cfg.api_base)
            .header("x-api-key", &self.cfg.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        let text = resp
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|b| b.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| ScalperError::Llm(format!("empty response: {resp}")))?;

        info!(llm_raw = %text, "LLM response");
        let d = parse_trade_decision(text)?;
        Ok(sanitize_prices(d, current_price))
    }

    /// OpenAI-compatible chat completions API — used by OpenRouter, OpenAI,
    /// Together, Groq, etc. `POST /chat/completions` with bearer auth.
    async fn call_openai_compat(&self, prompt: &str, current_price: f64) -> Result<TradeDecision> {
        let body = serde_json::json!({
            "model": self.cfg.model,
            "max_completion_tokens": self.cfg.max_tokens,
            "thinking": { "type": "disabled" },
            "temperature": 0.1,   // 0.0 causes empty responses on some APIs including Mimo
            "stream": false,
            // top_p intentionally omitted — not supported by all providers (Mimo, Together, etc.)
            "messages": [
                { "role": "system", "content": ARIA_SYSTEM_PROMPT },
                { "role": "user",   "content": prompt }
            ]
        });

        let mut req = self
            .client
            .post(&self.cfg.api_base)
            .header("api-key", &self.cfg.api_key)
            .json(&body);
        if let Some(ref r) = self.cfg.http_referer {
            req = req.header("HTTP-Referer", r);
        }
        if let Some(ref t) = self.cfg.http_app_title {
            req = req.header("X-Title", t);
        }

        // Read raw bytes first so we can log what the API actually returned
        let http_resp = req.send().await?;
        let status = http_resp.status();
        let raw_bytes = http_resp.bytes().await?;
        let raw_str = String::from_utf8_lossy(&raw_bytes);

        if raw_bytes.is_empty() || raw_str.trim().is_empty() {
            warn!(status = %status, "LLM returned empty body — check API key, model name, and parameters");
            return Err(ScalperError::Llm(format!(
                "empty body (HTTP {status}) — API may not support these parameters"
            )));
        }

        let resp: serde_json::Value = serde_json::from_str(&raw_str)
            .map_err(|e| ScalperError::Llm(format!("json parse error: {e} | raw={raw_str}")))?;

        // Check for API error response (e.g. {"error": {...}})
        if let Some(err) = resp.get("error") {
            warn!(api_error = %err, "LLM API returned error object");
            return Err(ScalperError::Llm(format!("API error: {err}")));
        }

        // Some reasoning models (MiMo, DeepSeek-R1) put the response in
        // "reasoning_content" instead of "content". Check both.
        let message = resp
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"));

        let text = message
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                message
                    .and_then(|m| m.get("reasoning_content"))
                    .and_then(|t| t.as_str())
            })
            .unwrap_or("");

        if text.is_empty() {
            return Err(ScalperError::Llm(format!("empty response: {resp}")));
        }

        info!(llm_raw = %text, "LLM response");
        let d = parse_trade_decision(text)?;
        Ok(sanitize_prices(d, current_price))
    }

    /// Freeform LLM call — used by the learning agent for qualitative analysis.
    /// Returns the raw text response (caller parses it).
    pub async fn analyze_text(
        &self,
        system_prompt: &str,
        user_content: &str,
    ) -> anyhow::Result<String> {
        if self.cfg.api_key.is_empty() {
            return Err(anyhow::anyhow!("LLM api key empty"));
        }
        let result = timeout(
            Duration::from_secs(60),
            self.call_text_api(system_prompt, user_content),
        )
        .await
        .map_err(|_| anyhow::anyhow!("LLM text call timed out after 60s"))??;
        Ok(result)
    }

    async fn call_text_api(
        &self,
        system_prompt: &str,
        user_content: &str,
    ) -> anyhow::Result<String> {
        match self.cfg.provider {
            LlmProvider::Anthropic => {
                let body = serde_json::json!({
                    "model": self.cfg.model,
                    "max_tokens": 1024,
                    "system": system_prompt,
                    "messages": [{ "role": "user", "content": user_content }]
                });
                let resp: serde_json::Value = self
                    .client
                    .post(&self.cfg.api_base)
                    .header("x-api-key", &self.cfg.api_key)
                    .header("anthropic-version", "2023-06-01")
                    .json(&body)
                    .send()
                    .await?
                    .json()
                    .await?;
                let text = resp
                    .get("content")
                    .and_then(|c| c.get(0))
                    .and_then(|b| b.get("text"))
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| anyhow::anyhow!("empty anthropic response: {resp}"))?;
                Ok(text.to_string())
            }
            LlmProvider::OpenAiCompatible => {
                let body = serde_json::json!({
                    "model": self.cfg.model,
                    "max_completion_tokens": 1024,
                    "temperature": 0.1,
                    "stream": false,
                    "messages": [
                        { "role": "system", "content": system_prompt },
                        { "role": "user",   "content": user_content }
                    ]
                });
                let mut req = self
                    .client
                    .post(&self.cfg.api_base)
                    .header("api-key", &self.cfg.api_key)
                    .json(&body);
                if let Some(ref r) = self.cfg.http_referer {
                    req = req.header("HTTP-Referer", r);
                }
                if let Some(ref t) = self.cfg.http_app_title {
                    req = req.header("X-Title", t);
                }

                let raw_bytes = req.send().await?.bytes().await?;
                let raw_str = String::from_utf8_lossy(&raw_bytes);
                let resp: serde_json::Value = serde_json::from_str(&raw_str)
                    .map_err(|e| anyhow::anyhow!("json parse: {e} | raw={raw_str}"))?;
                if let Some(err) = resp.get("error") {
                    return Err(anyhow::anyhow!("API error: {err}"));
                }
                let message = resp
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"));
                let text = message
                    .and_then(|m| m.get("content"))
                    .and_then(|t| t.as_str())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        message
                            .and_then(|m| m.get("reasoning_content"))
                            .and_then(|t| t.as_str())
                    })
                    .ok_or_else(|| anyhow::anyhow!("empty openai response: {resp}"))?;
                Ok(text.to_string())
            }
        }
    }

    fn fallback_decision(ctx: &MarketContext, threshold: u8) -> TradeDecision {
        let go = ctx.ta_confidence >= threshold;
        TradeDecision {
            decision: if go { Decision::Go } else { Decision::NoGo },
            direction: if go {
                ctx.pre_signal_direction.clone()
            } else {
                "NONE".into()
            },
            confidence: ctx.ta_confidence,
            entry_price: None,
            sl_adjustment: None,
            tp_adjustment: None,
            position_size_pct: 0.5,
            reasoning: DecisionReasoning {
                summary: "LLM unavailable — TA-only fallback mode".into(),
                ta_analysis: format!("TA confidence: {}/100", ctx.ta_confidence),
                microstructure: "LLM offline — microstructure not evaluated".into(),
                risk_factors: format!("LLM offline — raised TA threshold to {threshold}+"),
                invalidation: "Any TA signal reversal".into(),
                sentiment_analysis: String::new(),
                fundamental_analysis: String::new(),
            },
            market_context_score: ContextScore {
                ta_score: ctx.ta_confidence,
                microstructure_score: 0,
                sentiment_score: 0,
                risk_score: 50,
                composite_score: ctx.ta_confidence,
                fundamental_score: 0,
            },
        }
    }
}

/// Validate LLM-provided prices against current market price.
/// Catches hallucinated prices (e.g. LLM returns BTC SL of $100 when price is $67k).
/// If prices are unreasonable, nullify them so the system uses its own computed values.
fn sanitize_prices(mut d: TradeDecision, current_price: f64) -> TradeDecision {
    if current_price <= 0.0 {
        return d;
    }

    // Entry must be within 1% of current price — reject stale/hallucinated entries
    if let Some(entry) = d.entry_price {
        let deviation = (entry - current_price).abs() / current_price;
        if deviation > 0.01 {
            warn!(
                entry,
                current_price,
                deviation_pct = deviation * 100.0,
                "sanitize: entry price too far from market — nullifying"
            );
            d.entry_price = None;
            d.sl_adjustment = None;
            d.tp_adjustment = None;
            return d;
        }
    }

    // SL must be 0.1% – 3% away from entry (or current_price if entry is null)
    let reference = d.entry_price.unwrap_or(current_price);
    if let Some(sl) = d.sl_adjustment {
        let dist = (sl - reference).abs() / reference;
        if !(0.001..=0.03).contains(&dist) {
            warn!(
                sl,
                reference,
                dist_pct = dist * 100.0,
                "sanitize: SL distance unreasonable — nullifying SL/TP"
            );
            d.sl_adjustment = None;
            d.tp_adjustment = None;
            return d;
        }
    }

    // TP must be at least 1.5× SL distance (minimum R:R)
    if let (Some(sl), Some(tp)) = (d.sl_adjustment, d.tp_adjustment) {
        let sl_dist = (sl - reference).abs();
        let tp_dist = (tp - reference).abs();
        if sl_dist > 0.0 && tp_dist < sl_dist * 1.4 {
            warn!(
                sl,
                tp, sl_dist, tp_dist, "sanitize: R:R below 1.4 — nullifying TP"
            );
            d.tp_adjustment = None;
        }
    }

    d
}
