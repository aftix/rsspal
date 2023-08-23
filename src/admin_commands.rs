use std::sync::{Arc, OnceLock};

use log::{debug, error, info};

use tokio::sync::Barrier;
use tokio::task::spawn;

use serenity::async_trait;
use serenity::framework::standard::{
    macros::{command, group},
    CommandError, CommandResult,
};
use serenity::model::id::GuildId;
use serenity::model::{channel::Message, gateway::Ready};
use serenity::prelude::*;

use crate::discord;
use crate::feed;
use crate::signal::{send_termination, wait_for_termination};
use crate::update::{background_task, Command, COMMANDS};

pub static GUILDS: OnceLock<Vec<GuildId>> = OnceLock::new();

#[group]
#[commands(ping, exit)]
pub struct Admin;

pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    // Real main function since everything needs access to the context
    async fn ready(&self, ctx: Context, ready: Ready) {
        debug!("serenity discord client is ready");
        let ids: Vec<_> = ready.guilds.iter().map(|guild| guild.id).collect();
        GUILDS
            .set(ids)
            .expect("failed to set guild ids static variable");

        // Get the stored database
        let feeds = feed::import().await.expect("Failed to import feeds.");

        discord::setup_channels(&feeds, &ctx).await;

        debug!("spawning background task");
        spawn(background_task(feeds, ctx.clone()));

        ctx.online().await;
        info!("{} is ready.", ready.user.name);

        let exit = async move {
            info!("Recieved exit signal or command, cleaning up bot.");
            ctx.invisible().await;
            debug!("{} set to be invisible", ready.user.name);
            let barrier = Arc::new(Barrier::new(2));
            if let Some(s) = COMMANDS.get().cloned() {
                if let Err(e) = s.send((Command::Exit, barrier.clone())).await {
                    error!("Error sending exit command on channel: {}", e);
                } else {
                    barrier.wait().await;
                }
            }
            info!("Background thread finished exiting.");
        };

        debug!("Spawning task to wait for bot termination.");
        spawn(wait_for_termination(exit));
    }
}

#[command]
pub async fn exit(_ctx: &Context, _msg: &Message) -> CommandResult {
    info!("Recieved exit command.");
    send_termination().await.map_err(|e| CommandError::from(e))
}

#[command]
pub async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    info!("Recieved ping command.");
    msg.channel_id
        .send_message(&ctx.http, |create| create.content("Pong!"))
        .await?;
    Ok(())
}
