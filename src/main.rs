use std::{env, error::Error};

use serenity::{
    async_trait,
    model::{channel::Message, gateway::Ready},
    prelude::*,
};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        println!("Connected as {}", ready.user.name);
    }

    async fn message(&self, ctx: Context, message: Message) {
        if message.author.bot {
            return;
        }

        if message.content == "!ping" {
            if let Err(error) = message.channel_id.say(&ctx.http, "Pong!").await {
                eprintln!("Failed to send reply: {error}");
            }
        }
    }
}

async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }

    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let token =
        env::var("DISCORD_TOKEN").map_err(|_| "Set DISCORD_TOKEN to your Discord bot token")?;

    let intents =
        GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(token, intents)
        .event_handler(Handler)
        .await?;
    let shard_manager = client.shard_manager.clone();

    tokio::select! {
        result = client.start() => result?,
        result = shutdown_signal() => {
            result?;
            shard_manager.shutdown_all().await;
        }
    }

    Ok(())
}
