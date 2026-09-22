use crate::{GuildAiConfig, rules};
use reqwest::{
    StatusCode,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde::Deserialize;
use std::{env, fmt, time::Duration};

const MAX_RULE_BYTES: usize = 400_000;

#[derive(Clone)]
pub(crate) struct Ai {
    config: Option<Config>,
}

#[derive(Clone)]
struct Config {
    endpoint: String,
    key: String,
    model: String,
    review_max_tokens: u64,
    summary_max_tokens: u64,
    generation_max_tokens: u64,
    reasoning_effort: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SummaryError {
    MissingConfig,
    RulesPrivate,
    NoRuleFiles,
    FilesTooLarge,
    Storage,
    BadResponse,
    ProviderRequest(AiDiagnostic),
    ProviderResponse(AiDiagnostic),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AiDiagnostic {
    pub operation: &'static str,
    pub endpoint: String,
    pub model: String,
    pub status: Option<u16>,
    pub kind: &'static str,
    pub detail: String,
}

impl AiDiagnostic {
    fn request(
        config: &Config,
        operation: &'static str,
        status: Option<StatusCode>,
        kind: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            endpoint: safe_endpoint(&config.endpoint),
            model: config.model.clone(),
            status: status.map(|status| status.as_u16()),
            kind,
            detail: truncate_detail(detail.into()),
        }
    }
}

impl fmt::Display for AiDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} using model `{}` at `{}`",
            self.operation, self.model, self.endpoint
        )?;
        if let Some(status) = self.status {
            write!(formatter, " returned HTTP {status}")?;
        }
        if !self.kind.is_empty() {
            write!(formatter, " ({})", self.kind)?;
        }
        if !self.detail.is_empty() {
            write!(formatter, ": {}", self.detail)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReviewStatus {
    Compliant,
    GrayArea,
    Violation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuleQuote {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MessageReview {
    pub status: ReviewStatus,
    pub severity: String,
    pub rule_ids: Vec<String>,
    pub reason: String,
    pub quoted_rules: Vec<RuleQuote>,
}

impl MessageReview {
    pub(crate) fn needs_action(&self) -> bool {
        matches!(
            self.status,
            ReviewStatus::GrayArea | ReviewStatus::Violation
        )
    }

    pub(crate) fn label(&self) -> &'static str {
        match self.status {
            ReviewStatus::Compliant => "compliant",
            ReviewStatus::GrayArea => "gray area",
            ReviewStatus::Violation => "violation",
        }
    }
}

fn uploaded_rules_prompt(files: &[(String, Vec<u8>)]) -> Result<String, SummaryError> {
    if files.is_empty() {
        return Err(SummaryError::NoRuleFiles);
    }
    let mut total = 0usize;
    let mut prompt = String::from(
        "Extract every Discord server rule from the uploaded files below. \
         Return only JSON matching this schema: \
         {\"rules\":[{\"id\":\"short-stable-id\",\"severity\":\"low|medium|high|critical\",\"text\":\"complete rule text\"}]}. \
         Include all rules, keep exceptions and thresholds, split separate rules, merge duplicates, \
         and do not include explanations outside JSON.\n\n",
    );
    for (name, data) in files {
        total = total
            .checked_add(data.len())
            .ok_or(SummaryError::FilesTooLarge)?;
        if total > MAX_RULE_BYTES {
            return Err(SummaryError::FilesTooLarge);
        }
        prompt.push_str("FILE: ");
        prompt.push_str(name);
        prompt.push_str("\n---\n");
        prompt.push_str(&String::from_utf8_lossy(data));
        prompt.push_str("\n---\n\n");
    }
    Ok(prompt)
}

fn channel_rules_prompt(messages: &[(String, String)]) -> Result<String, SummaryError> {
    if messages.is_empty() {
        return Err(SummaryError::NoRuleFiles);
    }
    let mut total = 0usize;
    let mut prompt = String::from(
        "Extract Discord server rules from the channel messages below. \
         Ignore chatter, questions, jokes, acknowledgements, and anything that is not a rule or rule clarification. \
         Return only JSON matching this schema: \
         {\"rules\":[{\"id\":\"short-stable-id\",\"severity\":\"low|medium|high|critical\",\"text\":\"complete rule text\"}]}. \
         Include all relevant rules, keep exceptions and thresholds, split separate rules, merge duplicates, \
         and do not include explanations outside JSON.\n\n",
    );
    for (author, content) in messages {
        total = total
            .checked_add(author.len())
            .and_then(|value| value.checked_add(content.len()))
            .ok_or(SummaryError::FilesTooLarge)?;
        if total > MAX_RULE_BYTES {
            return Err(SummaryError::FilesTooLarge);
        }
        prompt.push_str("MESSAGE BY ");
        prompt.push_str(author);
        prompt.push_str(":\n");
        prompt.push_str(content);
        prompt.push_str("\n---\n");
    }
    Ok(prompt)
}

fn review_prompt(rulebook_json: &str, message: &str) -> Result<String, SummaryError> {
    if rulebook_json.trim().is_empty() || rulebook_json.contains("\"rules\": []") {
        return Err(SummaryError::NoRuleFiles);
    }
    let total = rulebook_json
        .len()
        .checked_add(message.len())
        .ok_or(SummaryError::FilesTooLarge)?;
    if total > MAX_RULE_BYTES {
        return Err(SummaryError::FilesTooLarge);
    }
    Ok(format!(
        "Review this Discord message against every rule in the curated rules JSON. \
         Return only JSON matching this schema: \
         {{\"status\":\"compliant|gray_area|violation\",\"severity\":\"low|medium|high|critical\",\"rule_ids\":[\"matching-rule-id\"],\"reason\":\"short reason\",\"quoted_rules\":[{{\"id\":\"matching-rule-id\",\"text\":\"exact relevant rule text\"}}]}}. \
         Use gray_area when the message may violate a rule but context is missing or uncertain. \
         Use violation only when the message clearly breaks one or more rules. \
         Quote only rules from the provided JSON; do not invent rules.\n\nRULES JSON:\n{rulebook_json}\n\nMESSAGE:\n{message}"
    ))
}

pub(crate) fn rulebook_prompt(rulebook_json: &str) -> Result<String, SummaryError> {
    if rulebook_json.trim().is_empty() || rulebook_json.contains("\"rules\": []") {
        return Err(SummaryError::NoRuleFiles);
    }
    if rulebook_json.len() > MAX_RULE_BYTES {
        return Err(SummaryError::FilesTooLarge);
    }
    Ok(format!(
        "Summarize every server rule contained in this rules JSON. \
         Preserve rule ids, severities, exceptions, thresholds, and all rule meaning. \
         Write a concise numbered Markdown list grouped by severity when useful.\n\n{rulebook_json}"
    ))
}

fn reasoning_effort() -> Option<String> {
    env::var("AI_REASONING_EFFORT")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"))
}

fn token_limit(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| (1..=4096).contains(value))
        .unwrap_or(default)
}

fn safe_endpoint(endpoint: &str) -> String {
    reqwest::Url::parse(endpoint).map_or_else(
        |_| "<invalid-url>".to_owned(),
        |url| {
            let host = url.host_str().unwrap_or("<unknown-host>");
            format!("{host}{}", url.path())
        },
    )
}

fn truncate_detail(detail: String) -> String {
    let mut detail = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    if detail.len() > 500 {
        detail.truncate(500);
        detail.push('…');
    }
    detail
}

fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "api_key" | "apikey" | "key" | "token" | "authorization" | "access_token"
                ) {
                    *value = serde_json::Value::String("[redacted]".into());
                } else {
                    redact_json(value);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_json(value);
            }
        }
        _ => {}
    }
}

fn redacted_body_preview(text: &str) -> String {
    let mut value = match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => value,
        Err(_) => return truncate_detail(text.to_owned()),
    };
    redact_json(&mut value);
    truncate_detail(value.to_string())
}

impl Ai {
    pub(crate) fn from_env() -> Self {
        let key = env::var("API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let endpoint = env::var("AI_ENDPOINT_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let model = env::var("AI_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "gpt-4o-mini".into());
        let review_max_tokens = token_limit("AI_MAX_TOKENS", 300);
        let summary_max_tokens = token_limit("AI_SUMMARY_MAX_TOKENS", 1200);
        let generation_max_tokens = token_limit("AI_RULE_GENERATION_MAX_TOKENS", 2000);
        let reasoning_effort = reasoning_effort();
        Self {
            config: key.zip(endpoint).map(|(key, endpoint)| Config {
                endpoint,
                key,
                model,
                review_max_tokens,
                summary_max_tokens,
                generation_max_tokens,
                reasoning_effort,
            }),
        }
    }

    fn config_for(&self, guild_config: Option<&GuildAiConfig>) -> Result<Config, SummaryError> {
        if let Some(guild_config) = guild_config {
            return Ok(Config {
                endpoint: guild_config.endpoint.clone(),
                key: guild_config.api_key.clone(),
                model: guild_config.model.clone(),
                review_max_tokens: token_limit("AI_MAX_TOKENS", 300),
                summary_max_tokens: token_limit("AI_SUMMARY_MAX_TOKENS", 1200),
                generation_max_tokens: token_limit("AI_RULE_GENERATION_MAX_TOKENS", 2000),
                reasoning_effort: reasoning_effort(),
            });
        }
        self.config.clone().ok_or(SummaryError::MissingConfig)
    }

    pub(crate) async fn summarize_rulebook(
        &self,
        rulebook_json: &str,
        guild_config: Option<&GuildAiConfig>,
    ) -> Result<String, SummaryError> {
        let config = self.config_for(guild_config)?;
        let prompt = rulebook_prompt(rulebook_json)?;
        self.chat(
            &config,
            "summary",
            "You summarize Discord server rules exactly from the provided JSON. Do not invent rules.",
            &prompt,
            config.summary_max_tokens,
        )
        .await
    }

    pub(crate) async fn generate_rulebook(
        &self,
        files: &[(String, Vec<u8>)],
        public: bool,
        guild_config: Option<&GuildAiConfig>,
    ) -> Result<rules::RuleBook, SummaryError> {
        let config = self.config_for(guild_config)?;
        let prompt = uploaded_rules_prompt(files)?;
        let response = self
            .chat(
                &config,
                "rules-generate-files",
                "You extract Discord server rules into strict JSON. Return JSON only.",
                &prompt,
                config.generation_max_tokens,
            )
            .await?;
        parse_generated_rulebook(&response, public)
    }

    pub(crate) async fn generate_rulebook_from_messages(
        &self,
        messages: &[(String, String)],
        public: bool,
        guild_config: Option<&GuildAiConfig>,
    ) -> Result<rules::RuleBook, SummaryError> {
        let config = self.config_for(guild_config)?;
        let prompt = channel_rules_prompt(messages)?;
        let response = self
            .chat(
                &config,
                "rules-generate-channel",
                "You extract Discord server rules from channel messages into strict JSON. Return JSON only.",
                &prompt,
                config.generation_max_tokens,
            )
            .await?;
        parse_generated_rulebook(&response, public)
    }

    pub(crate) async fn review_message(
        &self,
        rulebook_json: &str,
        message: &str,
        guild_config: Option<&GuildAiConfig>,
    ) -> Result<MessageReview, SummaryError> {
        let config = self.config_for(guild_config)?;
        let prompt = review_prompt(rulebook_json, message)?;
        let response = self
            .chat(
                &config,
                "message-review",
                "You review Discord messages against provided server rules. Return strict JSON only.",
                &prompt,
                config.review_max_tokens,
            )
            .await?;
        parse_message_review(&response)
    }

    async fn chat(
        &self,
        config: &Config,
        operation: &'static str,
        system: &str,
        prompt: &str,
        max_tokens: u64,
    ) -> Result<String, SummaryError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let auth = HeaderValue::from_str(&format!("Bearer {}", config.key))
            .map_err(|_| SummaryError::MissingConfig)?;
        headers.insert(AUTHORIZATION, auth);
        let mut body = serde_json::json!({
            "model": config.model,
            "temperature": 0.1,
            "max_tokens": max_tokens,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": prompt}
            ]
        });
        if let Some(reasoning_effort) = &config.reasoning_effort {
            body["reasoning_effort"] = serde_json::Value::String(reasoning_effort.clone());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| {
                SummaryError::ProviderRequest(AiDiagnostic::request(
                    config,
                    operation,
                    None,
                    "client-build",
                    error.to_string(),
                ))
            })?;
        let response = client
            .post(&config.endpoint)
            .headers(headers)
            .body(body.to_string())
            .send()
            .await
            .map_err(|error| {
                SummaryError::ProviderRequest(AiDiagnostic::request(
                    config,
                    operation,
                    error.status(),
                    "transport",
                    error.to_string(),
                ))
            })?;
        let status = response.status();
        let response_text = response.text().await.map_err(|error| {
            SummaryError::ProviderResponse(AiDiagnostic::request(
                config,
                operation,
                Some(status),
                "body-read",
                error.to_string(),
            ))
        })?;
        if !status.is_success() {
            return Err(SummaryError::ProviderRequest(AiDiagnostic::request(
                config,
                operation,
                Some(status),
                "http",
                redacted_body_preview(&response_text),
            )));
        }
        let value: serde_json::Value = serde_json::from_str(&response_text).map_err(|error| {
            SummaryError::ProviderResponse(AiDiagnostic::request(
                config,
                operation,
                Some(status),
                "invalid-json",
                format!("{error}; body: {}", redacted_body_preview(&response_text)),
            ))
        })?;
        value["choices"][0]["message"]["content"]
            .as_str()
            .or_else(|| value["output_text"].as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                SummaryError::ProviderResponse(AiDiagnostic::request(
                    config,
                    operation,
                    Some(status),
                    "empty-content",
                    redacted_body_preview(&response_text),
                ))
            })
    }
}

fn json_slice(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    (start < end).then_some(&trimmed[start..=end])
}

#[derive(Deserialize)]
struct ProviderReview {
    status: String,
    #[serde(default = "default_low")]
    severity: String,
    #[serde(default)]
    rule_ids: Vec<String>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    quoted_rules: Vec<ProviderQuote>,
}

#[derive(Deserialize)]
struct ProviderQuote {
    id: String,
    text: String,
}

fn default_low() -> String {
    "low".into()
}

pub(crate) fn parse_message_review(text: &str) -> Result<MessageReview, SummaryError> {
    let json = json_slice(text).ok_or(SummaryError::BadResponse)?;
    let review: ProviderReview =
        serde_json::from_str(json).map_err(|_| SummaryError::BadResponse)?;
    let status = match review.status.as_str() {
        "compliant" | "allow" | "allowed" | "ok" | "no_violation" => ReviewStatus::Compliant,
        "gray_area" | "grey_area" | "gray" | "grey" | "uncertain" => ReviewStatus::GrayArea,
        "violation" | "violates" | "blocked" => ReviewStatus::Violation,
        _ => return Err(SummaryError::BadResponse),
    };
    let severity = review.severity.trim().to_ascii_lowercase();
    rules::validate_severity(&severity).map_err(|_| SummaryError::BadResponse)?;
    let mut rule_ids = Vec::new();
    for id in review.rule_ids {
        rules::validate_id(&id).map_err(|_| SummaryError::BadResponse)?;
        if !rule_ids.contains(&id) {
            rule_ids.push(id);
        }
    }
    let reason = review.reason.trim().to_owned();
    if status != ReviewStatus::Compliant && reason.is_empty() {
        return Err(SummaryError::BadResponse);
    }
    if reason.len() > 1200 {
        return Err(SummaryError::BadResponse);
    }
    let mut quoted_rules = Vec::new();
    for quote in review.quoted_rules {
        rules::validate_id(&quote.id).map_err(|_| SummaryError::BadResponse)?;
        let text = quote.text.trim().to_owned();
        if text.is_empty() || text.len() > 1200 {
            return Err(SummaryError::BadResponse);
        }
        quoted_rules.push(RuleQuote { id: quote.id, text });
    }
    Ok(MessageReview {
        status,
        severity,
        rule_ids,
        reason,
        quoted_rules,
    })
}

pub(crate) fn parse_generated_rulebook(
    text: &str,
    public: bool,
) -> Result<rules::RuleBook, SummaryError> {
    let json = json_slice(text).ok_or(SummaryError::BadResponse)?;
    let mut book: rules::RuleBook =
        serde_json::from_str(json).map_err(|_| SummaryError::BadResponse)?;
    book.public = public;
    if book.rules.is_empty() {
        return Err(SummaryError::NoRuleFiles);
    }
    for rule in &book.rules {
        rules::validate_id(&rule.id).map_err(|_| SummaryError::BadResponse)?;
        rules::validate_severity(&rule.severity).map_err(|_| SummaryError::BadResponse)?;
        if rule.text.trim().is_empty() || rule.text.len() > 3000 {
            return Err(SummaryError::BadResponse);
        }
    }
    book.rules.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rulebook_prompt_rejects_missing_or_too_large_rules() {
        assert_eq!(
            rulebook_prompt("{\"rules\": []}"),
            Err(SummaryError::NoRuleFiles)
        );
        let huge = "x".repeat(MAX_RULE_BYTES + 1);
        assert_eq!(rulebook_prompt(&huge), Err(SummaryError::FilesTooLarge));
        let prompt = rulebook_prompt(
            "{\"rules\":[{\"id\":\"spam\",\"severity\":\"medium\",\"text\":\"No spam\"}]}",
        )
        .unwrap();
        assert!(prompt.contains("spam"));
        assert!(prompt.contains("severity"));
    }

    #[test]
    fn message_review_accepts_gray_area_aliases() {
        let review = parse_message_review(
            r#"```json
{"status":"grey","severity":"high","rule_ids":["spam","spam"],"reason":"Might be spam.","quoted_rules":[{"id":"spam","text":"No spam."}]}
```"#,
        )
        .unwrap();
        assert_eq!(review.status, ReviewStatus::GrayArea);
        assert_eq!(review.rule_ids, vec!["spam"]);
        assert!(review.needs_action());
    }

    #[test]
    fn message_review_rejects_action_without_reason_or_bad_ids() {
        assert!(
            parse_message_review("{\"status\":\"violation\",\"severity\":\"medium\"}").is_err()
        );
        assert!(
            parse_message_review(
                "{\"status\":\"compliant\",\"severity\":\"low\",\"rule_ids\":[\"../bad\"]}"
            )
            .is_err()
        );
        assert!(review_prompt("{\"rules\": []}", "hello").is_err());
    }

    #[test]
    fn generated_rulebook_accepts_json_inside_provider_text() {
        let book = parse_generated_rulebook(
            "```json\n{\"rules\":[{\"id\":\"spam\",\"severity\":\"medium\",\"text\":\"No spam.\"}]}\n```",
            true,
        )
        .unwrap();
        assert!(book.public);
        assert_eq!(book.rules[0].id, "spam");
    }

    #[test]
    fn generated_rulebook_rejects_invalid_rules() {
        assert!(parse_generated_rulebook("{\"rules\":[]}", false).is_err());
        assert!(
            parse_generated_rulebook(
                "{\"rules\":[{\"id\":\"../bad\",\"severity\":\"medium\",\"text\":\"No\"}]}",
                false
            )
            .is_err()
        );
        let files = vec![("rules.txt".into(), b"No spam".to_vec())];
        assert!(uploaded_rules_prompt(&files).unwrap().contains("rules.txt"));
    }

    #[test]
    fn channel_rule_prompt_includes_messages_and_filters_guidance() {
        let messages = vec![
            ("10".to_owned(), "No advertising invite links.".to_owned()),
            ("20".to_owned(), "thanks!".to_owned()),
        ];
        let prompt = channel_rules_prompt(&messages).unwrap();
        assert!(prompt.contains("MESSAGE BY 10"));
        assert!(prompt.contains("No advertising invite links."));
        assert!(prompt.contains("Ignore chatter"));
        assert_eq!(channel_rules_prompt(&[]), Err(SummaryError::NoRuleFiles));
    }

    #[test]
    fn provider_diagnostic_redacts_secret_fields() {
        let preview = redacted_body_preview(
            r#"{"error":{"message":"bad key","api_key":"secret","nested":{"authorization":"Bearer secret"}}}"#,
        );
        assert!(preview.contains("[redacted]"));
        assert!(!preview.contains("secret"));
        assert_eq!(
            safe_endpoint("https://user:pass@example.test/v1/chat/completions?key=secret"),
            "example.test/v1/chat/completions"
        );
    }
}
