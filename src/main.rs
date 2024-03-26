use lazy_static::lazy_static;
use serenity::framework::StandardFramework;
use serenity::prelude::*;
use std::sync::RwLock;
use tracing_subscriber::{prelude::*, EnvFilter, Registry};

mod admin_commands;
mod config;
mod discord;
mod feed;
mod opml;
mod signal;
mod update;

lazy_static! {
    static ref CONFIG: RwLock<config::Config> =
        RwLock::new(config::Config::new().unwrap_or_else(|e| panic!("{}", e)));
}

// Read the configuration, parse variables, and start discord client
#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    signal::mask_signals().map_err(|e| anyhow::anyhow!("SIG_UNBLOCK sigprocmask errno: {}", e))?;

    let console_layer = console_subscriber::spawn();
    Registry::default()
        .with(
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_filter(EnvFilter::from_default_env()),
        )
        .with(tracing_journald::layer()?.with_filter(EnvFilter::from_default_env()))
        .with(console_layer)
        .init();

    let token = match CONFIG.read() {
        Err(e) => {
            anyhow::bail!("Failed to read CONFIG: {}", e);
        }
        Ok(cfg) => cfg.discord_token.clone(),
    };

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
