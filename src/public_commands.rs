use crate::{
    GuildAiConfig, GuildConfig, Handler, delete_ai_config, get_ai_config, get_guild_config,
    logging::Level, retention, rules, save_ai_config, storage,
};
use serenity::all::*;

const CHANNEL_RULE_IMPORT_LIMIT: i64 = 1_000;

pub(crate) fn commands() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("storage")
            .description("Show this server's used and available storage")
            .dm_permission(false),
        CreateCommand::new("disable")
            .description("Disable Clause for this server until /setup is run again")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .dm_permission(false),
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
) -> CreateEmbed {
    let log_channel = config
        .log_channel_id
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
        .field(
            "AI rule review",
            ai_review_visibility(has_guild_ai_config),
            false,
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

fn summary_error(error: crate::ai::SummaryError) -> (&'static str, Level) {
    match error {
        crate::ai::SummaryError::MissingConfig => (
            "AI is not configured. Set API_KEY, AI_ENDPOINT_URL, and AI_MODEL before using /summary or /rules generate.",
            Level::Warn,
        ),
        crate::ai::SummaryError::RulesPrivate => (
            "Rules are private for this server. Ask a bot manager to make them public or request the summary.",
            Level::Warn,
        ),
        crate::ai::SummaryError::NoRuleFiles => (
            "No rules were found. A bot manager can add rules with /rules add or upload rule files and run /rules generate.",
            Level::Warn,
        ),
        crate::ai::SummaryError::FilesTooLarge => (
            "The rules input is too large for one AI request. Split or shorten the uploaded files or curated rules first.",
            Level::Warn,
        ),
        crate::ai::SummaryError::Storage => (
            "Could not read curated rules. Ask the bot operator to check guild storage.",
            Level::Error,
        ),
        crate::ai::SummaryError::ProviderRequest(_) => (
            "The AI request failed. Check API_KEY, AI_ENDPOINT_URL, AI_MODEL, and provider availability.",
            Level::Error,
        ),
        crate::ai::SummaryError::BadResponse | crate::ai::SummaryError::ProviderResponse(_) => (
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
            "INSERT INTO guild_configs (guild_id, setup_completed, log_channel_id, log_level, retention)
            VALUES (?1, 0, NULL, 'off', 'none')
            ON CONFLICT(guild_id) DO UPDATE SET
            setup_completed = 0, log_channel_id = NULL, log_level = 'off', retention = 'none'",
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
        Ok(())
    })();
    if result.is_err() {
        db.execute_batch("ROLLBACK TO disable_guild")?;
    }
    db.execute_batch("RELEASE disable_guild")?;
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
                    EditInteractionResponse::new().content("Choose show, set, or clear."),
                )
                .await;
            return;
        };

        let database = self.database.clone();
        let member = command.member.clone();
        let action = (*name).to_owned();
        let endpoint = string_arg(options, "endpoint");
        let model = string_arg(options, "model");
        let api_key = string_arg(options, "api_key");
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
                _ => Err("Choose show, set, or clear.".into()),
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
        if command.data.name == "storage" {
            self.storage_command(ctx, command).await;
            return;
        }
        if command.data.name == "disable" {
            self.disable_command(ctx, command).await;
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
            move || -> Result<(GuildConfig, Option<storage::Usage>, bool), &'static str> {
                let db = database.lock().map_err(|_| "Settings are unavailable.")?;
                let config = get_guild_config(&db, guild.get() as i64)
                    .map_err(|_| "Could not load settings.")?
                    .filter(|config| config.setup_completed)
                    .ok_or("Run /setup before checking settings.")?;
                let has_ai_config = get_ai_config(&db, guild.get() as i64)
                    .map_err(|_| "Could not load AI settings.")?
                    .is_some();
                let usage = lock.lock().ok().and_then(|_guard| {
                    let _ =
                        retention::prune(&root, guild.get(), config.retention, retention::now());
                    storage::stats(&root, guild.get()).ok()
                });
                Ok((config, usage, has_ai_config))
            },
        )
        .await;

        let (response, level) = match result {
            Ok(Ok((config, usage, has_ai_config))) => (
                EditInteractionResponse::new().embed(settings_embed(
                    &config,
                    usage.as_ref(),
                    has_ai_config,
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
                let mut response = EditInteractionResponse::new().content(
                    "AI-generated rule summary from the current curated rules JSON. Review before relying on it.",
                );
                response = response.new_attachment(CreateAttachment::bytes(
                    summary.clone().into_bytes(),
                    "rule-summary.md",
                ));
                (
                    response,
                    Level::Info,
                    format!("summary generated ({} bytes)", summary.len()),
                )
            }
            Err(error) => {
                let detail = ai_error_detail(&error);
                let (message, level) = summary_error(error);
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
            Ok(book) => book,
            Err(error) => {
                let detail = ai_error_detail(&error);
                let (message, level) = summary_error(error);
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
                self.finish_rules_response(
                    ctx,
                    command,
                    guild,
                    Level::Info,
                    format!("Generated {count} curated rule(s) from uploaded files."),
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
            Ok(book) => book,
            Err(error) => {
                let detail = ai_error_detail(&error);
                let (message, level) = summary_error(error);
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
            if value["name"] == "disable" {
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
            INSERT INTO guild_configs VALUES (1, 1, 10, 'debug', 'all:7');
            INSERT INTO guild_manager_roles VALUES (1, 20);
            INSERT INTO guild_bot_channels VALUES (1, 30);
            INSERT INTO guild_ai_configs VALUES (1, 'https://example.test/v1/chat/completions', 'secret', 'model');",
        )
        .unwrap();

        disable_guild_settings(&db, 1).unwrap();
        let config = get_guild_config(&db, 1).unwrap().unwrap();
        assert!(!config.setup_completed);
        assert_eq!(config.log_channel_id, None);
        assert_eq!(config.log_level, Level::Off);
        assert_eq!(config.retention, retention::Policy::None);
        assert!(config.channel_ids.is_empty());
        assert!(config.admin_role_ids.is_empty());
        assert!(get_ai_config(&db, 1).unwrap().is_none());
    }

    #[test]
    fn settings_report_discloses_user_visible_configuration() {
        let config = GuildConfig {
            guild_id: 1,
            setup_completed: true,
            log_channel_id: Some(10),
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
        let value = serde_json::to_value(settings_embed(&config, Some(&usage), true)).unwrap();
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
            field["name"] == "AI rule review" && field["value"].as_str().unwrap().contains("AI")
        }));
    }
}
