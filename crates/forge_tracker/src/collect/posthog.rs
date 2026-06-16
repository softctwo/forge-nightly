use tokio::sync::OnceCell;

use super::super::Result;
use super::Collect;
use crate::Event;

/// PostHog event collector backed by the `posthog-rs` SDK.
///
/// The underlying [`posthog_rs::Client`] is lazily initialized on the first
/// [`collect`](Collect::collect) call, so `Tracker::new` can be called outside
/// an async context.
pub struct Tracker {
    api_key: String,
    client: OnceCell<posthog_rs::Client>,
}

/// Maps our domain event names to PostHog AI Observability event types.
fn to_ai_event_name(name: &str) -> &str {
    match name {
        "prompt" => "$ai_generation",
        "tool_call" => "$ai_span",
        "ai_trace" => "$ai_trace",
        _ => name,
    }
}

impl Tracker {
    pub fn new(api_key: String) -> Self {
        Self { api_key, client: OnceCell::new() }
    }

    /// Returns a reference to the initialized SDK client, creating it on the
    /// first call.
    async fn client(&self) -> &posthog_rs::Client {
        self.client
            .get_or_init(|| async { posthog_rs::client(self.api_key.as_str()).await })
            .await
    }

    /// Library metadata injected into every `$ai_*` event so PostHog can
    /// identify the SDK / agent that produced the telemetry.
    const AI_LIB: &str = "forge_tracker";
    const AI_FRAMEWORK: &str = "code-forge";

    /// Derives the project name from the current working directory (cached).
    fn project_name() -> Option<String> {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
    }

    /// Converts our domain [`Event`] into a [`posthog_rs::Event`], injecting
    /// PostHog AI Observability trace properties and user profile updates.
    ///
    /// Property naming follows the conventions used by the `@posthog/pi`
    /// extension so generic "prompt" and "tool_call" events appear in the
    /// PostHog LLM Analytics dashboard.
    fn build_event(input: &Event) -> posthog_rs::Event {
        let raw_name: String = input.event_name.clone().into();
        let event_name = to_ai_event_name(&raw_name).to_string();
        let distinct_id = input.client_id.clone();
        let mut event = posthog_rs::Event::new(event_name, distinct_id);

        // Required AI-observability metadata — always injected.
        let _ = event.insert_prop("$ai_lib", Self::AI_LIB);
        let _ = event.insert_prop("$ai_lib_version", crate::dispatch::version());
        let _ = event.insert_prop("$ai_framework", Self::AI_FRAMEWORK);
        if let Some(proj) = Self::project_name() {
            let _ = event.insert_prop("$ai_project_name", &proj);
            let _ = event.insert_prop("$ai_agent_name", &proj);
        }

        // Serialize all domain fields and inject them as PostHog properties.
        // Keys listed below are mapped to their `$ai_*` PostHog equivalents
        // and excluded from the flat property passthrough.
        if let Ok(serde_json::Value::Object(map)) = serde_json::to_value(input) {
            for (key, value) in map {
                match key.as_str() {
                    // Used as the PostHog event name and distinct_id already.
                    "event_name" | "client_id" => {}

                    // PostHog AI Observability trace properties.
                    // Mirrors the `@posthog/pi` convention of `$ai_*` prefixes.
                    "trace_id" => {
                        if let Some(s) = value.as_str() {
                            let _ = event.insert_prop("$ai_trace_id", s);
                        }
                    }
                    "session_id" => {
                        if let Some(s) = value.as_str() {
                            let _ = event.insert_prop("$ai_session_id", s);
                        }
                    }
                    "ai_span_id" => {
                        if let Some(s) = value.as_str() {
                            let _ = event.insert_prop("$ai_span_id", s);
                        }
                    }
                    "ai_parent_id" => {
                        if let Some(s) = value.as_str() {
                            let _ = event.insert_prop("$ai_parent_id", s);
                        }
                    }

                    // LLM generation metadata.
                    "model" => {
                        if let Some(s) = value.as_str() {
                            let _ = event.insert_prop("$ai_model", s);
                        }
                    }
                    "provider" => {
                        if let Some(s) = value.as_str() {
                            let _ = event.insert_prop("$ai_provider", s);
                        }
                    }

                    // Token / latency fields (when present).
                    "ai_input_tokens" => {
                        if let Some(n) = value.as_u64() {
                            let _ = event.insert_prop("$ai_input_tokens", n);
                        }
                    }
                    "ai_output_tokens" => {
                        if let Some(n) = value.as_u64() {
                            let _ = event.insert_prop("$ai_output_tokens", n);
                        }
                    }
                    "ai_total_tokens" => {
                        if let Some(n) = value.as_u64() {
                            let _ = event.insert_prop("$ai_total_tokens", n);
                        }
                    }
                    "ai_latency" => {
                        if let Some(n) = value.as_f64() {
                            let _ = event.insert_prop("$ai_latency", n);
                        }
                    }

                    // User identity: PostHog applies `$set` on the person
                    // profile linked to the distinct_id.
                    "identity" if !value.is_null() => {
                        let _ = event.insert_prop("$set", &value);
                    }

                    _ => {
                        let _ = event.insert_prop(&key, &value);
                    }
                }
            }
        }

        event
    }
}

#[async_trait::async_trait]
impl Collect for Tracker {
    async fn collect(&self, event: Event) -> Result<()> {
        let sdk_event = Self::build_event(&event);
        self.client().await.capture(sdk_event).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;
    use crate::event::{Identity, Name};

    /// Fixture: a fully populated [`Event`] with trace, session, model, and
    /// identity fields set so we can verify every special-case mapping.
    fn fixture() -> Event {
        Event {
            event_name: Name::from("test_event".to_string()),
            event_value: "test_value".to_string(),
            start_time: Utc::now(),
            cores: 4,
            client_id: "client-123".to_string(),
            os_name: "macOS".to_string(),
            up_time: 0,
            path: None,
            cwd: None,
            user: "test_user".to_string(),
            args: vec![],
            version: "1.0.0".to_string(),
            email: vec![],
            model: Some("claude-sonnet".to_string()),
            conversation: None,
            identity: Some(Identity { login: "test@example.com".to_string() }),
            trace_id: Some("trace-uuid-123".to_string()),
            session_id: Some("session-uuid-456".to_string()),
            ai_span_id: Some("span-uuid-789".to_string()),
            ai_parent_id: Some("parent-uuid-012".to_string()),
            provider: Some("anthropic".to_string()),
            ai_input_tokens: Some(150),
            ai_output_tokens: Some(80),
            ai_total_tokens: Some(230),
            ai_latency: Some(1.42),
        }
    }

    #[test]
    fn build_event_maps_trace_id_to_dollar_ai_trace_id() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        let expected = Some(&json!("trace-uuid-123"));
        assert_eq!(actual.properties().get("$ai_trace_id"), expected);
    }

    #[test]
    fn build_event_maps_session_id_to_dollar_ai_session_id() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        let expected = Some(&json!("session-uuid-456"));
        assert_eq!(actual.properties().get("$ai_session_id"), expected);
    }

    #[test]
    fn build_event_maps_model_to_dollar_ai_model() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        let expected = Some(&json!("claude-sonnet"));
        assert_eq!(actual.properties().get("$ai_model"), expected);
    }

    #[test]
    fn build_event_maps_identity_to_dollar_set() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        let expected = Some(&json!({"login": "test@example.com"}));
        assert_eq!(actual.properties().get("$set"), expected);
    }

    #[test]
    fn build_event_excludes_event_name_from_properties() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.properties().get("event_name"), None);
    }

    #[test]
    fn build_event_excludes_client_id_from_properties() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.properties().get("client_id"), None);
    }

    #[test]
    fn build_event_passes_through_regular_fields() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        let expected = Some(&json!("test_value"));
        assert_eq!(actual.properties().get("event_value"), expected);
        let expected = Some(&json!("test_user"));
        assert_eq!(actual.properties().get("user"), expected);
        let expected = Some(&json!("macOS"));
        assert_eq!(actual.properties().get("os_name"), expected);
    }

    #[test]
    fn build_event_omits_trace_id_when_none() {
        let mut input = fixture();
        input.trace_id = None;
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.properties().get("$ai_trace_id"), None);
    }

    #[test]
    fn build_event_omits_session_id_when_none() {
        let mut input = fixture();
        input.session_id = None;
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.properties().get("$ai_session_id"), None);
    }

    #[test]
    fn build_event_omits_ai_model_when_none() {
        let mut input = fixture();
        input.model = None;
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.properties().get("$ai_model"), None);
    }

    #[test]
    fn build_event_omits_dollar_set_when_identity_none() {
        let mut input = fixture();
        input.identity = None;
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.properties().get("$set"), None);
    }

    #[test]
    fn build_event_maps_prompt_to_ai_generation() {
        let mut input = fixture();
        input.event_name = Name::from("prompt".to_string());
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.event_name(), "$ai_generation");
    }

    #[test]
    fn build_event_maps_tool_call_to_ai_span() {
        let mut input = fixture();
        input.event_name = Name::from("tool_call".to_string());
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.event_name(), "$ai_span");
    }

    #[test]
    fn build_event_maps_ai_trace_to_ai_trace() {
        let mut input = fixture();
        input.event_name = Name::from("ai_trace".to_string());
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.event_name(), "$ai_trace");
    }

    #[test]
    fn build_event_passes_through_unknown_event_names() {
        let mut input = fixture();
        input.event_name = Name::from("start".to_string());
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.event_name(), "start");
    }

    #[test]
    fn build_event_stamps_ai_lib_metadata() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        assert_eq!(
            actual.properties().get("$ai_lib"),
            Some(&json!("forge_tracker"))
        );
        assert_eq!(
            actual.properties().get("$ai_framework"),
            Some(&json!("code-forge"))
        );
    }

    #[test]
    fn build_event_stamps_ai_lib_version() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        // Just verify the key exists — the version changes per build
        assert!(actual.properties().contains_key("$ai_lib_version"));
    }

    #[test]
    fn build_event_maps_ai_span_id_to_dollar_ai_span_id() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        let expected = Some(&json!("span-uuid-789"));
        assert_eq!(actual.properties().get("$ai_span_id"), expected);
    }

    #[test]
    fn build_event_maps_ai_parent_id_to_dollar_ai_parent_id() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        let expected = Some(&json!("parent-uuid-012"));
        assert_eq!(actual.properties().get("$ai_parent_id"), expected);
    }

    #[test]
    fn build_event_maps_provider_to_dollar_ai_provider() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        let expected = Some(&json!("anthropic"));
        assert_eq!(actual.properties().get("$ai_provider"), expected);
    }

    #[test]
    fn build_event_maps_token_counts_to_ai_properties() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.properties().get("$ai_input_tokens"), Some(&json!(150)));
        assert_eq!(actual.properties().get("$ai_output_tokens"), Some(&json!(80)));
        assert_eq!(actual.properties().get("$ai_total_tokens"), Some(&json!(230)));
    }

    #[test]
    fn build_event_maps_latency_to_dollar_ai_latency() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        let expected = Some(&json!(1.42));
        assert_eq!(actual.properties().get("$ai_latency"), expected);
    }

    #[test]
    fn build_event_distinct_id_uses_client_id_field() {
        let input = fixture();
        let actual = Tracker::build_event(&input);
        assert_eq!(actual.distinct_id(), "client-123");
    }
}
