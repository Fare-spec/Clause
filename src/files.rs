use crate::{GuildConfig, Handler, get_guild_config, storage};
use serenity::all::*;
use std::time::Duration;

pub(crate) fn command() -> CreateCommand {
    let filename = || {
        CreateCommandOption::new(
            CommandOptionType::String,
            "filename",
            "Exact filename from /files list",
        )
        .required(true)
        .max_length(100)
    };
    CreateCommand::new("files")
        .description("Manage this server's stored files")
        .dm_permission(false)
        // Runtime role checks allow configured managers without Manage Server.
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "list",
                "Browse files and storage usage",
            )
            .add_sub_option(
                CreateCommandOption::new(CommandOptionType::Integer, "page", "Page number")
                    .min_int_value(1),
            ),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "upload",
                &format!(
                    "Upload a file ({} total per server)",
                    storage::limit_label()
                ),
            )
            .add_sub_option(
                CreateCommandOption::new(CommandOptionType::Attachment, "file", "File to upload")
                    .required(true),
            ),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "remove",
                "Permanently remove a stored file",
            )
            .add_sub_option(filename()),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "view",
                "Download a stored file to inspect it",
            )
            .add_sub_option(filename()),
        )
}

fn authorize(
    config: Option<&GuildConfig>,
    member: Option<&Member>,
    channel: ChannelId,
) -> Result<(), &'static str> {
    let config = config
        .filter(|c| c.setup_completed)
        .ok_or("Run /setup before managing files.")?;
    let member = member.ok_or("Use this command inside a server.")?;
    let permissions = member.permissions.unwrap_or_default();
    if !(permissions.administrator()
        || permissions.manage_guild()
        || member
            .roles
            .iter()
            .any(|role| config.admin_role_ids.contains(&(role.get() as i64))))
    {
        return Err("You need a configured bot manager role or Manage Server to manage files.");
    }
    if !config.channel_ids.contains(&(channel.get() as i64)) {
        return Err("Use /files in one of the configured bot channels.");
    }
    Ok(())
}

enum Action {
    List(usize),
    Upload(String, Vec<u8>),
    Remove(String),
    View(String),
}

async fn parse_action(command: &CommandInteraction) -> Result<Action, FileFailure> {
    let options = command.data.options();
    let Some(ResolvedOption {
        name,
        value: ResolvedValue::SubCommand(options),
        ..
    }) = options.first()
    else {
        return Err("Choose list, upload, remove, or view.".into());
    };
    match *name {
        "list" => {
            let page = options
                .iter()
                .find_map(|o| match o.value {
                    ResolvedValue::Integer(n) => Some(n),
                    _ => None,
                })
                .unwrap_or(1);
            Ok(Action::List(
                usize::try_from(page)
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("Invalid page.")?,
            ))
        }
        "remove" | "view" => {
            let filename = options
                .iter()
                .find_map(|o| match o.value {
                    ResolvedValue::String(s) => Some(s),
                    _ => None,
                })
                .ok_or("Choose a filename.")?;
            storage::validate_name(filename, *name == "remove").map_err(|e| e.to_string())?;
            Ok(if *name == "remove" {
                Action::Remove(filename.into())
            } else {
                Action::View(filename.into())
            })
        }
        "upload" => {
            let file = options
                .iter()
                .find_map(|o| match o.value {
                    ResolvedValue::Attachment(a) => Some(a),
                    _ => None,
                })
                .ok_or("Attach a file to upload.")?;
            storage::validate_name(&file.filename, true).map_err(|e| e.to_string())?;
            if u64::from(file.size) > storage::limit_bytes() {
                return Err(
                    format!("File exceeds the {} storage limit.", storage::limit_label()).into(),
                );
            }
            let url = reqwest::Url::parse(&file.url).map_err(|_| "Invalid attachment URL.")?;
            if url.scheme() != "https"
                || !matches!(
                    url.host_str(),
                    Some("cdn.discordapp.com" | "media.discordapp.net")
                )
            {
                return Err("Only Discord-hosted attachments are supported.".into());
            }
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| FileFailure::error("Could not start download."))?;
            let mut response = client
                .get(url)
                .send()
                .await
                .map_err(|_| FileFailure::error("Attachment download failed. Try again."))?;
            if !response.status().is_success() {
                return Err("Attachment download failed. Attach the file again.".into());
            }
            let mut data = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| FileFailure::error("Attachment download failed. Try again."))?
            {
                if data.len() as u64 + chunk.len() as u64 > storage::limit_bytes() {
                    return Err(format!(
                        "File exceeds the {} storage limit.",
                        storage::limit_label()
                    )
                    .into());
                }
                data.extend_from_slice(&chunk);
            }
            if data.len() as u64 != u64::from(file.size) {
                return Err("Attachment size did not match. Try uploading again.".into());
            }
            Ok(Action::Upload(file.filename.clone(), data))
        }
        _ => Err("Unknown file command.".into()),
    }
}

#[derive(Debug)]
struct FileFailure {
    level: crate::logging::Level,
    message: String,
}
impl FileFailure {
    fn error(message: impl Into<String>) -> Self {
        Self {
            level: crate::logging::Level::Error,
            message: message.into(),
        }
    }
}
impl From<String> for FileFailure {
    fn from(message: String) -> Self {
        Self {
            level: crate::logging::Level::Warn,
            message,
        }
    }
}
impl From<&str> for FileFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl Handler {
    pub(crate) async fn files_command(&self, ctx: &Context, command: &CommandInteraction) {
        if let Err(error) = command.defer_ephemeral(&ctx.http).await {
            eprintln!("Failed to acknowledge /files: {error}");
            if let Some(guild) = command.guild_id {
                self.logger.log(
                    ctx,
                    guild,
                    crate::logging::Level::Error,
                    "Failed to acknowledge /files.",
                );
            }
            return;
        }
        let result = self.run_files(command).await;
        if let Some(guild) = command.guild_id {
            let action = command
                .data
                .options
                .first()
                .map(|option| option.name.as_str())
                .unwrap_or("unknown");
            let (level, detail) = match &result {
                Ok(response) => (
                    crate::logging::Level::Info,
                    serde_json::to_value(response)
                        .ok()
                        .and_then(|value| value["content"].as_str().map(str::to_owned))
                        .unwrap_or_else(|| "Listed guild files".into()),
                ),
                Err(error) => (error.level, error.message.clone()),
            };
            self.logger.log(
                ctx,
                guild,
                level,
                format!(
                    "/files {action} by user {} in channel {}: {detail}",
                    command.user.id, command.channel_id
                ),
            );
        }
        let response = match result {
            Ok(response) => response,
            Err(error) => EditInteractionResponse::new().content(error.message),
        }
        .allowed_mentions(
            CreateAllowedMentions::new()
                .all_users(false)
                .all_roles(false)
                .everyone(false),
        );
        if let Err(error) = command.edit_response(&ctx.http, response).await {
            eprintln!("Failed to respond to /files: {error}");
            if let Some(guild) = command.guild_id {
                self.logger.log(
                    ctx,
                    guild,
                    crate::logging::Level::Error,
                    "Failed to deliver /files response.",
                );
            }
            let _ = command.edit_response(&ctx.http, EditInteractionResponse::new()
                .content("Could not deliver the result. Check /files list before retrying a change. Downloads are also subject to Discord's attachment size limit.")).await;
        }
    }

    async fn run_files(
        &self,
        command: &CommandInteraction,
    ) -> Result<EditInteractionResponse, FileFailure> {
        let guild = command.guild_id.ok_or("Use /files inside a server.")?;
        {
            let db = self
                .database
                .lock()
                .map_err(|_| FileFailure::error("Database unavailable."))?;
            let config = get_guild_config(&db, guild.get() as i64)
                .map_err(|_| FileFailure::error("Could not load settings."))?;
            authorize(
                config.as_ref(),
                command.member.as_deref(),
                command.channel_id,
            )?;
        }
        let action = parse_action(command).await?;
        let database = self.database.clone();
        let lock = self.storage_lock.clone();
        let root = self.storage_root.clone();
        let member = command.member.clone();
        let channel = command.channel_id;
        tokio::task::spawn_blocking(move || {
            // Recheck configuration after downloading, then serialize quota checks and writes.
            // Setup takes locks in the same order.
            let db = database.lock().map_err(|_| FileFailure::error("Database unavailable."))?;
            let config = get_guild_config(&db, guild.get() as i64).map_err(|_| FileFailure::error("Could not load settings."))?;
            authorize(config.as_ref(), member.as_deref(), channel)?;
            let _guard = lock.lock().map_err(|_| FileFailure::error("Storage unavailable."))?;
            if let Some(config) = &config {
                crate::retention::prune(&root, guild.get(), config.retention, crate::retention::now())
                    .map_err(|_| FileFailure::error("Could not clean expired logs; check guild storage."))?;
            }
            let mut reply = EditInteractionResponse::new();
            let result: std::io::Result<()> = (|| {
                match action {
                    Action::List(page) => {
                        let (entries, used) = storage::list(&root, guild.get())?;
                        let pages = entries.len().div_ceil(10).max(1);
                        if page > pages { reply = reply.clone().content(format!("Choose a page between 1 and {pages}.")); return Ok(()); }
                        let rows = entries.iter().skip((page - 1) * 10).take(10).map(|(name, size)| {
                            // Escape display names; files created outside the bot may contain Markdown.
                            let display: String = name.chars().take(100).map(|c| if c.is_ascii_alphanumeric() || " ._-".contains(c) { c } else { '?' }).collect();
                            format!("`{display}` — {size} bytes{}", if name == storage::LIMIT_FILE { " (read-only)" } else { "" })
                        }).collect::<Vec<_>>().join("\n");
                        reply = reply.clone().embed(CreateEmbed::new().title("Guild uploads").colour(0x5865F2)
                            .description(if rows.is_empty() { "No files.".into() } else { rows })
                            .footer(CreateEmbedFooter::new(format!("Page {page}/{pages} · {used} / {} bytes total (uploads + logs + metadata) · /files view to download", storage::limit_bytes()))));
                    }
                    Action::Upload(name, data) => {
                        storage::upload(&root, guild.get(), &name, &data)?;
                        reply = reply.clone().content(format!("Uploaded `{name}` ({} bytes).", data.len()));
                    }
                    Action::Remove(name) => {
                        storage::remove(&root, guild.get(), &name)?;
                        reply = reply.clone().content(format!("Removed `{name}`."));
                    }
                    Action::View(name) => {
                        let data = storage::read(&root, guild.get(), &name)?;
                        reply = reply.clone().content(format!("File: `{name}`")).new_attachment(CreateAttachment::bytes(data, name));
                    }
                }
                Ok(())
            })();
            result.map_err(|error| {
                eprintln!("Guild {} file operation failed: {error}", guild.get());
                match error.kind() {
                    std::io::ErrorKind::NotFound => "File or guild folder not found. Check /files list or run /setup again.".into(),
                    std::io::ErrorKind::Other => FileFailure::from(error.to_string()),
                    _ => FileFailure::error("Could not access guild storage. Check the bot's filesystem permissions and available disk space."),
                }
            })?;
            Ok::<_, FileFailure>(reply)
        }).await.map_err(|_| FileFailure::error("File operation failed."))?
    }
}

#[cfg(test)]
mod tests;
