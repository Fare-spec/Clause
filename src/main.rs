mod ai;
mod files;
mod logging;
mod metrics;
mod public_commands;
mod retention;
mod rules;
mod setup;
mod storage;

use std::{
    env,
    error::Error,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, params};

use serenity::{
    all::{
        Command, CreateAllowedMentions, CreateCommand, CreateMessage, GatewayIntents, GuildId,
        Interaction, Message, Permissions, Ready, RoleId,
    },
    async_trait,
    prelude::*,
};

// ============================================================
// DATABASE
// ============================================================

const TABLE_NAME: &str = "guild_configs";

#[derive(Debug, Clone, PartialEq)]
struct GuildConfig {
    pub guild_id: i64,
    pub setup_completed: bool,
    pub log_channel_id: Option<i64>,
    pub rule_source_channel_id: Option<i64>,
    pub log_level: logging::Level,
    pub retention: retention::Policy,
    pub channel_ids: Vec<i64>,
    pub admin_role_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuildAiConfig {
    pub guild_id: i64,
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
}

/// Opens the database and creates the required table if necessary.
fn early_init(database: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(database)?;

    conn.execute(
        &format!(
            "
            CREATE TABLE IF NOT EXISTS {TABLE_NAME} (
                guild_id INTEGER PRIMARY KEY,
                setup_completed INTEGER NOT NULL DEFAULT 0,
                log_channel_id INTEGER,
                channel_to_manage INTEGER,
                admin_role_id INTEGER
            )
            "
        ),
        [],
    )?;

    conn.execute_batch(
        "BEGIN;
        CREATE TABLE IF NOT EXISTS guild_manager_roles (
            guild_id INTEGER NOT NULL, role_id INTEGER NOT NULL,
            PRIMARY KEY (guild_id, role_id));
        CREATE TABLE IF NOT EXISTS guild_bot_channels (
            guild_id INTEGER NOT NULL, channel_id INTEGER NOT NULL,
            PRIMARY KEY (guild_id, channel_id));
        CREATE TABLE IF NOT EXISTS guild_ai_configs (
            guild_id INTEGER PRIMARY KEY,
            endpoint TEXT NOT NULL,
            api_key TEXT NOT NULL,
            model TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS guild_metrics_settings (
            guild_id INTEGER PRIMARY KEY,
            forwarding_enabled INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS guild_moderation_settings (
            guild_id INTEGER PRIMARY KEY,
            auto_delete_enabled INTEGER NOT NULL DEFAULT 0);
        INSERT OR IGNORE INTO guild_manager_roles SELECT guild_id, admin_role_id
            FROM guild_configs WHERE admin_role_id IS NOT NULL;
        INSERT OR IGNORE INTO guild_bot_channels SELECT guild_id, channel_to_manage
            FROM guild_configs WHERE channel_to_manage IS NOT NULL;
        UPDATE guild_configs SET admin_role_id = NULL, channel_to_manage = NULL;
        COMMIT;",
    )?;
    conn.execute_batch(metrics::schema())?;
    let has_level = conn
        .prepare("PRAGMA table_info(guild_configs)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == "log_level");
    if !has_level {
        conn.execute(
            "ALTER TABLE guild_configs ADD COLUMN log_level TEXT NOT NULL DEFAULT 'info'",
            [],
        )?;
    }
    let has_retention = conn
        .prepare("PRAGMA table_info(guild_configs)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == "retention");
    if !has_retention {
        conn.execute(
            "ALTER TABLE guild_configs ADD COLUMN retention TEXT NOT NULL DEFAULT 'none'",
            [],
        )?;
    }
    let has_rule_source = conn
        .prepare("PRAGMA table_info(guild_configs)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == "rule_source_channel_id");
    if !has_rule_source {
        conn.execute(
            "ALTER TABLE guild_configs ADD COLUMN rule_source_channel_id INTEGER",
            [],
        )?;
    }
    Ok(conn)
}

/// Reads a guild config from SQLite.
fn get_guild_config(conn: &Connection, guild_id: i64) -> rusqlite::Result<Option<GuildConfig>> {
    let mut statement = conn.prepare(&format!(
        "
            SELECT
                guild_id,
                setup_completed,
                log_channel_id, rule_source_channel_id, log_level, retention
            FROM {TABLE_NAME}
            WHERE guild_id = ?1
            "
    ))?;

    let mut rows = statement.query(params![guild_id])?;

    let Some(row) = rows.next()? else {
        return Ok(None);
    };

    Ok(Some(GuildConfig {
        guild_id: row.get(0)?,
        setup_completed: row.get(1)?,
        log_channel_id: row.get(2)?,
        rule_source_channel_id: row.get(3)?,
        log_level: logging::Level::parse(&row.get::<_, String>(4)?).unwrap_or(logging::Level::Off),
        retention: retention::Policy::parse(&row.get::<_, String>(5)?)
            .unwrap_or(retention::Policy::None),
        channel_ids: read_ids(conn, "guild_bot_channels", "channel_id", guild_id)?,
        admin_role_ids: read_ids(conn, "guild_manager_roles", "role_id", guild_id)?,
    }))
}

fn read_ids(
    conn: &Connection,
    table: &str,
    column: &str,
    guild_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    conn.prepare(&format!(
        "SELECT {column} FROM {table} WHERE guild_id = ?1 ORDER BY {column}"
    ))?
    .query_map([guild_id], |row| row.get(0))?
    .collect()
}

pub(crate) fn get_ai_config(
    conn: &Connection,
    guild_id: i64,
) -> rusqlite::Result<Option<GuildAiConfig>> {
    let mut statement = conn.prepare(
        "SELECT guild_id, endpoint, api_key, model FROM guild_ai_configs WHERE guild_id = ?1",
    )?;
    let mut rows = statement.query([guild_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(GuildAiConfig {
        guild_id: row.get(0)?,
        endpoint: row.get(1)?,
        api_key: row.get(2)?,
        model: row.get(3)?,
    }))
}

pub(crate) fn save_ai_config(conn: &Connection, config: &GuildAiConfig) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO guild_ai_configs (guild_id, endpoint, api_key, model)
        VALUES (?1, ?2, ?3, ?4) ON CONFLICT(guild_id) DO UPDATE SET
        endpoint = excluded.endpoint, api_key = excluded.api_key, model = excluded.model",
        params![
            config.guild_id,
            config.endpoint,
            config.api_key,
            config.model
        ],
    )?;
    Ok(())
}

pub(crate) fn delete_ai_config(conn: &Connection, guild_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM guild_ai_configs WHERE guild_id = ?1",
        [guild_id],
    )?;
    Ok(())
}

pub(crate) fn metrics_forwarding_enabled(
    conn: &Connection,
    guild_id: i64,
) -> rusqlite::Result<bool> {
    let mut statement =
        conn.prepare("SELECT forwarding_enabled FROM guild_metrics_settings WHERE guild_id = ?1")?;
    let mut rows = statement.query([guild_id])?;
    let Some(row) = rows.next()? else {
        return Ok(false);
    };
    Ok(row.get::<_, bool>(0)?)
}

pub(crate) fn set_metrics_forwarding(
    conn: &Connection,
    guild_id: i64,
    enabled: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO guild_metrics_settings (guild_id, forwarding_enabled)
        VALUES (?1, ?2) ON CONFLICT(guild_id) DO UPDATE SET
        forwarding_enabled = excluded.forwarding_enabled",
        params![guild_id, enabled],
    )?;
    Ok(())
}

pub(crate) fn auto_delete_enabled(conn: &Connection, guild_id: i64) -> rusqlite::Result<bool> {
    let mut statement = conn
        .prepare("SELECT auto_delete_enabled FROM guild_moderation_settings WHERE guild_id = ?1")?;
    let mut rows = statement.query([guild_id])?;
    let Some(row) = rows.next()? else {
        return Ok(false);
    };
    Ok(row.get::<_, bool>(0)?)
}

pub(crate) fn set_auto_delete(
    conn: &Connection,
    guild_id: i64,
    enabled: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO guild_moderation_settings (guild_id, auto_delete_enabled)
        VALUES (?1, ?2) ON CONFLICT(guild_id) DO UPDATE SET
        auto_delete_enabled = excluded.auto_delete_enabled",
        params![guild_id, enabled],
    )?;
    Ok(())
}

// ============================================================
// DISCORD HANDLER
// ============================================================

fn truncate_utf16(text: &str, limit: usize) -> String {
    let mut result = String::new();
    let mut units = 0;
    for character in text.chars() {
        units += character.len_utf16();
        if units > limit {
            result.push_str("…");
            break;
        }
        result.push(character);
    }
    result
}

fn rule_ids(review: &ai::MessageReview) -> String {
    if review.rule_ids.is_empty() {
        "unknown".into()
    } else {
        review
            .rule_ids
            .iter()
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn message_for_ai(message: &Message) -> Option<String> {
    let content = message.content.trim();
    if content.is_empty() && message.attachments.is_empty() {
        return None;
    }
    let mut text = String::new();
    if !content.is_empty() {
        text.push_str("Content:\n");
        text.push_str(content);
        text.push('\n');
    }
    if !message.attachments.is_empty() {
        text.push_str("Attachments:\n");
        for attachment in &message.attachments {
            let content_type = attachment.content_type.as_deref().unwrap_or("unknown type");
            text.push_str(&format!(
                "- {} ({}; {} bytes)\n",
                attachment.filename, content_type, attachment.size
            ));
        }
    }
    Some(text)
}

fn moderation_report(guild: GuildId, message: &Message, review: &ai::MessageReview) -> String {
    let link = format!(
        "https://discord.com/channels/{}/{}/{}",
        guild.get(),
        message.channel_id.get(),
        message.id.get()
    );
    let quotes = if review.quoted_rules.is_empty() {
        "No exact rule quote returned by AI.".into()
    } else {
        review
            .quoted_rules
            .iter()
            .map(|quote| format!("- `{}`: {}", quote.id, truncate_utf16(&quote.text, 500)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "AI rule review: {}\nSeverity: {}\nConfidence: {:.0}%\nAuthor: <@{}> ({})\nChannel: <#{}>\nMessage: {}\nMatched rules: {}\nReason: {}\nQuoted rules:\n{}",
        review.label().to_uppercase(),
        review.severity,
        review.confidence * 100.0,
        message.author.id.get(),
        message.author.id.get(),
        message.channel_id.get(),
        link,
        rule_ids(review),
        truncate_utf16(&review.reason, 800),
        quotes
    )
}

fn should_auto_delete(review: &ai::MessageReview, manager: bool, enabled: bool) -> bool {
    enabled
        && !manager
        && review.status == ai::ReviewStatus::Violation
        && review.confidence >= 0.90
        && matches!(review.severity.as_str(), "high" | "critical")
}

fn moderation_response(review: &ai::MessageReview) -> String {
    match review.status {
        ai::ReviewStatus::Violation => format!(
            "Clause flagged this message for staff review because it appears to break server rules. Reason: {}\nMatched rules: {}.",
            truncate_utf16(&review.reason, 900),
            rule_ids(review)
        ),
        ai::ReviewStatus::GrayArea => format!(
            "Clause is not sure this message fits the server rules, so staff have been asked to review it. Reason: {}\nPossible rules: {}.",
            truncate_utf16(&review.reason, 900),
            rule_ids(review)
        ),
        ai::ReviewStatus::Compliant => String::new(),
    }
}

struct Handler {
    database: Arc<Mutex<Connection>>,
    setup: Mutex<setup::Sessions>,
    storage_root: std::path::PathBuf,
    storage_lock: Arc<Mutex<()>>,
    logger: logging::Logger,
    ai: ai::Ai,
    rules: Arc<Mutex<rules::Cache>>,
}

impl Handler {
    pub(crate) fn record_metric(&self, guild: GuildId, counter: metrics::Counter, amount: u64) {
        if let Ok(db) = self.database.lock() {
            let _ = metrics::increment(&db, guild.get() as i64, counter, amount);
        }
    }

    pub(crate) fn record_ai_tokens(&self, guild: GuildId, usage: ai::TokenUsage) {
        if let Ok(db) = self.database.lock() {
            let _ = metrics::add_tokens(
                &db,
                guild.get() as i64,
                usage.input,
                usage.output,
                usage.total,
            );
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    // --------------------------------------------------------
    // BOT READY
    // --------------------------------------------------------

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("Connected as {}", ready.user.name);
        for guild in &ready.guilds {
            self.logger.log(
                &ctx,
                guild.id,
                logging::Level::Info,
                "Bot connected to Discord.",
            );
        }

        let setup_command = CreateCommand::new("setup")
            .description("Setup the bot for this server.")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .dm_permission(false);

        let mut commands = public_commands::commands();
        commands.push(files::command());
        commands.push(setup_command);

        if let Some(guild) = command_guild_id() {
            match guild.set_commands(&ctx.http, commands).await {
                Ok(commands) => {
                    println!(
                        "Registered {} guild command(s) for guild {}",
                        commands.len(),
                        guild
                    );
                }
                Err(error) => eprintln!("Failed to register guild commands: {error}"),
            }
        } else {
            match Command::set_global_commands(&ctx.http, commands).await {
                Ok(commands) => {
                    println!("Registered {} global command(s)", commands.len());
                }
                Err(error) => eprintln!("Failed to register global commands: {error}"),
            }
        }
    }

    // --------------------------------------------------------
    // SLASH COMMANDS
    // --------------------------------------------------------

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let result = match interaction {
            Interaction::Command(command)
                if matches!(
                    command.data.name.as_str(),
                    "storage"
                        | "metrics"
                        | "disable"
                        | "leave"
                        | "settings"
                        | "summary"
                        | "rules"
                        | "ai"
                        | "logs"
                        | "privacy"
                        | "terms"
                ) =>
            {
                self.public_command(&ctx, &command).await;
                return;
            }
            Interaction::Command(command) if command.data.name == "files" => {
                self.files_command(&ctx, &command).await;
                return;
            }
            Interaction::Command(command) if command.data.name == "setup" => {
                let response = self.setup_response(&ctx, &command);
                let outcome = logging::setup_outcome(&response);
                let level = logging::setup_level(&response);
                let result = command.create_response(&ctx.http, response).await;
                if let Some(guild) = command.guild_id {
                    self.logger.log(
                        &ctx,
                        guild,
                        if result.is_ok() {
                            level
                        } else {
                            logging::Level::Error
                        },
                        format!(
                            "/setup for user {} in channel {}: {outcome}; response {}",
                            command.user.id,
                            command.channel_id,
                            if result.is_ok() { "sent" } else { "failed" }
                        ),
                    );
                }
                result
            }
            Interaction::Component(component) if component.data.custom_id.starts_with("setup:") => {
                let response = self.setup_component(&ctx, &component);
                let outcome = logging::setup_outcome(&response);
                let level = logging::setup_level(&response);
                let result = component.create_response(&ctx.http, response).await;
                if let Some(guild) = component.guild_id {
                    self.logger.log(
                        &ctx,
                        guild,
                        if result.is_ok() {
                            level
                        } else {
                            logging::Level::Error
                        },
                        format!(
                            "Setup action by user {} in channel {}: {outcome}; response {}",
                            component.user.id,
                            component.channel_id,
                            if result.is_ok() { "sent" } else { "failed" }
                        ),
                    );
                }
                result
            }
            _ => return,
        };
        if let Err(error) = result {
            eprintln!("Failed to respond to setup interaction: {error}");
        }
    }

    // --------------------------------------------------------
    // NORMAL MESSAGES
    // --------------------------------------------------------

    async fn message(&self, ctx: Context, message: Message) {
        if message.author.bot {
            return;
        }
        let Some(guild) = message.guild_id else {
            return;
        };

        let (config, auto_delete) = {
            let Ok(database) = self.database.lock() else {
                return;
            };
            let config = match get_guild_config(&database, guild.get() as i64) {
                Ok(Some(config)) if config.setup_completed => config,
                Ok(_) => return,
                Err(error) => {
                    eprintln!("Failed to read configuration: {error}");
                    return;
                }
            };
            let auto_delete = auto_delete_enabled(&database, guild.get() as i64).unwrap_or(false);
            (config, auto_delete)
        };
        let permissions = message.author_permissions(&ctx).unwrap_or_default();
        let manager_role = message.member.as_ref().is_some_and(|member| {
            member
                .roles
                .iter()
                .any(|id| config.admin_role_ids.contains(&(id.get() as i64)))
        });
        let manager = permissions.administrator() || permissions.manage_guild() || manager_role;
        let in_rule_source = config.rule_source_channel_id == Some(message.channel_id.get() as i64);
        if in_rule_source {
            if manager {
                self.ingest_rule_source_message(&ctx, &message, guild).await;
            }
            return;
        }

        let in_bot_channel = config
            .channel_ids
            .contains(&(message.channel_id.get() as i64));
        if !in_bot_channel {
            return;
        }

        match message.content.as_str() {
            "!ping" if manager => {
                let result = message.channel_id.say(&ctx.http, "Pong!").await;
                self.logger.log(
                    &ctx,
                    guild,
                    if result.is_ok() {
                        logging::Level::Info
                    } else {
                        logging::Level::Error
                    },
                    format!(
                        "!ping reply for user {} in channel {}: {}",
                        message.author.id,
                        message.channel_id,
                        if result.is_ok() { "sent" } else { "failed" }
                    ),
                );
                return;
            }
            "!config" if manager => {
                let content = format!(
                    "\
Guild ID: {}
Setup completed: {}
Log channel: {:?}
Rule source channel: {:?}
Bot channels: {:?}
Manager roles: {:?}
Logging level: {}
Retention: {}",
                    config.guild_id,
                    config.setup_completed,
                    config.log_channel_id,
                    config.rule_source_channel_id,
                    config.channel_ids,
                    config.admin_role_ids,
                    config.log_level.as_str(),
                    config.retention.label(),
                );
                let result = message.channel_id.say(&ctx.http, content).await;
                self.logger.log(
                    &ctx,
                    guild,
                    if result.is_ok() {
                        logging::Level::Info
                    } else {
                        logging::Level::Error
                    },
                    format!(
                        "!config reply for user {} in channel {}: {}",
                        message.author.id,
                        message.channel_id,
                        if result.is_ok() { "sent" } else { "failed" }
                    ),
                );
                return;
            }
            _ => {}
        }

        let Some(message_text) = message_for_ai(&message) else {
            return;
        };
        self.record_metric(guild, metrics::Counter::MessagesSeen, 1);

        let root = self.storage_root.clone();
        let lock = self.storage_lock.clone();
        let cache = self.rules.clone();
        let rulebook = tokio::task::spawn_blocking(move || {
            let _guard = lock.lock().map_err(|_| crate::ai::SummaryError::Storage)?;
            let mut cache = cache.lock().map_err(|_| crate::ai::SummaryError::Storage)?;
            let book = cache
                .get(&root, guild.get())
                .map_err(|_| crate::ai::SummaryError::Storage)?;
            if book.rules.is_empty() {
                return Err(crate::ai::SummaryError::NoRuleFiles);
            }
            Ok(rules::to_ai_text(&book))
        })
        .await
        .map_err(|_| crate::ai::SummaryError::Storage)
        .and_then(|result| result);

        let Ok(rulebook) = rulebook else {
            return;
        };
        let ai_config = self.guild_ai_config(guild);
        self.record_metric(guild, metrics::Counter::AiReviews, 1);
        let review = self
            .ai
            .review_message(&rulebook, &message_text, ai_config.as_ref())
            .await;
        let review = match review {
            Ok(review) if review.value.needs_action() => review,
            Ok(review) => {
                self.record_ai_tokens(guild, review.usage);
                self.record_metric(guild, metrics::Counter::AiCompliant, 1);
                return;
            }
            Err(crate::ai::SummaryError::MissingConfig | crate::ai::SummaryError::NoRuleFiles) => {
                return;
            }
            Err(error) => {
                self.record_metric(guild, metrics::Counter::AiErrors, 1);
                let level = match error {
                    crate::ai::SummaryError::BadResponse
                    | crate::ai::SummaryError::ProviderRequest(_)
                    | crate::ai::SummaryError::ProviderResponse(_) => logging::Level::Error,
                    _ => logging::Level::Warn,
                };
                let detail = match &error {
                    crate::ai::SummaryError::ProviderRequest(diagnostic)
                    | crate::ai::SummaryError::ProviderResponse(diagnostic) => {
                        diagnostic.to_string()
                    }
                    _ => format!("{error:?}"),
                };
                self.logger.log(
                    &ctx,
                    guild,
                    level,
                    format!(
                        "AI rule review skipped for message {} by user {} in channel {}: {detail}",
                        message.id, message.author.id, message.channel_id
                    ),
                );
                return;
            }
        };

        let mention_roles = config
            .admin_role_ids
            .iter()
            .filter_map(|id| u64::try_from(*id).ok())
            .map(RoleId::new)
            .collect::<Vec<_>>();
        let review_usage = review.usage;
        let review = review.value;
        self.record_ai_tokens(guild, review_usage);
        match review.status {
            ai::ReviewStatus::GrayArea => {
                self.record_metric(guild, metrics::Counter::AiGrayArea, 1)
            }
            ai::ReviewStatus::Violation => {
                self.record_metric(guild, metrics::Counter::AiViolations, 1)
            }
            ai::ReviewStatus::Compliant => {
                self.record_metric(guild, metrics::Counter::AiCompliant, 1)
            }
        }
        let report = moderation_report(guild, &message, &review);
        self.logger.managed_message_with_mentions(
            &ctx,
            &message,
            &report,
            logging::Level::Warn,
            mention_roles,
        );
        self.record_metric(guild, metrics::Counter::StaffReviewPings, 1);

        if should_auto_delete(&review, manager, auto_delete) {
            match message.delete(&ctx.http).await {
                Ok(()) => {
                    self.record_metric(guild, metrics::Counter::AiDeletedMessages, 1);
                    self.logger.log(
                        &ctx,
                        guild,
                        logging::Level::Warn,
                        format!(
                            "Deleted AI-flagged message {} in channel {} with confidence {:.0}%.",
                            message.id,
                            message.channel_id,
                            review.confidence * 100.0
                        ),
                    );
                    return;
                }
                Err(_) => {
                    self.record_metric(guild, metrics::Counter::AiDeleteFailures, 1);
                    self.logger.log(
                        &ctx,
                        guild,
                        logging::Level::Error,
                        format!(
                            "Could not delete AI-flagged message {} in channel {}.",
                            message.id, message.channel_id
                        ),
                    );
                }
            }
        }

        let response = moderation_response(&review);
        let send_result = message
            .channel_id
            .send_message(
                &ctx.http,
                CreateMessage::new()
                    .content(response)
                    .reference_message(&message)
                    .allowed_mentions(
                        CreateAllowedMentions::new()
                            .all_users(false)
                            .all_roles(false)
                            .everyone(false)
                            .replied_user(false),
                    ),
            )
            .await;
        if send_result.is_ok() {
            self.record_metric(guild, metrics::Counter::BotReplies, 1);
        }
        if send_result.is_err() {
            self.logger.log(
                &ctx,
                guild,
                logging::Level::Error,
                format!(
                    "Could not send active AI rule response for message {} in channel {}.",
                    message.id, message.channel_id
                ),
            );
        }
    }
}

impl Handler {
    async fn ingest_rule_source_message(&self, ctx: &Context, message: &Message, guild: GuildId) {
        let content = message.content.trim();
        if content.is_empty() {
            return;
        }
        self.record_metric(guild, metrics::Counter::RuleSourceMessages, 1);
        let ai_config = self.guild_ai_config(guild);
        let public = {
            let root = self.storage_root.clone();
            let lock = self.storage_lock.clone();
            let cache = self.rules.clone();
            tokio::task::spawn_blocking(move || {
                let _guard = lock.lock().map_err(|_| crate::ai::SummaryError::Storage)?;
                let mut cache = cache.lock().map_err(|_| crate::ai::SummaryError::Storage)?;
                cache
                    .get(&root, guild.get())
                    .map(|book| book.public)
                    .map_err(|_| crate::ai::SummaryError::Storage)
            })
            .await
            .map_err(|_| crate::ai::SummaryError::Storage)
            .and_then(|result| result)
            .unwrap_or(false)
        };
        let generated = self
            .ai
            .generate_rulebook_from_messages(
                &[(message.author.id.to_string(), content.to_owned())],
                public,
                ai_config.as_ref(),
            )
            .await;
        let generated = match generated {
            Ok(book) => book,
            Err(crate::ai::SummaryError::NoRuleFiles) => return,
            Err(error) => {
                self.record_metric(guild, metrics::Counter::AiErrors, 1);
                let detail = match &error {
                    crate::ai::SummaryError::ProviderRequest(diagnostic)
                    | crate::ai::SummaryError::ProviderResponse(diagnostic) => {
                        diagnostic.to_string()
                    }
                    _ => format!("{error:?}"),
                };
                self.logger.log(
                    ctx,
                    guild,
                    logging::Level::Error,
                    format!(
                        "Rule source import failed for message {} by user {} in channel {}: {detail}",
                        message.id, message.author.id, message.channel_id
                    ),
                );
                return;
            }
        };
        let generated_usage = generated.usage;
        let generated = generated.value;
        self.record_ai_tokens(guild, generated_usage);
        let generated_count = generated.rules.len();
        let saved = {
            let root = self.storage_root.clone();
            let lock = self.storage_lock.clone();
            let cache = self.rules.clone();
            tokio::task::spawn_blocking(move || -> Result<usize, String> {
                let _guard = lock
                    .lock()
                    .map_err(|_| "Storage is unavailable.".to_owned())?;
                let mut cache = cache
                    .lock()
                    .map_err(|_| "Rules cache is unavailable.".to_owned())?;
                let mut book = cache
                    .get(&root, guild.get())
                    .map_err(|_| "Could not read existing rules.".to_owned())?;
                book.public = public;
                for rule in generated.rules {
                    rules::upsert(&mut book, &rule.id, &rule.severity, Some(&rule.text))
                        .map_err(|_| "Could not merge generated rule.".to_owned())?;
                }
                cache
                    .save(&root, guild.get(), &book)
                    .map_err(|_| "Could not save generated rules.".to_owned())?;
                Ok(generated_count)
            })
            .await
        };
        match saved {
            Ok(Ok(count)) => {
                self.logger.log(
                    ctx,
                    guild,
                    logging::Level::Info,
                    format!(
                        "Rule source message {} by user {} updated {} curated rule(s).",
                        message.id, message.author.id, count
                    ),
                );
                self.record_metric(guild, metrics::Counter::RulesImported, count as u64);
                let _ = message
                    .channel_id
                    .send_message(
                        &ctx.http,
                        CreateMessage::new()
                            .content(format!(
                                "Updated curated rules from this message: {count} rule(s)."
                            ))
                            .reference_message(message)
                            .allowed_mentions(
                                CreateAllowedMentions::new()
                                    .all_users(false)
                                    .all_roles(false)
                                    .everyone(false)
                                    .replied_user(false),
                            ),
                    )
                    .await;
            }
            Ok(Err(error)) => {
                self.logger.log(
                    ctx,
                    guild,
                    logging::Level::Error,
                    format!(
                        "Rule source message {} by user {} could not be saved: {error}",
                        message.id, message.author.id
                    ),
                );
            }
            Err(_) => {
                self.logger.log(
                    ctx,
                    guild,
                    logging::Level::Error,
                    format!(
                        "Rule source message {} by user {} could not be processed.",
                        message.id, message.author.id
                    ),
                );
            }
        }
    }
}

fn command_guild_id() -> Option<GuildId> {
    let value = env::var("COMMAND_GUILD_ID").ok()?;
    match value.trim().parse::<u64>() {
        Ok(id) if id != 0 => Some(GuildId::new(id)),
        _ => {
            eprintln!("Ignoring invalid COMMAND_GUILD_ID; registering global commands.");
            None
        }
    }
}

// ============================================================
// SHUTDOWN
// ============================================================

async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate())?;

        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

// ============================================================
// MAIN
// ============================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // --------------------------------------------------------
    // ENV
    // --------------------------------------------------------

    let token =
        env::var("DISCORD_TOKEN").map_err(|_| "Set DISCORD_TOKEN to your Discord bot token")?;

    let database_path = env::var("DATABASE_PATH").unwrap_or_else(|_| "bot.db".to_string());

    // --------------------------------------------------------
    // DATABASE
    // --------------------------------------------------------

    let connection = early_init(&database_path)?;

    let database = Arc::new(Mutex::new(connection));

    println!("Database initialized: {database_path}");

    // --------------------------------------------------------
    // DISCORD
    // --------------------------------------------------------

    let intents =
        GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let storage_root: std::path::PathBuf = env::var_os("GUILD_STORAGE_PATH")
        .map(Into::into)
        .unwrap_or_else(|| "guilds".into());
    let storage_lock = Arc::new(Mutex::new(()));
    let logger = logging::Logger::new(database.clone(), storage_root.clone(), storage_lock.clone());
    let handler = Handler {
        logger: logger.clone(),
        ai: ai::Ai::from_env(),
        database,
        rules: Arc::new(Mutex::new(rules::Cache::default())),
        setup: Mutex::new(setup::Sessions::default()),
        storage_lock,
        storage_root,
    };

    let mut client = Client::builder(token, intents)
        .raw_event_handler(logger)
        .event_handler(handler)
        .await?;

    let shard_manager = client.shard_manager.clone();

    // --------------------------------------------------------
    // RUN
    // --------------------------------------------------------

    tokio::select! {
        result = client.start() => {
            result?;
        }

        result = shutdown_signal() => {
            result?;

            println!("Shutting down...");

            shard_manager
                .shutdown_all()
                .await;
        }
    }

    Ok(())
}
