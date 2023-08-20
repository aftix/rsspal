use serenity::framework::StandardFramework;
use serenity::prelude::*;

mod admin_commands;
mod config;
mod feed;
mod signal;

#[tokio::main]
async fn main() {
    use crate::signal;
    if let Err(errno) = signal::mask_signals() {
        eprintln!("SIG_UNNBLOCK sigprocmask errno: {}", errno);
    }

    let config = match config::Config::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading config: {}", e);
            std::process::exit(1);
        }
    };

    let framework = StandardFramework::new()
        .configure(|c| c.allow_dm(false))
        .group(&admin_commands::ADMIN_GROUP);
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(admin_commands::Handler)
        .framework(framework)
        .await
        .expect("error creating client");

    if let Err(err) = client.start().await {
        eprintln!("Error while running client: {:?}", err);
    }
}
