use crate::{
    GuildConfig, get_guild_config,
    retention::{self, Kind},
};
use rusqlite::Connection;
use serde_json::Value;
use serenity::{all::*, async_trait, client::RawEventHandler};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    Off,
    Error,
    Warn,
    Info,
    Debug,
}

impl Level {
    pub(crate) const ALL: [Self; 5] = [Self::Off, Self::Error, Self::Warn, Self::Info, Self::Debug];
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.as_str() == value)
    }
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Off => "Disable guild logging",
            Self::Error => "Failed bot operations",
            Self::Warn => "Errors and rejected or invalid actions",
            Self::Info => "Warnings, errors, and bot actions",
            Self::Debug => "All of the above plus incoming guild events and message content",
        }
    }
    fn allows(self, event: Self) -> bool {
        event != Self::Off && self >= event
    }
}

#[derive(Clone)]
pub(crate) struct Logger {
    database: Arc<Mutex<Connection>>,
    sender: mpsc::Sender<Record>,
}
struct Record {
    ctx: Context,
    guild: GuildId,
    level: Level,
    text: String,
    kind: Kind,
    timestamp: u64,
    mention_roles: Vec<RoleId>,
}

fn destination(config: &GuildConfig, guild: GuildId, level: Level) -> Option<ChannelId> {
    if config.guild_id != guild.get() as i64
        || !config.setup_completed
        || !config.log_level.allows(level)
    {
        return None;
    }
    config
        .log_channel_id
        .and_then(|id| u64::try_from(id).ok())
        .filter(|id| *id != 0)
        .map(ChannelId::new)
}

fn bounded(text: &str, limit: usize) -> String {
    let mut result = String::new();
    let mut units = 0;
    for character in text.chars() {
        units += character.len_utf16();
        if units > limit {
            result.push_str("\n[truncated]");
            break;
        }
        result.push(character);
    }
    result
}

fn belongs_to_guild(source: GuildId, destination_guild: Option<GuildId>) -> bool {
    destination_guild == Some(source)
}

impl Logger {
    pub(crate) fn new(
        database: Arc<Mutex<Connection>>,
        root: std::path::PathBuf,
        storage_lock: Arc<Mutex<()>>,
    ) -> Self {
        // Runs immediately on startup and every minute, including idle guilds.
        let cleanup_db = database.clone();
        let cleanup_root = root.clone();
        let cleanup_lock = storage_lock.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                timer.tick().await;
                let db = cleanup_db.clone();
                let root = cleanup_root.clone();
                let lock = cleanup_lock.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let Ok(db) = db.lock() else { return };
                    let ids = (|| -> rusqlite::Result<Vec<i64>> {
                        db.prepare("SELECT guild_id FROM guild_configs WHERE setup_completed = 1")?
                            .query_map([], |row| row.get(0))?
                            .collect()
                    })();
                    let Ok(ids) = ids else { return };
                    let Ok(_guard) = lock.lock() else { return };
                    for guild in ids {
                        if !root.join(guild.to_string()).exists() {
                            continue;
                        }
                        if let Ok(Some(config)) = get_guild_config(&db, guild) {
                            if retention::prune(
                                &root,
                                guild as u64,
                                config.retention,
                                retention::now(),
                            )
                            .is_err()
                            {
                                eprintln!("Could not clean retained logs for guild {guild}.");
                            }
                        }
                    }
                })
                .await;
            }
        });
        let (sender, mut receiver) = mpsc::channel::<Record>(256);
        let db = database.clone();
        tokio::spawn(async move {
            while let Some(record) = receiver.recv().await {
                // Recheck policy under the same locks as setup and uploads.
                let archive_db = db.clone();
                let archive_root = root.clone();
                let archive_lock = storage_lock.clone();
                let guild = record.guild;
                let level = record.level;
                let kind = record.kind;
                let timestamp = record.timestamp;
                let text = record.text.clone();
                let channel = tokio::task::spawn_blocking(move || {
                    let db = archive_db.lock().ok()?;
                    let config = get_guild_config(&db, guild.get() as i64).ok().flatten()?;
                    if config.setup_completed && config.retention.allows(kind) {
                        let _guard = archive_lock.lock().ok()?;
                        if retention::retain(
                            &archive_root,
                            guild.get(),
                            config.retention,
                            kind,
                            timestamp,
                            &text,
                            retention::now(),
                        )
                        .is_err()
                        {
                            eprintln!(
                                "Could not retain a log for guild {guild}; check quota and storage."
                            );
                        }
                    }
                    destination(&config, guild, level)
                })
                .await
                .ok()
                .flatten();
                let Some(channel) = channel else { continue };
                // Never trust a stored channel ID alone: reject foreign/deleted channels.
                let valid = record.ctx.cache.guild(record.guild).is_some_and(|guild| {
                    guild.channels.get(&channel).is_some_and(|channel| {
                        belongs_to_guild(record.guild, Some(channel.guild_id))
                    })
                });
                if !valid {
                    continue;
                }
                let embed = CreateEmbed::new()
                    .title(format!(
                        "{} · guild {}",
                        record.level.as_str().to_uppercase(),
                        record.guild
                    ))
                    .description(bounded(&record.text, 3800))
                    .timestamp(Timestamp::now())
                    .colour(match record.level {
                        Level::Error => 0xED4245,
                        Level::Warn => 0xFEE75C,
                        Level::Info => 0x57F287,
                        _ => 0x5865F2,
                    });
                let mentions = record
                    .mention_roles
                    .iter()
                    .map(|role| format!("<@&{}>", role.get()))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut message = CreateMessage::new().embed(embed);
                let mut allowed_mentions = CreateAllowedMentions::new()
                    .all_users(false)
                    .everyone(false)
                    .replied_user(false);
                if record.mention_roles.is_empty() {
                    allowed_mentions = allowed_mentions.all_roles(false);
                } else {
                    message = message.content(mentions);
                    allowed_mentions = allowed_mentions.roles(record.mention_roles.clone());
                }
                if channel
                    .send_message(&record.ctx.http, message.allowed_mentions(allowed_mentions))
                    .await
                    .is_err()
                {
                    // Do not recursively log delivery failures or print guild content to the console.
                    eprintln!("Could not deliver a guild log (guild {}).", record.guild);
                }
            }
        });
        Self { database, sender }
    }

    fn config(&self, guild: GuildId) -> Option<GuildConfig> {
        let db = self.database.lock().ok()?;
        get_guild_config(&db, guild.get() as i64).ok().flatten()
    }

    pub(crate) fn log(&self, ctx: &Context, guild: GuildId, level: Level, text: impl Into<String>) {
        self.enqueue(ctx, guild, level, Kind::Action, text.into(), Vec::new());
    }

    fn enqueue(
        &self,
        ctx: &Context,
        guild: GuildId,
        level: Level,
        kind: Kind,
        text: String,
        mention_roles: Vec<RoleId>,
    ) {
        let Some(config) = self.config(guild) else {
            return;
        };
        if !config.setup_completed
            || (destination(&config, guild, level).is_none() && !config.retention.allows(kind))
        {
            return;
        }
        let record = Record {
            ctx: ctx.clone(),
            guild,
            level,
            kind,
            text,
            timestamp: retention::now(),
            mention_roles,
        };
        if self.sender.try_send(record).is_err() {
            eprintln!("Guild log queue full or closed; dropped a record for guild {guild}.");
        }
    }

    pub(crate) fn managed_message_with_mentions(
        &self,
        ctx: &Context,
        message: &Message,
        action: &str,
        level: Level,
        mention_roles: Vec<RoleId>,
    ) {
        let Some(guild) = message.guild_id else {
            return;
        };
        let Ok(mut value) = serde_json::to_value(message) else {
            return;
        };
        redact(&mut value);
        self.enqueue(
            ctx,
            guild,
            level,
            Kind::Managed,
            format!("{action}\n{value}"),
            mention_roles,
        );
    }
}

fn id(value: &Value) -> Option<u64> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_u64())
        .filter(|id| *id != 0)
}

// Only an explicit top-level guild ID can route an event. Never infer a guild
// from nested references, which may contain forwarded or cross-guild content.
fn event_guild(event: &Value) -> Option<GuildId> {
    let data = event.get("d")?;
    let guild = id(&data["guild_id"]).or_else(|| match event["t"].as_str()? {
        "GUILD_CREATE" | "GUILD_UPDATE" | "GUILD_DELETE" => id(&data["id"]),
        _ => None,
    })?;
    Some(GuildId::new(guild))
}

fn redact(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if matches!(
                    key.as_str(),
                    "token"
                        | "api_key"
                        | "key"
                        | "authorization"
                        | "session_id"
                        | "resume_gateway_url"
                        | "email"
                        | "message_snapshots"
                        | "referenced_message"
                ) {
                    *value = Value::String("[redacted]".into());
                } else {
                    redact(value);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(redact),
        _ => {}
    }
}

#[async_trait]
impl RawEventHandler for Logger {
    async fn raw_event(&self, ctx: Context, event: Event) {
        let Ok(mut value) = serde_json::to_value(&event) else {
            return;
        };
        let Some(guild) = event_guild(&value) else {
            return;
        };
        let Some(config) = self.config(guild) else {
            return;
        };
        let kind = match value["t"].as_str() {
            Some(
                "MESSAGE_CREATE" | "MESSAGE_UPDATE" | "MESSAGE_DELETE" | "MESSAGE_DELETE_BULK",
            ) => Kind::Message,
            _ => Kind::Event,
        };
        if destination(&config, guild, Level::Debug).is_none() && !config.retention.allows(kind) {
            return;
        }
        let data = &value["d"];
        // Prevent feedback loops, including message edits/deletions in the log channel.
        if id(&data["channel_id"]) == config.log_channel_id.map(|id| id as u64)
            || id(&data["author"]["id"]) == Some(ctx.cache.current_user().id.get())
        {
            return;
        }
        redact(&mut value);
        // Put message content ahead of large optional payload fields.
        let text = format!(
            "{}\n{}\n{}",
            value["t"].as_str().unwrap_or("GUILD_EVENT"),
            value["d"]["content"].as_str().unwrap_or(""),
            value["d"]
        );
        self.enqueue(&ctx, guild, Level::Debug, kind, text, Vec::new());
    }
}

pub(crate) fn setup_level(response: &CreateInteractionResponse) -> Level {
    // Setup uses a standalone message with content to reject an action; panel
    // messages and successful updates are informational.
    if matches!(response, CreateInteractionResponse::Message(_))
        && serde_json::to_value(response).ok().is_some_and(|value| {
            value["data"]["content"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        })
    {
        Level::Warn
    } else {
        Level::Info
    }
}

pub(crate) fn setup_outcome(response: &CreateInteractionResponse) -> String {
    serde_json::to_value(response)
        .ok()
        .and_then(|value| value["data"]["content"].as_str().map(str::to_owned))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "Updated private setup panel".into())
}

#[cfg(test)]
mod tests;
