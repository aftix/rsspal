use serenity::framework::StandardFramework;
use serenity::prelude::*;

mod admin_commands;
mod config;
mod db;
mod feed;
mod signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    signal::mask_signals().map_err(|e| anyhow::anyhow!("SIG_UNBLOCK sigprocmask errno: {}", e))?;

    let config =
        config::Config::new().map_err(|e| anyhow::anyhow!("error reading config: {}", e))?;

    let db_path = config.data_dir.join("data.db");
    let pool = db::db_pool(db_path.to_string_lossy()).await?;
    db::setup_tables(&pool).await?;

    let framework = StandardFramework::new()
        .configure(|c| c.allow_dm(false))
        .group(&admin_commands::ADMIN_GROUP);
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(admin_commands::Handler)
        .framework(framework)
        .await?;

    client
        .start()
        .await
        .map_err(|e| anyhow::anyhow!("error running client: {}", e))
}
