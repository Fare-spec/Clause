# Clause

Minimal Rust Discord bot using Serenity. Responds to `!ping` with `Pong!`.
Rule loading and LLM evaluation are not implemented yet; the message handler in
`src/main.rs` is the extension point.

## Discord setup

1. Create an application and bot in the [Discord Developer Portal](https://discord.com/developers/applications).
2. Under **Bot → Privileged Gateway Intents**, enable **Message Content Intent**.
3. Invite the bot to your server using the `bot` scope and the **View Channels**
   and **Send Messages** permissions.
4. Set the `DISCORD_TOKEN` environment variable to the bot token. Keep it out of
   source control.

## Run locally

With Rust installed and `DISCORD_TOKEN` set:

```sh
cargo run --locked
```

Send `!ping` in a server channel the bot can access.

## Run with Docker

With `DISCORD_TOKEN` exported in your shell:

```sh
docker build -t clause .
docker run --rm --name clause --env DISCORD_TOKEN clause
```

The container runs as a non-root user and requires no inbound ports. Stop it with
`docker stop clause`; the bot handles SIGTERM and Ctrl+C for graceful shutdown.

## Configure a server

Run `/setup` as a server administrator or a member with **Manage Server**.
The private Discord panel provides role/channel dropdowns and Save/Cancel buttons:

- **Manager roles:** select 1–25 roles. Members of any selected role can use `!ping` and `!config`.
  Administrators and members with Manage Server always retain access.
- **Bot channels:** select 1–25 text channels where these commands work after setup.
- **Log channel:** the text channel reserved for logs. The destination is saved;
  event logging and moderation are not implemented yet.

Select all three settings and click **Save settings**. Clause checks that it can
view and send messages in every selected channel. The single log channel may
also be one of the bot channels.
Settings are saved together in SQLite (`DATABASE_PATH`, default `bot.db`) and
survive restarts. Cancel leaves previously saved settings intact.

Run `/setup` again to edit existing settings. Only one administrator can configure
a server at a time; reopening your own setup replaces your previous panel. Panels
expire after 15 minutes or a bot restart. Only administrators and members with
Manage Server can change setup, including the manager roles.
Existing single-role/channel configurations migrate automatically on startup.


## Guild storage

Saving `/setup` creates `guilds/<guild-id>/`, using the stable guild ID as the
folder name. Set `GUILD_STORAGE_PATH` to change the root directory. Existing
files survive repeat setup and cancellation does not create a folder.

Each folder contains `storage-limit.json` recording a **50 MB (50,000,000 bytes)**
limit per guild, including metadata. Registration checks recursive file usage
and refuses storage above the limit. This is an application limit, not an OS
filesystem quota: there is no upload feature yet, and future file writers must
check the total against `STORAGE_LIMIT_BYTES` before writing. Setup does not
delete files to make room. If saving the database fails after folder creation,
the folder is retained for a retry.

For Docker, persist `/app/guilds` with a volume (for example,
`--mount type=volume,src=clause-guilds,dst=/app/guilds`). A custom storage path
must be writable by the container user (UID 10001).
