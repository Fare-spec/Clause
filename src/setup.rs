use crate::{GuildConfig, Handler, get_guild_config};
use serenity::all::*;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(15 * 60);

#[derive(Default)]
pub(crate) struct Sessions(HashMap<GuildId, Draft>);

struct Draft {
    owner: UserId,
    token: String,
    started: Instant,
    original: Option<GuildConfig>,
    config: GuildConfig,
    logging_page: bool,
}

fn notice(text: &str) -> CreateInteractionResponse {
    CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(text)
            .ephemeral(true),
    )
}

fn can_setup(member: Option<&Member>) -> bool {
    member
        .and_then(|m| m.permissions)
        .is_some_and(|p| p.administrator() || p.manage_guild())
}

fn complete(config: &GuildConfig) -> bool {
    (1..=25).contains(&config.admin_role_ids.len())
        && (1..=25).contains(&config.channel_ids.len())
        && config.log_channel_id.is_some()
}

fn mentions(ids: &[i64], prefix: &str) -> String {
    if ids.is_empty() {
        "Not selected".into()
    } else {
        ids.iter()
            .map(|id| format!("<{prefix}{id}>"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn panel(draft: &Draft) -> CreateInteractionResponseMessage {
    let c = &draft.config;
    let menu = |action: &str, label: &str, kind, maximum| {
        CreateActionRow::SelectMenu(
            CreateSelectMenu::new(format!("setup:{}:{action}", draft.token), kind)
                .placeholder(label)
                .min_values(1)
                .max_values(maximum),
        )
    };
    let channel_kind = |ids: &[i64]| CreateSelectMenuKind::Channel {
        channel_types: Some(vec![ChannelType::Text]),
        default_channels: Some(ids.iter().map(|id| ChannelId::new(*id as u64)).collect()),
    };
    let button = |action: &str, label: &str| {
        CreateButton::new(format!("setup:{}:{action}", draft.token)).label(label)
    };
    let rows = if draft.logging_page {
        vec![
            menu(
                "level",
                "Discord logging level",
                CreateSelectMenuKind::String {
                    options: crate::logging::Level::ALL
                        .iter()
                        .map(|level| {
                            CreateSelectMenuOption::new(level.as_str(), level.as_str())
                                .description(level.description())
                                .default_selection(*level == c.log_level)
                        })
                        .collect(),
                },
                1,
            ),
            menu(
                "retention",
                "What should be kept on disk, and for how long?",
                CreateSelectMenuKind::String {
                    options: crate::retention::Policy::OPTIONS
                        .iter()
                        .map(|policy| {
                            CreateSelectMenuOption::new(policy.label(), policy.key())
                                .default_selection(*policy == c.retention)
                        })
                        .collect(),
                },
                1,
            ),
            CreateActionRow::Buttons(vec![
                button("back", "Back"),
                button("save", "Save settings")
                    .style(ButtonStyle::Success)
                    .disabled(!complete(c)),
                button("cancel", "Cancel").style(ButtonStyle::Secondary),
            ]),
        ]
    } else {
        vec![
            menu(
                "role",
                "1 · Select manager roles (up to 25)",
                CreateSelectMenuKind::Role {
                    default_roles: Some(
                        c.admin_role_ids
                            .iter()
                            .map(|id| RoleId::new(*id as u64))
                            .collect(),
                    ),
                },
                25,
            ),
            menu(
                "channel",
                "2 · Select bot channels (up to 25)",
                channel_kind(&c.channel_ids),
                25,
            ),
            menu(
                "logs",
                "3 · Select the log channel",
                channel_kind(&c.log_channel_id.into_iter().collect::<Vec<_>>()),
                1,
            ),
            CreateActionRow::Buttons(vec![
                button("next", "Next: logging & retention").style(ButtonStyle::Primary),
                button("cancel", "Cancel").style(ButtonStyle::Secondary),
            ]),
        ]
    };
    CreateInteractionResponseMessage::new().ephemeral(true)
        .allowed_mentions(CreateAllowedMentions::new().all_users(false).all_roles(false).everyone(false))
        .embed(CreateEmbed::new().title(if draft.logging_page { "Set up Clause · 2/2" } else { "Set up Clause · 1/2" }).colour(0x5865F2)
            .description("Choose your settings, then Save on page 2. Only you can see this panel.\nAdministrators and members with Manage Server retain access and can run /setup.\nRetained logs and uploads share 50 Mo. Saving None clears retained logs; shorter periods expire older records. Flagged-only keeps AI-flagged or gray-area review records.\nChanges save together. Panel expires after 15 minutes.")
            .field("Manager roles", mentions(&c.admin_role_ids, "@&"), false)
            .field("Bot channels", mentions(&c.channel_ids, "#"), false)
            .field("Log channel", mentions(&c.log_channel_id.into_iter().collect::<Vec<_>>(), "#"), false)
            .field("Discord logging level", c.log_level.as_str(), true)
            .field("Disk retention", c.retention.label(), false)
            .footer(CreateEmbedFooter::new("Discord messages are not deleted by disk retention. Restrict your log channel to trusted members.")))
        .components(rows)
}

// Validate against the guild cache, including channel permission overwrites.
// Fail closed if the guild or bot member has not been cached yet.
fn validate(ctx: &Context, guild_id: GuildId, config: &GuildConfig) -> Result<(), &'static str> {
    let guild = ctx
        .cache
        .guild(guild_id)
        .ok_or("Server information is unavailable. Try again shortly.")?;
    for &role in &config.admin_role_ids {
        if role as u64 == guild_id.get() || !guild.roles.contains_key(&RoleId::new(role as u64)) {
            return Err("Select an existing manager role other than @everyone.");
        }
    }
    let bot = guild
        .members
        .get(&ctx.cache.current_user().id)
        .ok_or("The bot's server permissions are unavailable. Try again shortly.")?;
    for id in config
        .channel_ids
        .iter()
        .copied()
        .chain(config.log_channel_id)
    {
        let channel = guild
            .channels
            .get(&ChannelId::new(id as u64))
            .ok_or("A selected channel no longer exists. Choose another channel.")?;
        if channel.kind != ChannelType::Text {
            return Err("Select server text channels for the bot and logs.");
        }
        if !guild
            .user_permissions_in(channel, bot)
            .contains(Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES)
        {
            return Err(
                "The bot needs View Channel and Send Messages in every selected channel. Update permissions or choose another channel.",
            );
        }
    }
    if let Some(id) = config.log_channel_id {
        if let Some(channel) = guild.channels.get(&ChannelId::new(id as u64)) {
            if !guild
                .user_permissions_in(channel, bot)
                .contains(Permissions::EMBED_LINKS)
            {
                return Err("The bot needs Embed Links in the log channel to send logs.");
            }
        }
    }
    Ok(())
}

impl Handler {
    pub(crate) fn setup_response(
        &self,
        _ctx: &Context,
        command: &CommandInteraction,
    ) -> CreateInteractionResponse {
        let Some(guild) = command.guild_id else {
            return notice("Use /setup inside a server.");
        };
        if !can_setup(command.member.as_deref()) {
            return notice("You need Manage Server to configure the bot.");
        }
        let Ok(mut sessions) = self.setup.lock() else {
            return notice("Setup is temporarily unavailable.");
        };
        sessions
            .0
            .retain(|_, draft| draft.started.elapsed() < LIFETIME);
        if sessions
            .0
            .get(&guild)
            .is_some_and(|d| d.owner != command.user.id)
        {
            return notice(
                "Another administrator is configuring this server. Wait for them to save, cancel, or for the panel to expire.",
            );
        }
        let original = match self
            .database
            .lock()
            .ok()
            .and_then(|db| get_guild_config(&db, guild.get() as i64).ok())
        {
            Some(config) => config,
            None => return notice("Could not load settings. Please try again."),
        };
        let draft = Draft {
            owner: command.user.id,
            logging_page: false,
            token: command.id.to_string(),
            started: Instant::now(),
            config: original.clone().unwrap_or(GuildConfig {
                guild_id: guild.get() as i64,
                setup_completed: false,
                admin_role_ids: vec![],
                channel_ids: vec![],
                log_channel_id: None,
                log_level: crate::logging::Level::Info,
                retention: crate::retention::Policy::None,
            }),
            original,
        };
        let response = panel(&draft);
        sessions.0.insert(guild, draft);
        CreateInteractionResponse::Message(response)
    }

    pub(crate) fn setup_component(
        &self,
        ctx: &Context,
        component: &ComponentInteraction,
    ) -> CreateInteractionResponse {
        let Some(guild) = component.guild_id else {
            return notice("Use /setup inside a server.");
        };
        if !can_setup(component.member.as_ref()) {
            return notice("You need Manage Server to configure the bot.");
        }
        let Ok(mut sessions) = self.setup.lock() else {
            return notice("Setup is temporarily unavailable.");
        };
        sessions
            .0
            .retain(|_, draft| draft.started.elapsed() < LIFETIME);
        let Some(draft) = sessions.0.get_mut(&guild) else {
            return notice("This setup expired or the bot restarted. Run /setup again.");
        };
        let parts: Vec<_> = component.data.custom_id.split(':').collect();
        if parts.len() != 3 || parts[1] != draft.token || draft.owner != component.user.id {
            return notice("This panel is no longer active for you. Run /setup again.");
        }
        let action = parts[2];
        if matches!(component.data.kind, ComponentInteractionDataKind::Button)
            && matches!(action, "next" | "back")
        {
            draft.logging_page = action == "next";
            return CreateInteractionResponse::UpdateMessage(panel(draft));
        }
        let mut candidate = draft.config.clone();
        match (action, &component.data.kind) {
            ("role", ComponentInteractionDataKind::RoleSelect { values })
                if (1..=25).contains(&values.len()) =>
            {
                candidate.admin_role_ids = values.iter().map(|id| id.get() as i64).collect();
                candidate.admin_role_ids.sort_unstable();
                candidate.admin_role_ids.dedup();
            }
            ("channel", ComponentInteractionDataKind::ChannelSelect { values })
                if (1..=25).contains(&values.len()) =>
            {
                candidate.channel_ids = values.iter().map(|id| id.get() as i64).collect();
                candidate.channel_ids.sort_unstable();
                candidate.channel_ids.dedup();
            }
            ("logs", ComponentInteractionDataKind::ChannelSelect { values })
                if values.len() == 1 =>
            {
                candidate.log_channel_id = Some(values[0].get() as i64)
            }
            ("level", ComponentInteractionDataKind::StringSelect { values })
                if values.len() == 1 =>
            {
                let Some(level) = crate::logging::Level::parse(&values[0]) else {
                    return notice("Select a valid logging level.");
                };
                candidate.log_level = level;
            }
            ("retention", ComponentInteractionDataKind::StringSelect { values })
                if values.len() == 1 =>
            {
                let Some(policy) = crate::retention::Policy::parse(&values[0]) else {
                    return notice("Select a valid retention policy.");
                };
                candidate.retention = policy;
            }
            ("cancel", ComponentInteractionDataKind::Button) => {
                sessions.0.remove(&guild);
                return CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content("Setup cancelled. Saved settings were not changed.")
                        .embeds(vec![])
                        .components(vec![]),
                );
            }
            ("save", ComponentInteractionDataKind::Button) => {
                if !complete(&candidate) {
                    return notice(
                        "Select a manager role, bot channel, and log channel before saving.",
                    );
                }
            }
            _ => return notice("Invalid selection. Run /setup again."),
        }
        let mut selection = candidate.clone();
        if action != "save" {
            // Validate just the edited field so deleted settings can be repaired one at a time.
            if action != "role" {
                selection.admin_role_ids.clear();
            }
            if action != "channel" {
                selection.channel_ids.clear();
            }
            if action != "logs" {
                selection.log_channel_id = None;
            }
        }
        if let Err(error) = validate(ctx, guild, &selection) {
            return notice(error);
        }
        if action == "save" {
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let mut db = self.database.lock().map_err(|_| "Database unavailable")?;
                let tx = db.transaction()?;
                if get_guild_config(&tx, guild.get() as i64)? != draft.original {
                    return Err(
                        "Settings changed since this panel was opened. Run /setup again.".into(),
                    );
                }
                let _storage = self
                    .storage_lock
                    .lock()
                    .map_err(|_| "Storage unavailable")?;
                crate::storage::register(&self.storage_root, guild.get())?;
                save_config(&tx, &candidate)?;
                tx.commit()?;
                if let Err(error) = crate::retention::prune(
                    &self.storage_root,
                    guild.get(),
                    candidate.retention,
                    crate::retention::now(),
                ) {
                    eprintln!("Retention cleanup failed for guild {guild}: {error}");
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("Failed to save setup: {error}");
                return notice(
                    "Could not save settings. Run /setup again and retry; existing settings were preserved.",
                );
            }
            sessions.0.remove(&guild);
            return CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::new()
                .content(format!("Settings saved. Managers: {} · Bot channels: {} · Log channel: <#{}>. Guild storage: 50 Mo. Retention: {}.", mentions(&candidate.admin_role_ids, "@&"), mentions(&candidate.channel_ids, "#"), candidate.log_channel_id.unwrap(), candidate.retention.label()))
                .allowed_mentions(CreateAllowedMentions::new().all_users(false).all_roles(false).everyone(false)).embeds(vec![]).components(vec![]));
        }
        draft.config = candidate;
        CreateInteractionResponse::UpdateMessage(panel(draft))
    }
}

fn save_config(db: &rusqlite::Connection, config: &GuildConfig) -> rusqlite::Result<()> {
    if !complete(config) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    // A savepoint makes list replacement atomic even when called outside setup's transaction.
    db.execute_batch("SAVEPOINT save_configuration")?;
    let result = (|| -> rusqlite::Result<()> {
        db.execute(
            "INSERT INTO guild_configs (guild_id, setup_completed, log_channel_id, log_level, retention)
            VALUES (?1, 1, ?2, ?3, ?4) ON CONFLICT(guild_id) DO UPDATE SET
            setup_completed = 1, log_channel_id = excluded.log_channel_id, log_level = excluded.log_level, retention = excluded.retention",
            rusqlite::params![config.guild_id, config.log_channel_id, config.log_level.as_str(), config.retention.key()],
        )?;
        db.execute(
            "DELETE FROM guild_manager_roles WHERE guild_id = ?1",
            [config.guild_id],
        )?;
        db.execute(
            "DELETE FROM guild_bot_channels WHERE guild_id = ?1",
            [config.guild_id],
        )?;
        for role in &config.admin_role_ids {
            db.execute(
                "INSERT INTO guild_manager_roles VALUES (?1, ?2)",
                [config.guild_id, *role],
            )?;
        }
        for channel in &config.channel_ids {
            db.execute(
                "INSERT INTO guild_bot_channels VALUES (?1, ?2)",
                [config.guild_id, *channel],
            )?;
        }
        Ok(())
    })();
    if result.is_err() {
        db.execute_batch("ROLLBACK TO save_configuration")?;
    }
    db.execute_batch("RELEASE save_configuration")?;
    result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup_pages_fit_discord_limits_and_expose_retention_before_saving() {
        let mut draft = Draft {
            owner: UserId::new(1),
            token: "test".into(),
            started: Instant::now(),
            original: None,
            logging_page: false,
            config: GuildConfig {
                guild_id: 1,
                setup_completed: false,
                admin_role_ids: vec![2],
                channel_ids: vec![3],
                log_channel_id: Some(4),
                log_level: crate::logging::Level::Info,
                retention: crate::retention::Policy::None,
            },
        };
        let first = serde_json::to_value(panel(&draft)).unwrap();
        assert!(first["components"].as_array().unwrap().len() <= 5);
        assert!(!first.to_string().contains("test:save"));
        draft.logging_page = true;
        let second = serde_json::to_value(panel(&draft)).unwrap();
        assert!(second["components"].as_array().unwrap().len() <= 5);
        assert!(second.to_string().contains("test:retention"));
        assert!(second.to_string().contains("test:save"));
        assert!(second.to_string().contains("flagged:30"));
        assert!(second.to_string().contains("all:7"));
    }

    #[test]
    fn setup_requires_manage_server_even_if_command_visibility_is_overridden() {
        assert!(!can_setup(None));
        for (permissions, allowed) in [
            (Permissions::empty(), false),
            (
                Permissions::MANAGE_MESSAGES | Permissions::MANAGE_ROLES,
                false,
            ),
            (Permissions::MANAGE_GUILD, true),
            (Permissions::ADMINISTRATOR, true),
        ] {
            let mut member = Member::default();
            member.permissions = Some(permissions);
            assert_eq!(can_setup(Some(&member)), allowed);
        }
    }

    #[test]
    fn migrates_legacy_selections_once() {
        let path = std::env::temp_dir().join(format!(
            "clause-migration-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE guild_configs (guild_id INTEGER PRIMARY KEY,
                setup_completed INTEGER NOT NULL DEFAULT 0, log_channel_id INTEGER,
                channel_to_manage INTEGER, admin_role_id INTEGER);
                INSERT INTO guild_configs VALUES (1, 1, 4, 3, 2);",
            )
            .unwrap();
        }
        {
            let db = crate::early_init(path.to_str().unwrap()).unwrap();
            let mut config = get_guild_config(&db, 1).unwrap().unwrap();
            assert_eq!(config.admin_role_ids, vec![2]);
            assert_eq!(config.channel_ids, vec![3]);
            assert_eq!(config.log_channel_id, Some(4));
            assert_eq!(config.log_level, crate::logging::Level::Info);
            assert_eq!(config.retention, crate::retention::Policy::None);
            config.retention = crate::retention::Policy::All(7);
            config.log_level = crate::logging::Level::Debug;
            config.admin_role_ids = vec![7, 8];
            config.channel_ids = vec![9, 10];
            save_config(&db, &config).unwrap();
        }
        {
            let db = crate::early_init(path.to_str().unwrap()).unwrap();
            let config = get_guild_config(&db, 1).unwrap().unwrap();
            assert_eq!(config.admin_role_ids, vec![7, 8]);
            assert_eq!(config.channel_ids, vec![9, 10]);
            assert_eq!(config.log_level, crate::logging::Level::Debug);
            assert_eq!(config.retention, crate::retention::Policy::All(7));
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn saves_complete_configuration_and_preserves_other_guilds() {
        let db = crate::early_init(":memory:").unwrap();
        let mut config = GuildConfig {
            guild_id: 1,
            setup_completed: false,
            admin_role_ids: vec![2, 7],
            channel_ids: vec![3, 8],
            log_channel_id: Some(4),
            log_level: crate::logging::Level::Info,
            retention: crate::retention::Policy::None,
        };
        save_config(&db, &config).unwrap();
        config.setup_completed = true;
        assert_eq!(get_guild_config(&db, 1).unwrap(), Some(config.clone()));
        config.guild_id = 5;
        save_config(&db, &config).unwrap();
        config.guild_id = 1;
        config.log_channel_id = Some(6);
        save_config(&db, &config).unwrap();
        assert_eq!(get_guild_config(&db, 1).unwrap(), Some(config.clone()));
        assert_eq!(
            get_guild_config(&db, 5).unwrap().unwrap().log_channel_id,
            Some(4)
        );
        let saved = config.clone();
        config.admin_role_ids = vec![9, 9];
        assert!(save_config(&db, &config).is_err());
        assert_eq!(get_guild_config(&db, 1).unwrap(), Some(saved));
        config.admin_role_ids = vec![7];
        config.channel_ids = vec![8];
        save_config(&db, &config).unwrap();
        assert_eq!(get_guild_config(&db, 1).unwrap(), Some(config.clone()));
        config.admin_role_ids.clear();
        assert!(save_config(&db, &config).is_err());
        assert_eq!(
            get_guild_config(&db, 1).unwrap().unwrap().admin_role_ids,
            vec![7]
        );
    }
}
