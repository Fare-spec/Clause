use crate::{
    GuildAiConfig, GuildConfig, Handler, auto_delete_enabled, delete_ai_config, get_ai_config,
    get_guild_config, logging::Level, metrics, metrics_forwarding_enabled, retention, rules,
    save_ai_config, set_auto_delete, set_metrics_forwarding, storage,
};
use serenity::all::*;

const CHANNEL_RULE_IMPORT_LIMIT: i64 = 1_000;

pub(crate) fn commands() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("storage")
            .description("Show this server's used and available storage")
            .dm_permission(false),
        metrics_command(),
        CreateCommand::new("disable")
            .description("Disable Clause for this server until /setup is run again")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .dm_permission(false),
        CreateCommand::new("leave")
            .description("Server-owner command to make Clause leave this server")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .dm_permission(false)
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Boolean,
                    "confirm",
                    "Set to true to make Clause leave this server",
                )
                .required(true),
            )
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Boolean,
                    "delete_data",
                    "Set to true to delete this guild's Clause folder and settings first",
                )
                .required(true),
            ),
        CreateCommand::new("settings")
            .description("Show this server's logging, retention, and privacy settings")
            .dm_permission(false),
        CreateCommand::new("summary")
            .description("Summarize this server's curated rules with AI")
            .dm_permission(false),
        rules_command(),
        ai_command(),
        CreateCommand::new("logs")
            .description("Manage this server's retained logs")
            .dm_permission(false)
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "clear",
                    "Delete retained local logs to free guild storage",
                )
                .add_sub_option(
                    CreateCommandOption::new(
                        CommandOptionType::Boolean,
                        "confirm",
                        "Set to true to delete retained local logs",
                    )
                    .required(true),
                ),
            ),
        CreateCommand::new("privacy").description(
            "Read the privacy policy, server logging settings, and data request contact",
        ),
        CreateCommand::new("terms").description("Read Clause's terms of service"),
    ]
}

fn severity_option(required: bool) -> CreateCommandOption {
    CreateCommandOption::new(CommandOptionType::String, "severity", "Rule severity")
        .required(required)
        .add_string_choice("low", "low")
        .add_string_choice("medium", "medium")
        .add_string_choice("high", "high")
        .add_string_choice("critical", "critical")
}

fn rule_id_option() -> CreateCommandOption {
    CreateCommandOption::new(CommandOptionType::String, "id", "Stable rule id")
        .required(true)
        .max_length(64)
}

fn rule_text_option(required: bool) -> CreateCommandOption {
    CreateCommandOption::new(CommandOptionType::String, "text", "Rule text")
        .required(required)
        .max_length(3000)
}

fn rules_command() -> CreateCommand {
    CreateCommand::new("rules")
        .description("Manage this server's curated AI rules")
        .dm_permission(false)
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "list",
            "View the curated rules if they are public or you can manage the bot",
        ))
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "export",
            "Export the curated rules JSON",
        ))
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "generate",
            "Regenerate all curated rules from uploaded files using AI",
        ))
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "from-channel",
                "Regenerate rules from recent messages in a channel using AI",
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::Channel,
                    "channel",
                    "Channel containing rule notes or policy messages",
                )
                .required(true)
                .channel_types(vec![ChannelType::Text]),
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::Integer,
                    "limit",
                    "Recent messages to inspect, 1-1000; default 1000",
                )
                .min_int_value(1)
                .max_int_value(CHANNEL_RULE_IMPORT_LIMIT as u64),
            ),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "visibility",
                "Choose whether ordinary members can view the curated rules",
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::Boolean,
                    "public",
                    "Whether ordinary members can view /rules list",
                )
                .required(true),
            ),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "add",
                "Add or replace a rule in the curated rules JSON",
            )
            .add_sub_option(rule_id_option())
            .add_sub_option(severity_option(true))
            .add_sub_option(rule_text_option(true)),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "update",
                "Update an existing rule's severity or text",
            )
            .add_sub_option(rule_id_option())
            .add_sub_option(severity_option(false))
            .add_sub_option(rule_text_option(false)),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "remove",
                "Remove a rule from the curated rules JSON",
            )
            .add_sub_option(rule_id_option()),
        )
}

fn metrics_command() -> CreateCommand {
    CreateCommand::new("metrics")
        .description("Show local guild metrics and control metrics forwarding")
        .dm_permission(false)
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "show",
            "Show this server's local Clause metrics",
        ))
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "forwarding",
                "Allow or disallow future operator metrics forwarding for this server",
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::Boolean,
                    "enabled",
                    "Whether Clause may forward aggregate metrics for this server",
                )
                .required(true),
            ),
        )
}

fn ai_command() -> CreateCommand {
    CreateCommand::new("ai")
        .description("Configure this server's AI provider")
        .dm_permission(false)
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "show",
            "Show this server's AI provider settings without revealing the key",
        ))
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "set",
                "Set this server's AI endpoint, model, and API key",
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "endpoint",
                    "OpenAI-compatible chat completions URL",
                )
                .required(true)
                .max_length(500),
            )
            .add_sub_option(
                CreateCommandOption::new(CommandOptionType::String, "model", "Provider model name")
                    .required(true)
                    .max_length(100),
            )
            .add_sub_option(
                CreateCommandOption::new(CommandOptionType::String, "api_key", "Provider API key")
                    .required(true)
                    .max_length(2000),
            ),
        )
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "clear",
            "Remove this server's AI override and use environment defaults",
        ))
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "test",
            "Send a small health-check request to this server's AI provider",
        ))
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "auto-delete",
                "Enable or disable high-confidence AI message deletion",
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::Boolean,
                    "enabled",
                    "Whether high-confidence high/critical violations may be deleted",
                )
                .required(true),
            ),
        )
}

fn bytes(value: u64) -> String {
    format!("{:.2} Mo ({value} bytes)", value as f64 / 1_000_000.0)
}

fn storage_embed(usage: &storage::Usage) -> CreateEmbed {
    let mut embed = CreateEmbed::new().title("Guild storage").colour(0x5865F2)
        .field("Available", bytes(usage.available()), false)
        .field("Used", bytes(usage.total), true)
        .field("Limit", bytes(storage::limit_bytes()), true)
        .field("Uploads", bytes(usage.uploads), true)
        .field("Retained logs", bytes(usage.logs), true)
        .field("Metadata and other files", bytes(usage.other), true)
        .footer(CreateEmbedFooter::new("Snapshot after retention cleanup; this command's audit log may consume additional space."));
    if usage.total > storage::limit_bytes() {
        embed = embed.field(
            "Over quota",
            bytes(usage.total - storage::limit_bytes()),
            false,
        );
    }
    embed
}

struct MetricsSnapshot {
    config: GuildConfig,
    usage: storage::Usage,
    upload_files: usize,
    rule_count: usize,
    rules_public: bool,
    has_guild_ai_config: bool,
    forwarding_enabled: bool,
    auto_delete_enabled: bool,
    today: metrics::Totals,
    seven_days: metrics::Totals,
    all_time: metrics::Totals,
}

fn yes_no(value: bool) -> &'static str {
    if value { "enabled" } else { "disabled" }
}

fn metrics_line(totals: &metrics::Totals) -> String {
    format!(
        "Messages: {} · AI reviews: {} · ok/gray/violations: {}/{}/{} · AI errors: {} · deleted: {} · delete failures: {} · staff pings: {} · replies: {} · rule-source msgs: {} · rules imported: {} · commands: {} · uploads +/−: {}/{} · tokens in/out/total: {}/{}/{}",
        totals.messages_seen,
        totals.ai_reviews,
        totals.ai_compliant,
        totals.ai_gray_area,
        totals.ai_violations,
        totals.ai_errors,
        totals.ai_deleted_messages,
        totals.ai_delete_failures,
        totals.staff_review_pings,
        totals.bot_replies,
        totals.rule_source_messages,
        totals.rules_imported,
        totals.commands_used,
        totals.uploads_added,
        totals.uploads_removed,
        totals.input_tokens,
        totals.output_tokens,
        totals.total_tokens,
    )
}

fn metrics_embed(snapshot: &MetricsSnapshot) -> CreateEmbed {
    CreateEmbed::new()
        .title("Clause metrics for this server")
        .colour(0x5865F2)
        .field("Storage used", bytes(snapshot.usage.total), true)
        .field("Storage available", bytes(snapshot.usage.available()), true)
        .field("Uploaded files", snapshot.upload_files.to_string(), true)
        .field("Uploads size", bytes(snapshot.usage.uploads), true)
        .field("Retained logs size", bytes(snapshot.usage.logs), true)
        .field("Other metadata", bytes(snapshot.usage.other), true)
        .field("Curated rules", snapshot.rule_count.to_string(), true)
        .field(
            "Rules visibility",
            if snapshot.rules_public { "public" } else { "private" },
            true,
        )
        .field(
            "AI provider",
            if snapshot.has_guild_ai_config {
                "server override"
            } else if env_ai_configured() {
                "environment default"
            } else {
                "not configured"
            },
            true,
        )
        .field("AI review channels", snapshot.config.channel_ids.len().to_string(), true)
        .field("Manager roles", snapshot.config.admin_role_ids.len().to_string(), true)
        .field("AI auto-delete", yes_no(snapshot.auto_delete_enabled), true)
        .field("Today", metrics_line(&snapshot.today), false)
        .field("Last 7 days", metrics_line(&snapshot.seven_days), false)
        .field("All time", metrics_line(&snapshot.all_time), false)
        .field(
            "Metrics forwarding allowed",
            yes_no(snapshot.forwarding_enabled),
            true,
        )
        .footer(CreateEmbedFooter::new(
            "Local metrics are aggregate counts and sizes; they do not include message text, filenames, rule text, usernames, or API keys.",
        ))
}

fn id_mentions(ids: &[i64], prefix: &str) -> String {
    if ids.is_empty() {
        "None configured".into()
    } else {
        ids.iter()
            .filter_map(|id| u64::try_from(*id).ok())
            .map(|id| format!("<{prefix}{id}>"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn message_visibility(config: &GuildConfig) -> &'static str {
    if matches!(config.retention, retention::Policy::All(_)) {
        "Message events are retained locally under this server's retention period."
    } else if config.log_level == Level::Debug {
        "Debug logging can post message content to the configured Discord log channel."
    } else {
        "Message content is not retained locally by the current policy."
    }
}

fn env_ai_configured() -> bool {
    std::env::var("API_KEY").is_ok_and(|value| !value.trim().is_empty())
        && std::env::var("AI_ENDPOINT_URL").is_ok_and(|value| !value.trim().is_empty())
}

fn ai_review_visibility(has_guild_ai_config: bool) -> &'static str {
    if has_guild_ai_config {
        "Enabled through this server's private AI override: messages in configured bot channels are checked against curated rules. Violations and gray areas notify managers."
    } else if env_ai_configured() {
        "Enabled when curated rules exist: messages in configured bot channels are checked against rules by the configured AI endpoint. Violations and gray areas notify managers."
    } else {
        "Not configured: a bot manager can use /ai set, or the operator can set API_KEY and AI_ENDPOINT_URL, to enable summaries, rule generation, and message review."
    }
}

fn settings_embed(
    config: &GuildConfig,
    usage: Option<&storage::Usage>,
    has_guild_ai_config: bool,
    forwarding_enabled: bool,
    auto_delete_enabled: bool,
) -> CreateEmbed {
    let log_channel = config
        .log_channel_id
        .and_then(|id| u64::try_from(id).ok())
        .filter(|id| *id != 0)
        .map(|id| format!("<#{id}>"))
        .unwrap_or_else(|| "None configured".into());
    let rule_source = config
        .rule_source_channel_id
        .and_then(|id| u64::try_from(id).ok())
        .filter(|id| *id != 0)
        .map(|id| format!("<#{id}>"))
        .unwrap_or_else(|| "None configured".into());
    let storage = usage.map_or_else(
        || "Storage unavailable".into(),
        |usage| {
            format!(
                "{} available; {} used by uploads and {} by retained logs",
                bytes(usage.available()),
                bytes(usage.uploads),
                bytes(usage.logs)
            )
        },
    );
    CreateEmbed::new()
        .title("Clause settings for this server")
        .colour(0x5865F2)
        .field("Discord log level", config.log_level.as_str(), true)
        .field("Local retention", config.retention.label(), true)
        .field("Log channel", log_channel, false)
        .field("Bot channels", id_mentions(&config.channel_ids, "#"), false)
        .field(
            "Bot manager roles",
            id_mentions(&config.admin_role_ids, "@&"),
            false,
        )
        .field("Message visibility", message_visibility(config), false)
        .field("Rule source channel", rule_source, false)
        .field(
            "AI rule review",
            ai_review_visibility(has_guild_ai_config),
            false,
        )
        .field("AI auto-delete", yes_no(auto_delete_enabled), true)
        .field(
            "Metrics forwarding allowed",
            yes_no(forwarding_enabled),
            true,
        )
        .field("Storage", storage, false)
        .field(
            "Privacy contact",
            std::env::var("PRIVACY_CONTACT").unwrap_or_else(|_| "git-spectre@proton.me".into()),
            false,
        )
        .footer(CreateEmbedFooter::new(
            "Raw logs are restricted. Use /privacy for policy details and personal data requests.",
        ))
}

fn summary_error(error: &crate::ai::SummaryError) -> (&'static str, Level) {
    match error {
        &crate::ai::SummaryError::MissingConfig => (
            "AI is not configured. Set API_KEY, AI_ENDPOINT_URL, and AI_MODEL before using /summary or /rules generate.",
            Level::Warn,
        ),
        &crate::ai::SummaryError::RulesPrivate => (
            "Rules are private for this server. Ask a bot manager to make them public or request the summary.",
            Level::Warn,
        ),
        &crate::ai::SummaryError::NoRuleFiles => (
            "No rules were found. A bot manager can add rules with /rules add or upload rule files and run /rules generate.",
            Level::Warn,
        ),
        &crate::ai::SummaryError::FilesTooLarge => (
            "The rules input is too large for one AI request. Split or shorten the uploaded files or curated rules first.",
            Level::Warn,
        ),
        &crate::ai::SummaryError::Storage => (
            "Could not read curated rules. Ask the bot operator to check guild storage.",
            Level::Error,
        ),
        &crate::ai::SummaryError::ProviderRequest(_) => (
            "The AI request failed. Check API_KEY, AI_ENDPOINT_URL, AI_MODEL, and provider availability.",
            Level::Error,
        ),
        &crate::ai::SummaryError::BadResponse | &crate::ai::SummaryError::ProviderResponse(_) => (
            "The AI provider returned an unexpected response.",
            Level::Error,
        ),
    }
}

fn ai_error_detail(error: &crate::ai::SummaryError) -> String {
    match error {
        crate::ai::SummaryError::ProviderRequest(diagnostic)
        | crate::ai::SummaryError::ProviderResponse(diagnostic) => diagnostic.to_string(),
        _ => format!("{error:?}"),
    }
}

fn disable_guild_settings(db: &rusqlite::Connection, guild: i64) -> rusqlite::Result<()> {
    db.execute_batch("SAVEPOINT disable_guild")?;
    let result = (|| -> rusqlite::Result<()> {
        db.execute(
            "INSERT INTO guild_configs (guild_id, setup_completed, log_channel_id, rule_source_channel_id, log_level, retention)
            VALUES (?1, 0, NULL, NULL, 'off', 'none')
            ON CONFLICT(guild_id) DO UPDATE SET
            setup_completed = 0, log_channel_id = NULL, rule_source_channel_id = NULL, log_level = 'off', retention = 'none'",
            [guild],
        )?;
        db.execute(
            "DELETE FROM guild_manager_roles WHERE guild_id = ?1",
            [guild],
        )?;
        db.execute(
            "DELETE FROM guild_bot_channels WHERE guild_id = ?1",
            [guild],
        )?;
        delete_ai_config(db, guild)?;
        set_metrics_forwarding(db, guild, false)?;
        set_auto_delete(db, guild, false)?;
        Ok(())
    })();
    if result.is_err() {
        db.execute_batch("ROLLBACK TO disable_guild")?;
    }
    db.execute_batch("RELEASE disable_guild")?;
    result
}

fn delete_guild_settings(db: &rusqlite::Connection, guild: i64) -> rusqlite::Result<()> {
    db.execute_batch("SAVEPOINT delete_guild")?;
    let result = (|| -> rusqlite::Result<()> {
        db.execute(
            "DELETE FROM guild_manager_roles WHERE guild_id = ?1",
            [guild],
        )?;
        db.execute(
            "DELETE FROM guild_bot_channels WHERE guild_id = ?1",
            [guild],
        )?;
        delete_ai_config(db, guild)?;
        db.execute(
            "DELETE FROM guild_metrics_settings WHERE guild_id = ?1",
            [guild],
        )?;
        db.execute(
            "DELETE FROM guild_moderation_settings WHERE guild_id = ?1",
            [guild],
        )?;
        db.execute(
            "DELETE FROM guild_metrics_daily WHERE guild_id = ?1",
            [guild],
        )?;
        db.execute("DELETE FROM guild_configs WHERE guild_id = ?1", [guild])?;
        Ok(())
    })();
    if result.is_err() {
        db.execute_batch("ROLLBACK TO delete_guild")?;
    }
    db.execute_batch("RELEASE delete_guild")?;
    result
}

enum RulesAction {
    List,
    Export,
    Generate,
    FromChannel {
        channel: ChannelId,
        limit: u16,
    },
    Visibility(bool),
    Add {
        id: String,
        severity: String,
        text: String,
    },
    Update {
        id: String,
        severity: Option<String>,
        text: Option<String>,
    },
    Remove(String),
}

fn string_arg(options: &[ResolvedOption<'_>], name: &str) -> Option<String> {
    options.iter().find_map(|option| match option {
        ResolvedOption {
            name: option_name,
            value: ResolvedValue::String(value),
            ..
        } if *option_name == name => Some((*value).to_owned()),
        _ => None,
    })
}

fn bool_arg(options: &[ResolvedOption<'_>], name: &str) -> Option<bool> {
    options.iter().find_map(|option| match option {
        ResolvedOption {
            name: option_name,
            value: ResolvedValue::Boolean(value),
            ..
        } if *option_name == name => Some(*value),
        _ => None,
    })
}

fn integer_arg(options: &[ResolvedOption<'_>], name: &str) -> Option<i64> {
    options.iter().find_map(|option| match option {
        ResolvedOption {
            name: option_name,
            value: ResolvedValue::Integer(value),
            ..
        } if *option_name == name => Some(*value),
        _ => None,
    })
}

fn channel_arg(options: &[ResolvedOption<'_>], name: &str) -> Option<ChannelId> {
    options.iter().find_map(|option| match option {
        ResolvedOption {
            name: option_name,
            value: ResolvedValue::Channel(channel),
            ..
        } if *option_name == name => Some(channel.id),
        ResolvedOption {
            name: option_name,
            value: ResolvedValue::Unresolved(Unresolved::Channel(id)),
            ..
        } if *option_name == name => Some(*id),
        _ => None,
    })
}

fn parse_rules_action(command: &CommandInteraction) -> Result<RulesAction, String> {
    let options = command.data.options();
    let Some(ResolvedOption {
        name,
        value: ResolvedValue::SubCommand(options),
        ..
    }) = options.first()
    else {
        return Err("Choose list, export, generate, visibility, add, update, or remove.".into());
    };
    match *name {
        "list" => Ok(RulesAction::List),
        "export" => Ok(RulesAction::Export),
        "generate" => Ok(RulesAction::Generate),
        "from-channel" => Ok(RulesAction::FromChannel {
            channel: channel_arg(options, "channel").ok_or("Choose a channel.")?,
            limit: integer_arg(options, "limit")
                .unwrap_or(CHANNEL_RULE_IMPORT_LIMIT)
                .clamp(1, CHANNEL_RULE_IMPORT_LIMIT) as u16,
        }),
        "visibility" => Ok(RulesAction::Visibility(
            bool_arg(options, "public").ok_or("Choose whether rules are public.")?,
        )),
        "add" => Ok(RulesAction::Add {
            id: string_arg(options, "id").ok_or("Choose a rule id.")?,
            severity: string_arg(options, "severity").ok_or("Choose a severity.")?,
            text: string_arg(options, "text").ok_or("Write the rule text.")?,
        }),
        "update" => {
            let severity = string_arg(options, "severity");
            let text = string_arg(options, "text");
            if severity.is_none() && text.is_none() {
                return Err("Choose a new severity, new text, or both.".into());
            }
            Ok(RulesAction::Update {
                id: string_arg(options, "id").ok_or("Choose a rule id.")?,
                severity,
                text,
            })
        }
        "remove" => Ok(RulesAction::Remove(
            string_arg(options, "id").ok_or("Choose a rule id.")?,
        )),
        _ => Err("Choose list, export, generate, visibility, add, update, or remove.".into()),
    }
}

fn manager_authorized(config: &GuildConfig, member: Option<&Member>) -> bool {
    let Some(member) = member else {
        return false;
    };
    let permissions = member.permissions.unwrap_or_default();
    permissions.administrator()
        || permissions.manage_guild()
        || member
            .roles
            .iter()
            .any(|role| config.admin_role_ids.contains(&(role.get() as i64)))
}

fn operator_notice(operator: Option<&str>, contact: Option<&str>) -> String {
    match (operator.filter(|s| !s.trim().is_empty()), contact.filter(|s| !s.trim().is_empty())) {
        (Some(operator), Some(contact)) => format!("Operator: {operator}\nPrivate contact for data access, correction, deletion, and issue reports: {contact}\nProvide your Discord user ID and relevant server/message IDs privately. Never send passwords or tokens."),
        _ => "The operator has not configured a complete privacy contact. Ask the server owner to identify the bot operator. This deployment needs an operator name and private request contact before public use.".into(),
    }
}

impl Handler {
    pub(crate) fn guild_ai_config(&self, guild: GuildId) -> Option<GuildAiConfig> {
        let db = self.database.lock().ok()?;
        get_ai_config(&db, guild.get() as i64).ok().flatten()
    }

    async fn disable_command(&self, ctx: &Context, command: &CommandInteraction) {
        let Some(guild) = command.guild_id.filter(|_| command.member.is_some()) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Use /disable inside your server."),
                    ),
                )
                .await;
            return;
        };
        if command.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }
        let allowed = command
            .member
            .as_deref()
            .and_then(|member| member.permissions)
            .is_some_and(|permissions| permissions.administrator() || permissions.manage_guild());
        if !allowed {
            let _ = command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new()
                        .content("You need Administrator or Manage Server to disable Clause."),
                )
                .await;
            return;
        }

        let database = self.database.clone();
        let root = self.storage_root.clone();
        let lock = self.storage_lock.clone();
        let disabled =
            tokio::task::spawn_blocking(move || -> Result<retention::ClearReport, String> {
                let db = database
                    .lock()
                    .map_err(|_| "Settings are unavailable.".to_owned())?;
                disable_guild_settings(&db, guild.get() as i64)
                    .map_err(|_| "Could not disable Clause for this server.".to_owned())?;
                let _guard = lock
                    .lock()
                    .map_err(|_| "Storage is unavailable.".to_owned())?;
                retention::clear(&root, guild.get())
                    .or_else(|_| Ok(retention::ClearReport { files: 0, bytes: 0 }))
            })
            .await;

        let (message, level) = match disabled {
            Ok(Ok(report)) => (
                format!(
                    "Clause is disabled for this server. Run /setup to enable it again. Cleared {} retained local log file(s), freeing {} bytes. Uploaded files and curated rules were kept.",
                    report.files, report.bytes
                ),
                Level::Warn,
            ),
            Ok(Err(error)) => (error, Level::Error),
            Err(_) => (
                "Could not disable Clause for this server.".to_owned(),
                Level::Error,
            ),
        };
        let sent = command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content(message.clone()),
            )
            .await
            .is_ok();
        self.logger.log(
            ctx,
            guild,
            if sent { level } else { Level::Error },
            format!(
                "/disable for user {}: {}; response {}",
                command.user.id,
                message,
                if sent { "sent" } else { "failed" }
            ),
        );
    }

    async fn leave_command(&self, ctx: &Context, command: &CommandInteraction) {
        let Some(guild) = command.guild_id.filter(|_| command.member.is_some()) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Use /leave inside your server."),
                    ),
                )
                .await;
            return;
        };
        if command.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }
        let options = command.data.options();
        let confirm = bool_arg(&options, "confirm").unwrap_or(false);
        let delete_data = bool_arg(&options, "delete_data").unwrap_or(false);
        if !confirm {
            let _ = command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new()
                        .content("Set `confirm:true` to make Clause leave this server."),
                )
                .await;
            return;
        }
        let owner = match guild.to_partial_guild(&ctx.http).await {
            Ok(guild) => guild.owner_id,
            Err(_) => {
                let _ = command
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new()
                            .content("Could not verify the server owner. Try again later."),
                    )
                    .await;
                return;
            }
        };
        if command.user.id != owner {
            let _ = command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new().content(
                        "Only the Discord server owner can make Clause leave or delete all guild data.",
                    ),
                )
                .await;
            return;
        }

        if delete_data {
            let database = self.database.clone();
            let root = self.storage_root.clone();
            let lock = self.storage_lock.clone();
            let cache = self.rules.clone();
            let deleted = tokio::task::spawn_blocking(move || -> Result<bool, String> {
                let db = database
                    .lock()
                    .map_err(|_| "Settings are unavailable.".to_owned())?;
                delete_guild_settings(&db, guild.get() as i64)
                    .map_err(|_| "Could not delete this guild's settings.".to_owned())?;
                let _guard = lock
                    .lock()
                    .map_err(|_| "Storage is unavailable.".to_owned())?;
                if let Ok(mut cache) = cache.lock() {
                    cache.forget(guild.get());
                }
                storage::delete_guild(&root, guild.get())
                    .map_err(|_| "Could not delete this guild's folder.".to_owned())
            })
            .await;
            match deleted {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    let _ = command
                        .edit_response(&ctx.http, EditInteractionResponse::new().content(error))
                        .await;
                    return;
                }
                Err(_) => {
                    let _ = command
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new()
                                .content("Could not delete this guild's data."),
                        )
                        .await;
                    return;
                }
            }
        }

        let left = guild.leave(&ctx.http).await;
        let message = if left.is_ok() {
            if delete_data {
                "Clause deleted this guild's local data and left the server."
            } else {
                "Clause left the server. Local data was kept for the operator to clean up or restore later."
            }
        } else {
            "Data deletion finished if requested, but Clause could not leave the server. Remove the bot from Discord manually or try again."
        };
        let _ = command
            .edit_response(&ctx.http, EditInteractionResponse::new().content(message))
            .await;
        if left.is_err() {
            self.logger.log(
                ctx,
                guild,
                Level::Error,
                format!("/leave for user {} failed to leave server", command.user.id),
            );
        }
    }

    async fn ai_command(&self, ctx: &Context, command: &CommandInteraction) {
        let Some(guild) = command.guild_id.filter(|_| command.member.is_some()) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Use /ai inside your server."),
                    ),
                )
                .await;
            return;
        };
        if command.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }
        let options = command.data.options();
        let Some(ResolvedOption {
            name,
            value: ResolvedValue::SubCommand(options),
            ..
        }) = options.first()
        else {
            let _ = command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new().content("Choose show, set, clear, or test."),
                )
                .await;
            return;
        };

        let database = self.database.clone();
        let member = command.member.clone();
        let action = (*name).to_owned();
        if action == "test" {
            let loaded =
                tokio::task::spawn_blocking(move || -> Result<Option<GuildAiConfig>, String> {
                    let db = database
                        .lock()
                        .map_err(|_| "Settings are unavailable.".to_owned())?;
                    let config = get_guild_config(&db, guild.get() as i64)
                        .map_err(|_| "Could not load settings.".to_owned())?
                        .filter(|config| config.setup_completed)
                        .ok_or_else(|| "Run /setup before testing AI.".to_owned())?;
                    if !manager_authorized(&config, member.as_deref()) {
                        return Err("Only bot managers can test AI.".into());
                    }
                    get_ai_config(&db, guild.get() as i64)
                        .map_err(|_| "Could not load AI settings.".to_owned())
                })
                .await;
            let (message, level) = match loaded {
                Ok(Ok(ai_config)) => match self.ai.test_provider(ai_config.as_ref()).await {
                    Ok(reply) => {
                        self.record_ai_tokens(guild, reply.usage);
                        (
                            format!(
                                "AI test succeeded. Provider replied: `{}`",
                                reply.value.chars().take(500).collect::<String>()
                            ),
                            Level::Info,
                        )
                    }
                    Err(error) => {
                        let (public, level) = summary_error(&error);
                        (
                            format!("{public}\nDiagnostic: {}", ai_error_detail(&error)),
                            level,
                        )
                    }
                },
                Ok(Err(error)) => (error, Level::Warn),
                Err(_) => ("Could not test AI.".into(), Level::Error),
            };
            let sent = command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new().content(message.clone()),
                )
                .await
                .is_ok();
            self.logger.log(
                ctx,
                guild,
                if sent { level } else { Level::Error },
                format!(
                    "/ai test for user {}: {}; response {}",
                    command.user.id,
                    message,
                    if sent { "sent" } else { "failed" }
                ),
            );
            return;
        }

        let endpoint = string_arg(options, "endpoint");
        let model = string_arg(options, "model");
        let api_key = string_arg(options, "api_key");
        let enabled = bool_arg(options, "enabled");
        let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let db = database.lock().map_err(|_| "Settings are unavailable.".to_owned())?;
            let config = get_guild_config(&db, guild.get() as i64)
                .map_err(|_| "Could not load settings.".to_owned())?
                .filter(|config| config.setup_completed)
                .ok_or_else(|| "Run /setup before configuring AI.".to_owned())?;
            if !manager_authorized(&config, member.as_deref()) {
                return Err("Only bot managers can configure AI.".into());
            }
            match action.as_str() {
                "show" => {
                    if let Some(ai) = get_ai_config(&db, guild.get() as i64)
                        .map_err(|_| "Could not load AI settings.".to_owned())?
                    {
                        Ok(format!(
                            "Server AI override is set. Endpoint: `{}` · Model: `{}` · API key: configured (hidden).",
                            ai.endpoint, ai.model
                        ))
                    } else {
                        Ok("No server AI override is set. Clause uses the environment AI settings.".into())
                    }
                }
                "set" => {
                    let endpoint = endpoint.ok_or_else(|| "Choose an endpoint.".to_owned())?;
                    let model = model.ok_or_else(|| "Choose a model.".to_owned())?;
                    let api_key = api_key.ok_or_else(|| "Provide an API key.".to_owned())?;
                    let parsed = reqwest::Url::parse(&endpoint)
                        .map_err(|_| "Endpoint must be a valid URL.".to_owned())?;
                    if parsed.scheme() != "https" {
                        return Err("Endpoint must use https.".into());
                    }
                    if api_key.trim().is_empty() || model.trim().is_empty() {
                        return Err("Model and API key cannot be empty.".into());
                    }
                    save_ai_config(
                        &db,
                        &GuildAiConfig {
                            guild_id: guild.get() as i64,
                            endpoint,
                            api_key,
                            model,
                        },
                    )
                    .map_err(|_| "Could not save AI settings.".to_owned())?;
                    Ok("Server AI provider saved. API key is configured and hidden.".into())
                }
                "clear" => {
                    delete_ai_config(&db, guild.get() as i64)
                        .map_err(|_| "Could not clear AI settings.".to_owned())?;
                    Ok("Server AI provider override cleared. Clause will use environment AI settings.".into())
                }
                "auto-delete" => {
                    let enabled = enabled.ok_or_else(|| "Choose whether auto-delete is enabled.".to_owned())?;
                    set_auto_delete(&db, guild.get() as i64, enabled)
                        .map_err(|_| "Could not save auto-delete setting.".to_owned())?;
                    Ok(format!(
                        "AI auto-delete is now {}. Only non-manager high/critical violations with at least 90% AI confidence may be deleted.",
                        yes_no(enabled)
                    ))
                }
                _ => Err("Choose show, set, clear, test, or auto-delete.".into()),
            }
        })
        .await;

        let (message, level) = match result {
            Ok(Ok(message)) => (message, Level::Info),
            Ok(Err(error)) => (error, Level::Warn),
            Err(_) => ("Could not configure AI.".into(), Level::Error),
        };
        let sent = command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content(message.clone()),
            )
            .await
            .is_ok();
        self.logger.log(
            ctx,
            guild,
            if sent { level } else { Level::Error },
            format!(
                "/ai {name} for user {}: {}; response {}",
                command.user.id,
                if matches!(*name, "set") {
                    "provider saved"
                } else {
                    message.as_str()
                },
                if sent { "sent" } else { "failed" }
            ),
        );
    }

    pub(crate) async fn public_command(&self, ctx: &Context, command: &CommandInteraction) {
        if let Some(guild) = command.guild_id {
            self.record_metric(guild, metrics::Counter::CommandsUsed, 1);
        }
        if command.data.name == "storage" {
            self.storage_command(ctx, command).await;
            return;
        }
        if command.data.name == "metrics" {
            self.metrics_command(ctx, command).await;
            return;
        }
        if command.data.name == "disable" {
            self.disable_command(ctx, command).await;
            return;
        }
        if command.data.name == "leave" {
            self.leave_command(ctx, command).await;
            return;
        }
        if command.data.name == "settings" {
            self.settings_command(ctx, command).await;
            return;
        }
        if command.data.name == "summary" {
            self.summary_command(ctx, command).await;
            return;
        }
        if command.data.name == "rules" {
            self.rules_command(ctx, command).await;
            return;
        }
        if command.data.name == "ai" {
            self.ai_command(ctx, command).await;
            return;
        }
        if command.data.name == "logs" {
            self.logs_command(ctx, command).await;
            return;
        }
        let operator =
            Some(std::env::var("BOT_OPERATOR").unwrap_or_else(|_| "Clause operator".into()));
        let contact = Some(
            std::env::var("PRIVACY_CONTACT").unwrap_or_else(|_| "git-spectre@proton.me".into()),
        );
        let notice = operator_notice(operator.as_deref(), contact.as_deref());
        let (name, document) = if command.data.name == "privacy" {
            ("PRIVACY.md", include_str!("../PRIVACY.md"))
        } else {
            ("TERMS.md", include_str!("../TERMS.md"))
        };
        let mut description = notice.chars().take(1500).collect::<String>();
        if command.data.name == "privacy" {
            description.push_str("\n\nRaw logs can contain other members' data and are not publicly downloadable. Ask the operator about your own data; requests are reviewed manually. There is no automatic personal-data export or deletion command.");
            if let Some(guild) = command.guild_id {
                let settings = self
                    .database
                    .lock()
                    .ok()
                    .and_then(|db| get_guild_config(&db, guild.get() as i64).ok())
                    .flatten();
                match settings {
                    Some(config) if config.setup_completed => description.push_str(&format!("\n\nThis server: Discord logging **{}**; disk retention **{}**. Discord channel permissions determine who can see posted logs.", config.log_level.as_str(), config.retention.label())),
                    _ => description.push_str("\n\nServer logging settings are unavailable or setup is incomplete."),
                }
            }
        }
        let response = CreateInteractionResponseMessage::new()
            .ephemeral(true)
            .content(description)
            .add_file(CreateAttachment::bytes(document.as_bytes(), name))
            .allowed_mentions(
                CreateAllowedMentions::new()
                    .all_users(false)
                    .all_roles(false)
                    .everyone(false),
            );
        let result = command
            .create_response(&ctx.http, CreateInteractionResponse::Message(response))
            .await;
        if let Some(guild) = command.guild_id {
            self.logger.log(
                ctx,
                guild,
                if result.is_ok() {
                    Level::Info
                } else {
                    Level::Error
                },
                format!(
                    "/{} policy response for user {}: {}",
                    command.data.name,
                    command.user.id,
                    if result.is_ok() { "sent" } else { "failed" }
                ),
            );
        }
        if result.is_err() {
            eprintln!("Could not deliver policy response.");
        }
    }

    async fn metrics_command(&self, ctx: &Context, command: &CommandInteraction) {
        let Some(guild) = command.guild_id.filter(|_| command.member.is_some()) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Use /metrics inside your server."),
                    ),
                )
                .await;
            return;
        };
        if command.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }
        let options = command.data.options();
        let Some(ResolvedOption {
            name,
            value: ResolvedValue::SubCommand(options),
            ..
        }) = options.first()
        else {
            let _ = command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new().content("Choose show or forwarding."),
                )
                .await;
            return;
        };

        let database = self.database.clone();
        let root = self.storage_root.clone();
        let lock = self.storage_lock.clone();
        let cache = self.rules.clone();
        let member = command.member.clone();
        let action = (*name).to_owned();
        let detail_action = action.clone();
        let enabled = bool_arg(options, "enabled");
        let result = tokio::task::spawn_blocking(move || -> Result<MetricsSnapshot, String> {
            let db = database
                .lock()
                .map_err(|_| "Settings are unavailable.".to_owned())?;
            let config = get_guild_config(&db, guild.get() as i64)
                .map_err(|_| "Could not load settings.".to_owned())?
                .filter(|config| config.setup_completed)
                .ok_or_else(|| "Run /setup before checking metrics.".to_owned())?;
            if !manager_authorized(&config, member.as_deref()) {
                return Err("Only bot managers can view or change metrics settings.".into());
            }
            if action == "forwarding" {
                let enabled =
                    enabled.ok_or_else(|| "Choose whether forwarding is enabled.".to_owned())?;
                set_metrics_forwarding(&db, guild.get() as i64, enabled)
                    .map_err(|_| "Could not save metrics settings.".to_owned())?;
            } else if action != "show" {
                return Err("Choose show or forwarding.".into());
            }
            let forwarding_enabled = metrics_forwarding_enabled(&db, guild.get() as i64)
                .map_err(|_| "Could not load metrics settings.".to_owned())?;
            let auto_delete_enabled = auto_delete_enabled(&db, guild.get() as i64)
                .map_err(|_| "Could not load moderation settings.".to_owned())?;
            let today = metrics::today_totals(&db, guild.get() as i64)
                .map_err(|_| "Could not load metrics totals.".to_owned())?;
            let seven_days = metrics::seven_day_totals(&db, guild.get() as i64)
                .map_err(|_| "Could not load metrics totals.".to_owned())?;
            let all_time = metrics::all_time_totals(&db, guild.get() as i64)
                .map_err(|_| "Could not load metrics totals.".to_owned())?;
            let has_guild_ai_config = get_ai_config(&db, guild.get() as i64)
                .map_err(|_| "Could not load AI settings.".to_owned())?
                .is_some();
            let _guard = lock
                .lock()
                .map_err(|_| "Storage is unavailable.".to_owned())?;
            retention::prune(&root, guild.get(), config.retention, retention::now())
                .map_err(|_| "Could not clean retained logs before reading metrics.".to_owned())?;
            let usage = storage::stats(&root, guild.get())
                .map_err(|_| "Could not inspect storage.".to_owned())?;
            let (files, _) = storage::list(&root, guild.get())
                .map_err(|_| "Could not inspect uploads.".to_owned())?;
            let upload_files = files
                .iter()
                .filter(|(name, _)| name != storage::LIMIT_FILE)
                .count();
            let mut cache = cache
                .lock()
                .map_err(|_| "Rules cache is unavailable.".to_owned())?;
            let rulebook = cache
                .get(&root, guild.get())
                .map_err(|_| "Could not inspect curated rules.".to_owned())?;
            Ok(MetricsSnapshot {
                config,
                usage,
                upload_files,
                rule_count: rulebook.rules.len(),
                rules_public: rulebook.public,
                has_guild_ai_config,
                forwarding_enabled,
                auto_delete_enabled,
                today,
                seven_days,
                all_time,
            })
        })
        .await;

        let (response, level, detail) = match result {
            Ok(Ok(snapshot)) => (
                EditInteractionResponse::new().embed(metrics_embed(&snapshot)),
                Level::Info,
                format!("{detail_action} metrics"),
            ),
            Ok(Err(error)) => (
                EditInteractionResponse::new().content(error.clone()),
                Level::Warn,
                error,
            ),
            Err(_) => (
                EditInteractionResponse::new().content("Could not inspect metrics."),
                Level::Error,
                "task failed".into(),
            ),
        };
        let sent = command.edit_response(&ctx.http, response).await.is_ok();
        self.logger.log(
            ctx,
            guild,
            if sent { level } else { Level::Error },
            format!(
                "/metrics {name} for user {}: {detail}; response {}",
                command.user.id,
                if sent { "sent" } else { "failed" }
            ),
        );
    }

    async fn storage_command(&self, ctx: &Context, command: &CommandInteraction) {
        let Some(guild) = command.guild_id.filter(|_| command.member.is_some()) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Use /storage inside your server."),
                    ),
                )
                .await;
            return;
        };
        if command.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }
        let database = self.database.clone();
        let lock = self.storage_lock.clone();
        let root = self.storage_root.clone();
        let result =
            tokio::task::spawn_blocking(move || -> Result<storage::Usage, &'static str> {
                let db = database
                    .lock()
                    .map_err(|_| "Storage settings are unavailable.")?;
                let config = get_guild_config(&db, guild.get() as i64)
                    .map_err(|_| "Could not load settings.")?
                    .filter(|config| config.setup_completed)
                    .ok_or("Run /setup before checking storage.")?;
                let _guard = lock.lock().map_err(|_| "Storage is unavailable.")?;
                crate::retention::prune(
                    &root,
                    guild.get(),
                    config.retention,
                    crate::retention::now(),
                )
                .map_err(
                    |_| "Could not clean retained logs; ask the bot operator to check storage.",
                )?;
                storage::stats(&root, guild.get()).map_err(|_| {
                    "Could not inspect storage; ask the bot operator to check the guild folder."
                })
            })
            .await;
        let (response, level) = match result {
            Ok(Ok(usage)) => (
                EditInteractionResponse::new().embed(storage_embed(&usage)),
                Level::Info,
            ),
            Ok(Err(error)) => (EditInteractionResponse::new().content(error), Level::Warn),
            Err(_) => (
                EditInteractionResponse::new().content("Could not inspect guild storage."),
                Level::Error,
            ),
        };
        let sent = command.edit_response(&ctx.http, response).await.is_ok();
        self.logger.log(
            ctx,
            guild,
            if sent { level } else { Level::Error },
            format!(
                "/storage response for user {}: {}",
                command.user.id,
                if sent { "sent" } else { "failed" }
            ),
        );
    }

    async fn settings_command(&self, ctx: &Context, command: &CommandInteraction) {
        let Some(guild) = command.guild_id.filter(|_| command.member.is_some()) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Use /settings inside your server."),
                    ),
                )
                .await;
            return;
        };
        if command.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }

        let database = self.database.clone();
        let lock = self.storage_lock.clone();
        let root = self.storage_root.clone();
        let result = tokio::task::spawn_blocking(
            move || -> Result<(GuildConfig, Option<storage::Usage>, bool, bool, bool), &'static str> {
                let db = database.lock().map_err(|_| "Settings are unavailable.")?;
                let config = get_guild_config(&db, guild.get() as i64)
                    .map_err(|_| "Could not load settings.")?
                    .filter(|config| config.setup_completed)
                    .ok_or("Run /setup before checking settings.")?;
                let has_ai_config = get_ai_config(&db, guild.get() as i64)
                    .map_err(|_| "Could not load AI settings.")?
                    .is_some();
                let forwarding_enabled = metrics_forwarding_enabled(&db, guild.get() as i64)
                    .map_err(|_| "Could not load metrics settings.")?;
                let auto_delete = auto_delete_enabled(&db, guild.get() as i64)
                    .map_err(|_| "Could not load moderation settings.")?;
                let usage = lock.lock().ok().and_then(|_guard| {
                    let _ =
                        retention::prune(&root, guild.get(), config.retention, retention::now());
                    storage::stats(&root, guild.get()).ok()
                });
                Ok((config, usage, has_ai_config, forwarding_enabled, auto_delete))
            },
        )
        .await;

        let (response, level) = match result {
            Ok(Ok((config, usage, has_ai_config, forwarding_enabled, auto_delete))) => (
                EditInteractionResponse::new().embed(settings_embed(
                    &config,
                    usage.as_ref(),
                    has_ai_config,
                    forwarding_enabled,
                    auto_delete,
                )),
                Level::Info,
            ),
            Ok(Err(error)) => (EditInteractionResponse::new().content(error), Level::Warn),
            Err(_) => (
                EditInteractionResponse::new().content("Could not inspect settings."),
                Level::Error,
            ),
        };
        let sent = command.edit_response(&ctx.http, response).await.is_ok();
        self.logger.log(
            ctx,
            guild,
            if sent { level } else { Level::Error },
            format!(
                "/settings response for user {}: {}",
                command.user.id,
                if sent { "sent" } else { "failed" }
            ),
        );
    }

    async fn summary_command(&self, ctx: &Context, command: &CommandInteraction) {
        let Some(guild) = command.guild_id.filter(|_| command.member.is_some()) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Use /summary inside your server."),
                    ),
                )
                .await;
            return;
        };
        if command.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }

        let config = {
            let db = self.database.lock();
            db.ok()
                .and_then(|db| get_guild_config(&db, guild.get() as i64).ok())
                .flatten()
                .filter(|config| config.setup_completed)
        };
        let Some(config) = config else {
            let _ = command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new().content("Run /setup before using /summary."),
                )
                .await;
            return;
        };
        let manager = manager_authorized(&config, command.member.as_deref());

        let root = self.storage_root.clone();
        let lock = self.storage_lock.clone();
        let cache = self.rules.clone();
        let ai = self.ai.clone();
        let result = tokio::task::spawn_blocking(move || {
            let _guard = lock.lock().map_err(|_| crate::ai::SummaryError::Storage)?;
            let mut cache = cache.lock().map_err(|_| crate::ai::SummaryError::Storage)?;
            cache
                .get(&root, guild.get())
                .map_err(|_| crate::ai::SummaryError::Storage)
                .and_then(|book| {
                    if !book.public && !manager {
                        Err(crate::ai::SummaryError::RulesPrivate)
                    } else {
                        Ok(book)
                    }
                })
        })
        .await
        .map_err(|_| crate::ai::SummaryError::Storage)
        .and_then(|result| result)
        .and_then(|book| {
            let text = rules::to_ai_text(&book);
            crate::ai::rulebook_prompt(&text)?;
            Ok(text)
        });

        let ai_config = self.guild_ai_config(guild);
        let result = match result {
            Ok(rulebook) => ai.summarize_rulebook(&rulebook, ai_config.as_ref()).await,
            Err(error) => Err(error),
        };

        let (response, level, detail) = match result {
            Ok(summary) => {
                self.record_ai_tokens(guild, summary.usage);
                let mut response = EditInteractionResponse::new().content(
                    "AI-generated rule summary from the current curated rules JSON. Review before relying on it.",
                );
                response = response.new_attachment(CreateAttachment::bytes(
                    summary.value.clone().into_bytes(),
                    "rule-summary.md",
                ));
                (
                    response,
                    Level::Info,
                    format!("summary generated ({} bytes)", summary.value.len()),
                )
            }
            Err(error) => {
                let detail = ai_error_detail(&error);
                let (message, level) = summary_error(&error);
                (
                    EditInteractionResponse::new().content(message),
                    level,
                    detail,
                )
            }
        };
        let sent = command.edit_response(&ctx.http, response).await.is_ok();
        self.logger.log(
            ctx,
            guild,
            if sent { level } else { Level::Error },
            format!(
                "/summary response for user {}: {detail}; response {}",
                command.user.id,
                if sent { "sent" } else { "failed" }
            ),
        );
    }

    async fn rules_command(&self, ctx: &Context, command: &CommandInteraction) {
        let Some(guild) = command.guild_id.filter(|_| command.member.is_some()) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Use /rules inside your server."),
                    ),
                )
                .await;
            return;
        };
        if command.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }
        let action = match parse_rules_action(command) {
            Ok(action) => action,
            Err(error) => {
                let _ = command
                    .edit_response(&ctx.http, EditInteractionResponse::new().content(error))
                    .await;
                return;
            }
        };

        let database = self.database.clone();
        let lock = self.storage_lock.clone();
        let root = self.storage_root.clone();
        let member = command.member.clone();
        let cache = self.rules.clone();
        match action {
            RulesAction::Generate => {
                self.rules_generate_command(
                    ctx, command, guild, database, lock, root, member, cache,
                )
                .await;
                return;
            }
            RulesAction::FromChannel { channel, limit } => {
                self.rules_from_channel_command(
                    ctx, command, guild, channel, limit, database, lock, root, member, cache,
                )
                .await;
                return;
            }
            _ => {}
        }
        let result = tokio::task::spawn_blocking(
            move || -> Result<(EditInteractionResponse, Level, String), String> {
                let db = database.lock().map_err(|_| "Settings are unavailable.")?;
                let config = get_guild_config(&db, guild.get() as i64)
                    .map_err(|_| "Could not load settings.")?
                    .filter(|config| config.setup_completed)
                    .ok_or("Run /setup before managing rules.")?;
                let manager = manager_authorized(&config, member.as_deref());
                let _guard = lock.lock().map_err(|_| "Storage is unavailable.")?;
                let mut cache = cache.lock().map_err(|_| "Rules cache is unavailable.")?;
                let mut book = cache
                    .get(&root, guild.get())
                    .map_err(|_| "Could not read rules.")?;

                match action {
                    RulesAction::List => {
                        if !book.public && !manager {
                            return Err("Rules are private for this server.".into());
                        }
                        let markdown = rules::to_markdown(&book);
                        Ok((
                            EditInteractionResponse::new()
                                .content(format!(
                                    "Curated rules: {} rule(s). Visibility: {}.",
                                    book.rules.len(),
                                    if book.public { "public" } else { "private" }
                                ))
                                .new_attachment(CreateAttachment::bytes(
                                    markdown.into_bytes(),
                                    "rules.md",
                                )),
                            Level::Info,
                            "listed rules".into(),
                        ))
                    }
                    RulesAction::Export => {
                        if !manager {
                            return Err("Only bot managers can export the rules JSON.".into());
                        }
                        let data = serde_json::to_vec_pretty(&book)
                            .map_err(|_| "Could not export rules.")?;
                        Ok((
                            EditInteractionResponse::new()
                                .content("Curated rules JSON export.")
                                .new_attachment(CreateAttachment::bytes(data, "rules.json")),
                            Level::Info,
                            "exported rules".into(),
                        ))
                    }
                    RulesAction::Generate | RulesAction::FromChannel { .. } => {
                        unreachable!("handled before blocking rules command")
                    }
                    RulesAction::Visibility(public) => {
                        if !manager {
                            return Err("Only bot managers can change rule visibility.".into());
                        }
                        book.public = public;
                        cache
                            .save(&root, guild.get(), &book)
                            .map_err(|_| "Could not save rule visibility.")?;
                        Ok((
                            EditInteractionResponse::new().content(format!(
                                "Rule visibility is now {}.",
                                if public { "public" } else { "private" }
                            )),
                            Level::Info,
                            format!("set rules visibility to {public}"),
                        ))
                    }
                    RulesAction::Add { id, severity, text } => {
                        if !manager {
                            return Err("Only bot managers can add rules.".into());
                        }
                        let created = rules::upsert(&mut book, &id, &severity, Some(&text))
                            .map_err(|error| error.to_string())?;
                        cache
                            .save(&root, guild.get(), &book)
                            .map_err(|_| "Could not save rules.")?;
                        Ok((
                            EditInteractionResponse::new().content(format!(
                                "{} rule `{id}` with severity `{severity}`.",
                                if created { "Added" } else { "Replaced" }
                            )),
                            Level::Info,
                            format!("upserted rule {id} severity {severity}"),
                        ))
                    }
                    RulesAction::Update { id, severity, text } => {
                        if !manager {
                            return Err("Only bot managers can update rules.".into());
                        }
                        let Some(rule) = book.rules.iter().find(|rule| rule.id == id).cloned()
                        else {
                            return Err("Rule not found.".into());
                        };
                        let severity = severity.unwrap_or(rule.severity);
                        rules::upsert(&mut book, &id, &severity, text.as_deref())
                            .map_err(|error| error.to_string())?;
                        cache
                            .save(&root, guild.get(), &book)
                            .map_err(|_| "Could not save rules.")?;
                        Ok((
                            EditInteractionResponse::new().content(format!(
                                "Updated rule `{id}` with severity `{severity}`."
                            )),
                            Level::Info,
                            format!("updated rule {id} severity {severity}"),
                        ))
                    }
                    RulesAction::Remove(id) => {
                        if !manager {
                            return Err("Only bot managers can remove rules.".into());
                        }
                        if !rules::remove(&mut book, &id).map_err(|error| error.to_string())? {
                            return Err("Rule not found.".into());
                        }
                        cache
                            .save(&root, guild.get(), &book)
                            .map_err(|_| "Could not save rules.")?;
                        Ok((
                            EditInteractionResponse::new().content(format!("Removed rule `{id}`.")),
                            Level::Info,
                            format!("removed rule {id}"),
                        ))
                    }
                }
            },
        )
        .await;

        let (response, level, detail) = match result {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => (
                EditInteractionResponse::new().content(error.clone()),
                Level::Warn,
                error,
            ),
            Err(_) => (
                EditInteractionResponse::new().content("Could not manage rules."),
                Level::Error,
                "task failed".into(),
            ),
        };
        let sent = command.edit_response(&ctx.http, response).await.is_ok();
        self.logger.log(
            ctx,
            guild,
            if sent { level } else { Level::Error },
            format!(
                "/rules response for user {}: {detail}; response {}",
                command.user.id,
                if sent { "sent" } else { "failed" }
            ),
        );
    }

    async fn rules_generate_command(
        &self,
        ctx: &Context,
        command: &CommandInteraction,
        guild: GuildId,
        database: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
        lock: std::sync::Arc<std::sync::Mutex<()>>,
        root: std::path::PathBuf,
        member: Option<Box<Member>>,
        cache: std::sync::Arc<std::sync::Mutex<rules::Cache>>,
    ) {
        let loaded = tokio::task::spawn_blocking({
            let root = root.clone();
            let lock = lock.clone();
            let cache = cache.clone();
            move || -> Result<(Vec<(String, Vec<u8>)>, bool), String> {
                let db = database.lock().map_err(|_| "Settings are unavailable.")?;
                let config = get_guild_config(&db, guild.get() as i64)
                    .map_err(|_| "Could not load settings.")?
                    .filter(|config| config.setup_completed)
                    .ok_or("Run /setup before generating rules.")?;
                if !manager_authorized(&config, member.as_deref()) {
                    return Err("Only bot managers can generate rules.".into());
                }
                let _guard = lock.lock().map_err(|_| "Storage is unavailable.")?;
                let mut cache = cache.lock().map_err(|_| "Rules cache is unavailable.")?;
                let book = cache
                    .get(&root, guild.get())
                    .map_err(|_| "Could not read existing rules.")?;
                let files = storage::read_uploads(&root, guild.get())
                    .map_err(|_| "Could not read uploaded files.")?;
                Ok((files, book.public))
            }
        })
        .await;

        let (files, public) = match loaded {
            Ok(Ok(data)) => data,
            Ok(Err(error)) => {
                self.finish_rules_response(ctx, command, guild, Level::Warn, error)
                    .await;
                return;
            }
            Err(_) => {
                self.finish_rules_response(
                    ctx,
                    command,
                    guild,
                    Level::Error,
                    "Could not load files for rule generation.".into(),
                )
                .await;
                return;
            }
        };

        let ai_config = self.guild_ai_config(guild);
        let generated = self
            .ai
            .generate_rulebook(&files, public, ai_config.as_ref())
            .await;
        let book = match generated {
            Ok(book) => {
                self.record_ai_tokens(guild, book.usage);
                book.value
            }
            Err(error) => {
                self.record_metric(guild, metrics::Counter::AiErrors, 1);
                let detail = ai_error_detail(&error);
                let (message, level) = summary_error(&error);
                self.logger.log(
                    ctx,
                    guild,
                    level,
                    format!(
                        "/rules generate AI failure for user {}: {detail}",
                        command.user.id
                    ),
                );
                self.finish_rules_response(ctx, command, guild, level, message.to_owned())
                    .await;
                return;
            }
        };

        let count = book.rules.len();
        let saved = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let _guard = lock.lock().map_err(|_| "Storage is unavailable.")?;
            let mut cache = cache.lock().map_err(|_| "Rules cache is unavailable.")?;
            cache
                .save(&root, guild.get(), &book)
                .map_err(|_| "Could not save generated rules.".to_owned())
        })
        .await;

        match saved {
            Ok(Ok(())) => {
                self.finish_rules_response(ctx, command, guild, Level::Info, {
                    self.record_metric(guild, metrics::Counter::RulesImported, count as u64);
                    format!("Generated {count} curated rule(s) from uploaded files.")
                })
                .await;
            }
            Ok(Err(error)) => {
                self.finish_rules_response(ctx, command, guild, Level::Error, error)
                    .await;
            }
            Err(_) => {
                self.finish_rules_response(
                    ctx,
                    command,
                    guild,
                    Level::Error,
                    "Could not save generated rules.".into(),
                )
                .await;
            }
        }
    }

    async fn rules_from_channel_command(
        &self,
        ctx: &Context,
        command: &CommandInteraction,
        guild: GuildId,
        channel: ChannelId,
        limit: u16,
        database: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
        lock: std::sync::Arc<std::sync::Mutex<()>>,
        root: std::path::PathBuf,
        member: Option<Box<Member>>,
        cache: std::sync::Arc<std::sync::Mutex<rules::Cache>>,
    ) {
        let valid_channel = ctx.cache.guild(guild).is_some_and(|cached| {
            cached.channels.get(&channel).is_some_and(|channel| {
                channel.guild_id == guild && channel.kind == ChannelType::Text
            })
        });
        if !valid_channel {
            self.finish_rules_response(
                ctx,
                command,
                guild,
                Level::Warn,
                "Choose a text channel from this server that the bot can see.".into(),
            )
            .await;
            return;
        }

        let loaded = tokio::task::spawn_blocking({
            let root = root.clone();
            let lock = lock.clone();
            let cache = cache.clone();
            move || -> Result<bool, String> {
                let db = database.lock().map_err(|_| "Settings are unavailable.")?;
                let config = get_guild_config(&db, guild.get() as i64)
                    .map_err(|_| "Could not load settings.")?
                    .filter(|config| config.setup_completed)
                    .ok_or("Run /setup before generating rules.")?;
                if !manager_authorized(&config, member.as_deref()) {
                    return Err("Only bot managers can generate rules.".into());
                }
                let _guard = lock.lock().map_err(|_| "Storage is unavailable.")?;
                let mut cache = cache.lock().map_err(|_| "Rules cache is unavailable.")?;
                let book = cache
                    .get(&root, guild.get())
                    .map_err(|_| "Could not read existing rules.")?;
                Ok(book.public)
            }
        })
        .await;
        let public = match loaded {
            Ok(Ok(public)) => public,
            Ok(Err(error)) => {
                self.finish_rules_response(ctx, command, guild, Level::Warn, error)
                    .await;
                return;
            }
            Err(_) => {
                self.finish_rules_response(
                    ctx,
                    command,
                    guild,
                    Level::Error,
                    "Could not prepare channel rule generation.".into(),
                )
                .await;
                return;
            }
        };

        let mut raw_messages = Vec::new();
        let mut before = None;
        while raw_messages.len() < usize::from(limit) {
            let remaining = usize::from(limit) - raw_messages.len();
            let page_limit = remaining.min(100) as u8;
            let mut builder = GetMessages::new().limit(page_limit);
            if let Some(before_id) = before {
                builder = builder.before(before_id);
            }
            let page = match channel.messages(&ctx.http, builder).await {
                Ok(page) => page,
                Err(_) => {
                    self.finish_rules_response(
                        ctx,
                        command,
                        guild,
                        Level::Error,
                        "Could not read messages from that channel. Check View Channel and Read Message History permissions.".into(),
                    )
                    .await;
                    return;
                }
            };
            if page.is_empty() {
                break;
            }
            before = page.last().map(|message| message.id);
            let received = page.len();
            raw_messages.extend(page);
            if received < usize::from(page_limit) {
                break;
            }
        }
        raw_messages.reverse();
        let scanned = raw_messages.len();
        let messages = raw_messages
            .into_iter()
            .filter(|message| !message.author.bot)
            .filter_map(|message| {
                let content = message.content.trim().to_owned();
                (!content.is_empty()).then(|| (message.author.id.to_string(), content))
            })
            .collect::<Vec<_>>();
        if scanned == 0 {
            self.finish_rules_response(
                ctx,
                command,
                guild,
                Level::Warn,
                "No readable messages found in that channel.".into(),
            )
            .await;
            return;
        }
        if messages.is_empty() {
            self.finish_rules_response(
                ctx,
                command,
                guild,
                Level::Warn,
                "No readable non-bot text messages found in that channel.".into(),
            )
            .await;
            return;
        }

        let ai_config = self.guild_ai_config(guild);
        let generated = self
            .ai
            .generate_rulebook_from_messages(&messages, public, ai_config.as_ref())
            .await;
        let book = match generated {
            Ok(book) => {
                self.record_ai_tokens(guild, book.usage);
                book.value
            }
            Err(error) => {
                self.record_metric(guild, metrics::Counter::AiErrors, 1);
                let detail = ai_error_detail(&error);
                let (message, level) = summary_error(&error);
                self.logger.log(
                    ctx,
                    guild,
                    level,
                    format!(
                        "/rules from-channel AI failure for user {} in channel {}: {detail}",
                        command.user.id,
                        channel.get()
                    ),
                );
                self.finish_rules_response(ctx, command, guild, level, message.to_owned())
                    .await;
                return;
            }
        };
        let count = book.rules.len();
        let saved = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let _guard = lock.lock().map_err(|_| "Storage is unavailable.")?;
            let mut cache = cache.lock().map_err(|_| "Rules cache is unavailable.")?;
            cache
                .save(&root, guild.get(), &book)
                .map_err(|_| "Could not save generated rules.".to_owned())
        })
        .await;
        match saved {
            Ok(Ok(())) => {
                self.record_metric(guild, metrics::Counter::RulesImported, count as u64);
                self.finish_rules_response(
                    ctx,
                    command,
                    guild,
                    Level::Info,
                    format!(
                        "Generated {count} curated rule(s) from {scanned} recent message(s) in <#{}>.",
                        channel.get()
                    ),
                )
                .await;
            }
            Ok(Err(error)) => {
                self.finish_rules_response(ctx, command, guild, Level::Error, error)
                    .await;
            }
            Err(_) => {
                self.finish_rules_response(
                    ctx,
                    command,
                    guild,
                    Level::Error,
                    "Could not save generated rules.".into(),
                )
                .await;
            }
        }
    }

    async fn finish_rules_response(
        &self,
        ctx: &Context,
        command: &CommandInteraction,
        guild: GuildId,
        level: Level,
        message: String,
    ) {
        let sent = command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content(message.clone()),
            )
            .await
            .is_ok();
        self.logger.log(
            ctx,
            guild,
            if sent { level } else { Level::Error },
            format!(
                "/rules generate for user {}: {message}; response {}",
                command.user.id,
                if sent { "sent" } else { "failed" }
            ),
        );
    }

    async fn logs_command(&self, ctx: &Context, command: &CommandInteraction) {
        let Some(guild) = command.guild_id.filter(|_| command.member.is_some()) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Use /logs inside your server."),
                    ),
                )
                .await;
            return;
        };
        if command.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }

        let confirm = match command.data.options().first() {
            Some(ResolvedOption {
                name: "clear",
                value: ResolvedValue::SubCommand(options),
                ..
            }) => options.iter().find_map(|option| match option.value {
                ResolvedValue::Boolean(value) => Some(value),
                _ => None,
            }),
            _ => None,
        };

        let Some(confirm) = confirm else {
            let _ = command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new().content("Use `/logs clear confirm:false` to preview or `/logs clear confirm:true` to delete retained local logs."),
                )
                .await;
            return;
        };

        let database = self.database.clone();
        let lock = self.storage_lock.clone();
        let root = self.storage_root.clone();
        let member = command.member.clone();
        let result = tokio::task::spawn_blocking(
            move || -> Result<(storage::Usage, Option<retention::ClearReport>), &'static str> {
                let db = database.lock().map_err(|_| "Settings are unavailable.")?;
                let config = get_guild_config(&db, guild.get() as i64)
                    .map_err(|_| "Could not load settings.")?
                    .filter(|config| config.setup_completed)
                    .ok_or("Run /setup before managing logs.")?;
                if !manager_authorized(&config, member.as_deref()) {
                    return Err("You need a configured bot manager role or Manage Server to delete retained logs.");
                }
                let _guard = lock.lock().map_err(|_| "Storage is unavailable.")?;
                if !confirm {
                    return storage::stats(&root, guild.get())
                        .map(|usage| (usage, None))
                        .map_err(|_| "Could not inspect retained logs.");
                }
                let report = retention::clear(&root, guild.get())
                    .map_err(|_| "Could not delete retained logs; ask the bot operator to check the guild folder.")?;
                let usage = storage::stats(&root, guild.get())
                    .map_err(|_| "Logs were deleted, but storage could not be inspected.")?;
                Ok((usage, Some(report)))
            },
        )
        .await;

        let (response, level, detail) = match result {
            Ok(Ok((usage, None))) => (
                EditInteractionResponse::new().embed(
                    CreateEmbed::new()
                        .title("Retained logs")
                        .colour(0x5865F2)
                        .description(format!(
                            "Retained logs currently use {}.\nRun `/logs clear confirm:true` to delete retained local log archives. Discord messages already posted in the log channel are not deleted.",
                            bytes(usage.logs)
                        ))
                        .field("Available after no-op", bytes(usage.available()), true)
                        .field("Total used", bytes(usage.total), true),
                ),
                Level::Info,
                "previewed retained logs".to_owned(),
            ),
            Ok(Ok((usage, Some(report)))) => (
                EditInteractionResponse::new().embed(
                    CreateEmbed::new()
                        .title("Retained logs deleted")
                        .colour(0x57F287)
                        .description(format!(
                            "Deleted {} retained log file(s), freeing {}. Discord messages already posted in the log channel were not deleted. If retention still includes bot actions, this cleanup may leave a new small audit entry.",
                            report.files,
                            bytes(report.bytes)
                        ))
                        .field("Available now", bytes(usage.available()), true)
                        .field("Total used now", bytes(usage.total), true),
                ),
                Level::Info,
                format!(
                    "deleted {} retained log file(s), freeing {} bytes",
                    report.files, report.bytes
                ),
            ),
            Ok(Err(error)) => (
                EditInteractionResponse::new().content(error),
                Level::Warn,
                error.to_owned(),
            ),
            Err(_) => (
                EditInteractionResponse::new().content("Could not manage retained logs."),
                Level::Error,
                "task failed".to_owned(),
            ),
        };
        let sent = command.edit_response(&ctx.http, response).await.is_ok();
        self.logger.log(
            ctx,
            guild,
            if sent { level } else { Level::Error },
            format!(
                "/logs clear by user {}: {detail}; response {}",
                command.user.id,
                if sent { "sent" } else { "failed" }
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storage_report_shows_exact_available_bytes_and_breakdown() {
        let usage = storage::Usage {
            uploads: 1_000_000,
            logs: 2_000_000,
            other: 100,
            total: 3_000_100,
        };
        let value = serde_json::to_value(storage_embed(&usage)).unwrap();
        let fields = value["fields"].as_array().unwrap();
        assert_eq!(fields[0]["value"], "7.00 Mo (6999900 bytes)");
        assert!(
            fields
                .iter()
                .any(|f| f["name"] == "Retained logs" && f["value"] == "2.00 Mo (2000000 bytes)")
        );
    }
    #[test]
    fn contact_is_explicit_about_missing_configuration() {
        assert!(operator_notice(None, None).contains("not configured"));
        assert!(operator_notice(Some("Operator"), Some(" ")).contains("not configured"));
        let notice = operator_notice(Some("Operator"), Some("privacy@example.test"));
        assert!(notice.contains("privacy@example.test"));
        assert!(notice.contains("deletion"));
    }
    #[test]
    fn public_commands_do_not_require_discord_permissions_or_filename_arguments() {
        for command in commands() {
            let value = serde_json::to_value(command).unwrap();
            if value["name"] == "disable" || value["name"] == "leave" {
                assert_eq!(value["default_member_permissions"], "32");
            } else {
                assert!(value["default_member_permissions"].is_null());
            }
            let has_filename = value["options"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|option| option["options"].as_array().into_iter().flatten())
                .any(|option| option["name"] == "filename");
            assert!(!has_filename);
        }
    }

    #[test]
    fn disable_command_is_registered_for_manage_server() {
        let command = commands()
            .into_iter()
            .find(|command| serde_json::to_value(command).unwrap()["name"] == "disable")
            .unwrap();
        let value = serde_json::to_value(command).unwrap();
        assert_eq!(
            value["description"],
            "Disable Clause for this server until /setup is run again"
        );
        assert_eq!(value["default_member_permissions"], "32");
        assert_eq!(value["dm_permission"], false);
    }

    #[test]
    fn leave_command_requires_explicit_confirmation_and_delete_choice() {
        let command = commands()
            .into_iter()
            .find(|command| serde_json::to_value(command).unwrap()["name"] == "leave")
            .unwrap();
        let value = serde_json::to_value(command).unwrap();
        assert_eq!(value["default_member_permissions"], "32");
        let options = value["options"].as_array().unwrap();
        assert!(
            options
                .iter()
                .any(|option| option["name"] == "confirm" && option["required"] == true)
        );
        assert!(
            options
                .iter()
                .any(|option| option["name"] == "delete_data" && option["required"] == true)
        );
    }

    #[test]
    fn metrics_command_registers_show_and_forwarding_toggle() {
        let command = commands()
            .into_iter()
            .find(|command| serde_json::to_value(command).unwrap()["name"] == "metrics")
            .unwrap();
        let value = serde_json::to_value(command).unwrap();
        assert_eq!(value["default_member_permissions"], serde_json::Value::Null);
        let options = value["options"].as_array().unwrap();
        assert!(options.iter().any(|option| option["name"] == "show"));
        let forwarding = options
            .iter()
            .find(|option| option["name"] == "forwarding")
            .unwrap();
        assert!(
            forwarding["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["name"] == "enabled" && option["required"] == true)
        );
    }

    #[test]
    fn logs_command_requires_explicit_clear_confirmation() {
        let command = commands()
            .into_iter()
            .find(|command| serde_json::to_value(command).unwrap()["name"] == "logs")
            .unwrap();
        let value = serde_json::to_value(command).unwrap();
        assert_eq!(value["options"][0]["name"], "clear");
        assert_eq!(value["options"][0]["options"][0]["name"], "confirm");
        assert_eq!(value["options"][0]["options"][0]["required"], true);
    }

    #[test]
    fn ai_and_channel_rule_commands_are_registered() {
        let rule_command = commands()
            .into_iter()
            .find(|command| serde_json::to_value(command).unwrap()["name"] == "rules")
            .unwrap();
        let rule_value = serde_json::to_value(rule_command).unwrap();
        let from_channel = rule_value["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["name"] == "from-channel")
            .unwrap();
        let from_channel_options = from_channel["options"].as_array().unwrap();
        assert!(
            from_channel_options
                .iter()
                .any(|option| { option["name"] == "channel" && option["required"] == true })
        );
        assert!(
            from_channel_options
                .iter()
                .any(|option| { option["name"] == "limit" && option["max_value"] == 1000 })
        );

        let ai_command = commands()
            .into_iter()
            .find(|command| serde_json::to_value(command).unwrap()["name"] == "ai")
            .unwrap();
        let ai_value = serde_json::to_value(ai_command).unwrap();
        assert!(
            ai_value["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["name"] == "test")
        );
        let auto_delete = ai_value["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["name"] == "auto-delete")
            .unwrap();
        assert!(
            auto_delete["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["name"] == "enabled" && option["required"] == true)
        );
        let set = ai_value["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["name"] == "set")
            .unwrap();
        let set_options = set["options"].as_array().unwrap();
        assert!(
            set_options
                .iter()
                .any(|option| { option["name"] == "endpoint" && option["required"] == true })
        );
        assert!(
            set_options
                .iter()
                .any(|option| option["name"] == "model" && option["required"] == true)
        );
        assert!(
            set_options
                .iter()
                .any(|option| { option["name"] == "api_key" && option["required"] == true })
        );
    }

    #[test]
    fn disable_guild_settings_clears_setup_and_private_provider() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE guild_configs (
                guild_id INTEGER PRIMARY KEY,
                setup_completed INTEGER NOT NULL DEFAULT 0,
                log_channel_id INTEGER,
                rule_source_channel_id INTEGER,
                log_level TEXT NOT NULL DEFAULT 'info',
                retention TEXT NOT NULL DEFAULT 'none');
            CREATE TABLE guild_manager_roles (
                guild_id INTEGER NOT NULL, role_id INTEGER NOT NULL,
                PRIMARY KEY (guild_id, role_id));
            CREATE TABLE guild_bot_channels (
                guild_id INTEGER NOT NULL, channel_id INTEGER NOT NULL,
                PRIMARY KEY (guild_id, channel_id));
            CREATE TABLE guild_ai_configs (
                guild_id INTEGER PRIMARY KEY,
                endpoint TEXT NOT NULL,
                api_key TEXT NOT NULL,
                model TEXT NOT NULL);
            CREATE TABLE guild_metrics_settings (
                guild_id INTEGER PRIMARY KEY,
                forwarding_enabled INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE guild_moderation_settings (
                guild_id INTEGER PRIMARY KEY,
                auto_delete_enabled INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE guild_metrics_daily (
                guild_id INTEGER NOT NULL, day INTEGER NOT NULL,
                messages_seen INTEGER NOT NULL DEFAULT 0, ai_reviews INTEGER NOT NULL DEFAULT 0,
                ai_compliant INTEGER NOT NULL DEFAULT 0, ai_gray_area INTEGER NOT NULL DEFAULT 0,
                ai_violations INTEGER NOT NULL DEFAULT 0, ai_errors INTEGER NOT NULL DEFAULT 0,
                ai_deleted_messages INTEGER NOT NULL DEFAULT 0, ai_delete_failures INTEGER NOT NULL DEFAULT 0,
                staff_review_pings INTEGER NOT NULL DEFAULT 0, bot_replies INTEGER NOT NULL DEFAULT 0,
                rule_source_messages INTEGER NOT NULL DEFAULT 0, rules_imported INTEGER NOT NULL DEFAULT 0,
                commands_used INTEGER NOT NULL DEFAULT 0, uploads_added INTEGER NOT NULL DEFAULT 0,
                uploads_removed INTEGER NOT NULL DEFAULT 0, input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0, total_tokens INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (guild_id, day));
            INSERT INTO guild_configs VALUES (1, 1, 10, 11, 'debug', 'all:7');
            INSERT INTO guild_manager_roles VALUES (1, 20);
            INSERT INTO guild_bot_channels VALUES (1, 30);
            INSERT INTO guild_ai_configs VALUES (1, 'https://example.test/v1/chat/completions', 'secret', 'model');
            INSERT INTO guild_metrics_settings VALUES (1, 1);
            INSERT INTO guild_moderation_settings VALUES (1, 1);",
        )
        .unwrap();

        disable_guild_settings(&db, 1).unwrap();
        let config = get_guild_config(&db, 1).unwrap().unwrap();
        assert!(!config.setup_completed);
        assert_eq!(config.log_channel_id, None);
        assert_eq!(config.rule_source_channel_id, None);
        assert_eq!(config.log_level, Level::Off);
        assert_eq!(config.retention, retention::Policy::None);
        assert!(config.channel_ids.is_empty());
        assert!(config.admin_role_ids.is_empty());
        assert!(get_ai_config(&db, 1).unwrap().is_none());
        assert!(!metrics_forwarding_enabled(&db, 1).unwrap());
    }

    #[test]
    fn delete_guild_settings_removes_all_database_state_for_one_guild() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE guild_configs (
                guild_id INTEGER PRIMARY KEY,
                setup_completed INTEGER NOT NULL DEFAULT 0,
                log_channel_id INTEGER,
                rule_source_channel_id INTEGER,
                log_level TEXT NOT NULL DEFAULT 'info',
                retention TEXT NOT NULL DEFAULT 'none');
            CREATE TABLE guild_manager_roles (
                guild_id INTEGER NOT NULL, role_id INTEGER NOT NULL,
                PRIMARY KEY (guild_id, role_id));
            CREATE TABLE guild_bot_channels (
                guild_id INTEGER NOT NULL, channel_id INTEGER NOT NULL,
                PRIMARY KEY (guild_id, channel_id));
            CREATE TABLE guild_ai_configs (
                guild_id INTEGER PRIMARY KEY,
                endpoint TEXT NOT NULL,
                api_key TEXT NOT NULL,
                model TEXT NOT NULL);
            CREATE TABLE guild_metrics_settings (
                guild_id INTEGER PRIMARY KEY,
                forwarding_enabled INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE guild_moderation_settings (
                guild_id INTEGER PRIMARY KEY,
                auto_delete_enabled INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE guild_metrics_daily (
                guild_id INTEGER NOT NULL, day INTEGER NOT NULL,
                messages_seen INTEGER NOT NULL DEFAULT 0, ai_reviews INTEGER NOT NULL DEFAULT 0,
                ai_compliant INTEGER NOT NULL DEFAULT 0, ai_gray_area INTEGER NOT NULL DEFAULT 0,
                ai_violations INTEGER NOT NULL DEFAULT 0, ai_errors INTEGER NOT NULL DEFAULT 0,
                ai_deleted_messages INTEGER NOT NULL DEFAULT 0, ai_delete_failures INTEGER NOT NULL DEFAULT 0,
                staff_review_pings INTEGER NOT NULL DEFAULT 0, bot_replies INTEGER NOT NULL DEFAULT 0,
                rule_source_messages INTEGER NOT NULL DEFAULT 0, rules_imported INTEGER NOT NULL DEFAULT 0,
                commands_used INTEGER NOT NULL DEFAULT 0, uploads_added INTEGER NOT NULL DEFAULT 0,
                uploads_removed INTEGER NOT NULL DEFAULT 0, input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0, total_tokens INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (guild_id, day));
            INSERT INTO guild_configs VALUES (1, 1, 10, 11, 'debug', 'all:7');
            INSERT INTO guild_configs VALUES (2, 1, 20, 21, 'info', 'none');
            INSERT INTO guild_manager_roles VALUES (1, 20);
            INSERT INTO guild_manager_roles VALUES (2, 40);
            INSERT INTO guild_bot_channels VALUES (1, 30);
            INSERT INTO guild_bot_channels VALUES (2, 50);
            INSERT INTO guild_ai_configs VALUES (1, 'https://example.test/v1/chat/completions', 'secret', 'model');
            INSERT INTO guild_metrics_settings VALUES (1, 1);
            INSERT INTO guild_moderation_settings VALUES (1, 1);
            INSERT INTO guild_metrics_settings VALUES (2, 1);
            INSERT INTO guild_moderation_settings VALUES (2, 1);",
        )
        .unwrap();

        delete_guild_settings(&db, 1).unwrap();
        assert!(get_guild_config(&db, 1).unwrap().is_none());
        assert!(get_ai_config(&db, 1).unwrap().is_none());
        assert!(!metrics_forwarding_enabled(&db, 1).unwrap());
        assert!(metrics_forwarding_enabled(&db, 2).unwrap());
        assert_eq!(
            get_guild_config(&db, 2).unwrap().unwrap().log_channel_id,
            Some(20)
        );
    }

    #[test]
    fn metrics_report_uses_aggregate_counts_without_content() {
        let snapshot = MetricsSnapshot {
            config: GuildConfig {
                guild_id: 1,
                setup_completed: true,
                log_channel_id: Some(10),
                rule_source_channel_id: None,
                log_level: Level::Info,
                retention: retention::Policy::Flagged(7),
                channel_ids: vec![20, 21],
                admin_role_ids: vec![30, 31],
            },
            usage: storage::Usage {
                uploads: 10,
                logs: 20,
                other: 30,
                total: 60,
            },
            upload_files: 2,
            rule_count: 4,
            rules_public: true,
            has_guild_ai_config: false,
            forwarding_enabled: false,
            auto_delete_enabled: false,
            today: metrics::Totals {
                messages_seen: 3,
                ai_reviews: 2,
                ai_violations: 1,
                total_tokens: 42,
                ..Default::default()
            },
            seven_days: metrics::Totals {
                messages_seen: 5,
                ai_reviews: 3,
                ai_gray_area: 1,
                ..Default::default()
            },
            all_time: metrics::Totals {
                messages_seen: 8,
                ai_deleted_messages: 1,
                uploads_added: 2,
                uploads_removed: 1,
                ..Default::default()
            },
        };
        let value = serde_json::to_value(metrics_embed(&snapshot)).unwrap();
        let fields = value["fields"].as_array().unwrap();
        assert!(
            fields
                .iter()
                .any(|field| { field["name"] == "Uploaded files" && field["value"] == "2" })
        );
        assert!(
            fields
                .iter()
                .any(|field| { field["name"] == "Curated rules" && field["value"] == "4" })
        );
        assert!(fields.iter().any(|field| {
            field["name"] == "Metrics forwarding allowed"
                && field["value"].as_str().unwrap().contains("disabled")
        }));
        let serialized = value.to_string();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("rules.pdf"));
    }

    #[test]
    fn settings_report_discloses_user_visible_configuration() {
        let config = GuildConfig {
            guild_id: 1,
            setup_completed: true,
            log_channel_id: Some(10),
            rule_source_channel_id: Some(11),
            log_level: Level::Debug,
            retention: retention::Policy::All(7),
            channel_ids: vec![20, 21],
            admin_role_ids: vec![30],
        };
        let usage = storage::Usage {
            uploads: 5,
            logs: 7,
            other: 2,
            total: 14,
        };
        let value =
            serde_json::to_value(settings_embed(&config, Some(&usage), true, true, true)).unwrap();
        let fields = value["fields"].as_array().unwrap();
        assert!(fields.iter().any(|field| field["name"] == "Local retention"
            && field["value"].as_str().unwrap().contains("All messages")));
        assert!(
            fields
                .iter()
                .any(|field| field["name"] == "Bot manager roles"
                    && field["value"].as_str().unwrap().contains("<@&30>"))
        );
        assert!(fields.iter().any(|field| {
            field["name"] == "Message visibility"
                && field["value"]
                    .as_str()
                    .unwrap()
                    .contains("retained locally")
        }));
        assert!(fields.iter().any(|field| {
            field["name"] == "Rule source channel"
                && field["value"].as_str().unwrap().contains("<#11>")
        }));
        assert!(fields.iter().any(|field| {
            field["name"] == "AI rule review" && field["value"].as_str().unwrap().contains("AI")
        }));
        assert!(fields.iter().any(|field| {
            field["name"] == "Metrics forwarding allowed"
                && field["value"].as_str().unwrap().contains("enabled")
        }));
    }
}
