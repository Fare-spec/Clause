mod setup;
mod storage;

use std::{
    env,
    error::Error,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, params};

use serenity::{
    all::{Command, CreateCommand, GatewayIntents, Interaction, Message, Permissions, Ready},
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
    pub channel_ids: Vec<i64>,
    pub admin_role_ids: Vec<i64>,
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
        INSERT OR IGNORE INTO guild_manager_roles SELECT guild_id, admin_role_id
            FROM guild_configs WHERE admin_role_id IS NOT NULL;
        INSERT OR IGNORE INTO guild_bot_channels SELECT guild_id, channel_to_manage
            FROM guild_configs WHERE channel_to_manage IS NOT NULL;
        UPDATE guild_configs SET admin_role_id = NULL, channel_to_manage = NULL;
        COMMIT;",
    )?;
    Ok(conn)
}

/// Reads a guild config from SQLite.
fn get_guild_config(conn: &Connection, guild_id: i64) -> rusqlite::Result<Option<GuildConfig>> {
    let mut statement = conn.prepare(&format!(
        "
            SELECT
                guild_id,
                setup_completed,
                log_channel_id
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

// ============================================================
// DISCORD HANDLER
// ============================================================

struct Handler {
    database: Arc<Mutex<Connection>>,
    setup: Mutex<setup::Sessions>,
    storage_root: std::path::PathBuf,
}

#[async_trait]
impl EventHandler for Handler {
    // --------------------------------------------------------
    // BOT READY
    // --------------------------------------------------------

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("Connected as {}", ready.user.name);

        let setup_command = CreateCommand::new("setup")
            .description("Setup the bot for this server.")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .dm_permission(false);

        match Command::create_global_command(&ctx.http, setup_command).await {
            Ok(_) => {
                println!("Registered /setup command");
            }

            Err(error) => {
                eprintln!("Failed to register /setup: {error}");
            }
        }
    }

    // --------------------------------------------------------
    // SLASH COMMANDS
    // --------------------------------------------------------

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let result = match interaction {
            Interaction::Command(command) if command.data.name == "setup" => {
                let response = self.setup_response(&ctx, &command);
                command.create_response(&ctx.http, response).await
            }
            Interaction::Component(component) if component.data.custom_id.starts_with("setup:") => {
                let response = self.setup_component(&ctx, &component);
                component.create_response(&ctx.http, response).await
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
        // Ignore bots.
        if message.author.bot {
            return;
        }

        // Ignore DMs.
        if message.guild_id.is_none() {
            return;
        }

        let Some(permissions) = message.author_permissions(&ctx) else {
            return;
        };

        let config = {
            let Ok(database) = self.database.lock() else {
                return;
            };
            match get_guild_config(&database, message.guild_id.unwrap().get() as i64) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("Failed to read configuration: {error}");
                    return;
                }
            }
        };
        let manager = config.as_ref().is_some_and(|config| {
            message.member.as_ref().is_some_and(|member| {
                member
                    .roles
                    .iter()
                    .any(|id| config.admin_role_ids.contains(&(id.get() as i64)))
            })
        });
        if !(permissions.administrator() || permissions.manage_guild() || manager) {
            return;
        }
        if let Some(config) = &config {
            if config.setup_completed
                && !config
                    .channel_ids
                    .contains(&(message.channel_id.get() as i64))
            {
                return;
            }
        }

        match message.content.as_str() {
            "!ping" => {
                if let Err(error) = message.channel_id.say(&ctx.http, "Pong!").await {
                    eprintln!("Failed to send ping reply: {error}");
                }
            }

            "!config" => {
                let Some(guild_id) = message.guild_id else {
                    return;
                };

                let guild_id = guild_id.get() as i64;

                let config = {
                    let database = match self.database.lock() {
                        Ok(database) => database,

                        Err(error) => {
                            eprintln!("Failed to lock database: {error}");

                            return;
                        }
                    };

                    get_guild_config(&database, guild_id)
                };

                match config {
                    Ok(Some(config)) => {
                        let content = format!(
                            "\
Guild ID: {}
Setup completed: {}
Log channel: {:?}
Bot channels: {:?}
Manager roles: {:?}",
                            config.guild_id,
                            config.setup_completed,
                            config.log_channel_id,
                            config.channel_ids,
                            config.admin_role_ids,
                        );

                        if let Err(error) = message.channel_id.say(&ctx.http, content).await {
                            eprintln!("Failed to send config: {error}");
                        }
                    }

                    Ok(None) => {
                        if let Err(error) = message
                            .channel_id
                            .say(
                                &ctx.http,
                                "This server has not been configured. Use /setup.",
                            )
                            .await
                        {
                            eprintln!("Failed to send reply: {error}");
                        }
                    }

                    Err(error) => {
                        eprintln!("Failed to read config: {error}");
                    }
                }
            }

            _ => {}
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

    let handler = Handler {
        database,
        setup: Mutex::new(setup::Sessions::default()),
        storage_root: env::var_os("GUILD_STORAGE_PATH")
            .map(Into::into)
            .unwrap_or_else(|| "guilds".into()),
    };

    let mut client = Client::builder(token, intents)
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
