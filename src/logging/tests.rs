use super::*;
use serde_json::json;

#[test]
fn levels_filter_less_important_records() {
    for configured in Level::ALL {
        for event in Level::ALL {
            let expected = event != Level::Off && (configured as u8) >= (event as u8);
            assert_eq!(configured.allows(event), expected);
        }
        assert_eq!(Level::parse(configured.as_str()), Some(configured));
    }
    assert_eq!(Level::parse("everything"), None);
}

#[test]
fn routing_requires_matching_completed_guild_and_enabled_level() {
    let mut config = GuildConfig {
        guild_id: 1,
        setup_completed: true,
        log_channel_id: Some(10),
        log_level: Level::Info,
        retention: crate::retention::Policy::None,
        channel_ids: vec![3],
        admin_role_ids: vec![2],
    };
    assert_eq!(
        destination(&config, GuildId::new(1), Level::Info),
        Some(ChannelId::new(10))
    );
    assert_eq!(destination(&config, GuildId::new(2), Level::Info), None);
    assert_eq!(destination(&config, GuildId::new(1), Level::Debug), None);
    config.log_level = Level::Off;
    assert_eq!(destination(&config, GuildId::new(1), Level::Error), None);
    config.log_level = Level::Debug;
    config.setup_completed = false;
    assert_eq!(destination(&config, GuildId::new(1), Level::Info), None);
}

#[test]
fn events_cannot_route_dms_or_multi_guild_ready_data() {
    assert_eq!(
        event_guild(&json!({"t":"MESSAGE_CREATE", "d":{"guild_id":"12"}})),
        Some(GuildId::new(12))
    );
    assert_eq!(
        event_guild(&json!({"t":"GUILD_CREATE", "d":{"id":"12"}})),
        Some(GuildId::new(12))
    );
    assert_eq!(
        event_guild(
            &json!({"t":"MESSAGE_CREATE", "d":{"id":"12", "referenced_message":{"guild_id":"12"}}})
        ),
        None
    );
    assert_eq!(
        event_guild(&json!({"t":"READY", "d":{"guilds":[{"id":"12"},{"id":"13"}]}})),
        None
    );
}

#[test]
fn secrets_and_forwarded_content_are_redacted_recursively() {
    let mut value = json!({"token":"secret", "api_key":"secret", "nested":[{"session_id":"secret", "content":"hello", "referenced_message":{"content":"another guild"}}]});
    redact(&mut value);
    assert!(!value.to_string().contains("secret"));
    assert!(!value.to_string().contains("another guild"));
    assert_eq!(value["nested"][0]["content"], "hello");
}

#[test]
fn long_unicode_records_are_bounded_without_invalid_utf8() {
    let output = bounded(&"🙂".repeat(5000), 3800);
    assert_eq!(output.encode_utf16().count(), 3812);
    assert!(output.ends_with("[truncated]"));
}

#[test]
fn setup_audit_records_the_actual_response() {
    let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new().content("Settings saved."),
    );
    assert_eq!(setup_outcome(&response), "Settings saved.");
    assert_eq!(setup_level(&response), Level::Info);
    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new().content("You need Manage Server."),
    );
    assert_eq!(setup_outcome(&response), "You need Manage Server.");
    assert_eq!(setup_level(&response), Level::Warn);
}

#[test]
fn foreign_or_missing_destination_is_rejected() {
    assert!(belongs_to_guild(GuildId::new(1), Some(GuildId::new(1))));
    assert!(!belongs_to_guild(GuildId::new(1), Some(GuildId::new(2))));
    assert!(!belongs_to_guild(GuildId::new(1), None));
}
