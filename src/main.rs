use std::env;

use serenity::framework::StandardFramework;
use serenity::prelude::*;

mod admin_commands;
mod feed;
mod signal;

#[tokio::main]
async fn main() {
    use crate::signal;
    if let Err(errno) = signal::mask_signals() {
        eprintln!("SIG_UNNBLOCK sigprocmask errno: {}", errno);
    }

    let feed = feed::RssFeedItem::from_url(
        "https://www.rssboard.org/files/sample-rss-2.xml",
        Option::<&str>::None,
        Option::<&str>::None,
    );
    println!("{:?}", feed);

    let framework = StandardFramework::new()
        .configure(|c| c.allow_dm(false))
        .group(&admin_commands::ADMIN_GROUP);
    let token = env::var("DISCORD_TOKEN").expect("need DISCORD_TOKEN environment variable set");
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents)
        .event_handler(admin_commands::Handler)
        .framework(framework)
        .await
        .expect("error creating client");

    if let Err(err) = client.start().await {
        eprintln!("Error while running client: {:?}", err);
    }
}
