use serenity::framework::StandardFramework;
use serenity::prelude::*;
use std::sync::OnceLock;

mod admin_commands;
mod config;
mod discord;
mod feed;
mod signal;
mod update;

static CONFIG: OnceLock<config::Config> = OnceLock::new();

// Read the configuration, parse variables, and start discord client
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    signal::mask_signals().map_err(|e| anyhow::anyhow!("SIG_UNBLOCK sigprocmask errno: {}", e))?;

    let config =
        config::Config::new().map_err(|e| anyhow::anyhow!("error reading config: {}", e))?;
    let token = config.discord_token.clone();
    CONFIG.get_or_init(move || config);

    let framework = StandardFramework::new()
        .configure(|c| c.allow_dm(false))
        .group(&admin_commands::ADMIN_GROUP);
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents)
        .event_handler(admin_commands::Handler)
        .framework(framework)
        .await?;

    client
        .start()
        .await
        .map_err(|e| anyhow::anyhow!("error running client: {}", e))
}
