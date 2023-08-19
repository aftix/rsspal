use std::env;

use serenity::async_trait;
use serenity::framework::standard::macros::{command, group};
use serenity::framework::standard::CommandResult;
use serenity::framework::StandardFramework;
use serenity::model::channel::Message;
use serenity::prelude::*;

struct Handler;

#[async_trait]
impl EventHandler for Handler {}

#[group("general")]
#[commands(ping)]
struct General;

#[tokio::main]
async fn main() {
    let framework = StandardFramework::new()
        .configure(|c| c.allow_dm(false))
        .group(&GENERAL_GROUP);
    let token = env::var("DISCORD_TOKEN").expect("need DISCORD_TOKEN environment variable set");
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("error creating client");

    if let Err(err) = client.start().await {
        eprintln!("Error while running client: {:?}", err);
    }
}

#[command]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .send_message(&ctx.http, |create| create.content("Pong!"))
        .await?;
    Ok(())
}
