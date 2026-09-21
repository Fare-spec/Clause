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
