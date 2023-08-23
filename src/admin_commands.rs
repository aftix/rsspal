use std::sync::{Arc, OnceLock};

use log::{debug, error, info, warn};

use tokio::sync::Barrier;
use tokio::task::spawn;

use serenity::async_trait;
use serenity::framework::standard::{
    macros::{command, group},
    CommandError, CommandResult,
};
use serenity::model::id::GuildId;
use serenity::model::{gateway::Ready, prelude::*};
use serenity::prelude::*;

use crate::discord;
use crate::feed;
use crate::signal::{send_termination, wait_for_termination};
use crate::update::{background_task, Command, COMMANDS};

pub static GUILDS: OnceLock<Vec<GuildId>> = OnceLock::new();
pub static USER_ID: OnceLock<UserId> = OnceLock::new();

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
        USER_ID
            .set(ready.user.id)
            .expect("failed to set current user id");

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

    // Handle marking items read/unread on reaction
    async fn reaction_add(&self, ctx: Context, reaction: Reaction) {
        debug!("Recieved reaction emote to message event.");
        let current_user = USER_ID.get().expect("failed to get USER_ID static").clone();

        let msg = match reaction.message(&ctx).await {
            Ok(msg) => msg,
            Err(e) => {
                error!(
                    "Error retrieving message {} for reaction: {}",
                    reaction.message_id, e
                );
                return;
            }
        };

        if current_user != msg.author.id {
            debug!("Reaction not on rsspal bot user message, ignoring.");
            return;
        }

        if msg.embeds.len() != 1 {
            debug!("Reaction not on a feed item, ignoring.");
            return;
        }

        let name = match msg.channel(&ctx).await {
            Ok(channel) => match channel.clone().guild() {
                Some(channel) => channel.name,
                None => {
                    error!(
                        "Could not convert channel {} to a guild channel.",
                        channel.id()
                    );
                    return;
                }
            },
            Err(e) => {
                error!("Could not get channel for message {}: {}", msg.id, e);
                return;
            }
        };
        let link = if let Some(link) = msg.embeds[0]
            .fields
            .iter()
            .find(|field| field.name == "link")
        {
            link.value.clone()
        } else {
            warn!(
                "Message {} appears to be a feed item with no link field, ignoring.",
                msg.id
            );
            return;
        };

        // Check for the emoji to mark a message read
        if reaction.emoji == '📖'.into() {
            if let Err(e) = discord::mark_read(msg, &ctx).await {
                error!("failed to mark item as read: {}", e);
            }

            match COMMANDS.get() {
                None => error!("could not get COMMANDS static"),
                Some(send) => {
                    let barrier = Arc::new(Barrier::new(2));
                    if let Err(e) = send
                        .send((Command::MarkRead(name, link), barrier.clone()))
                        .await
                    {
                        error!("Could not send MarkRead command on commands channel: {}", e);
                    } else {
                        barrier.wait().await;
                    }
                }
            };
        } else if reaction.emoji == '📕'.into() {
            if let Err(e) = discord::mark_unread(msg, &ctx).await {
                error!("failed to mark item as read: {}", e);
            }

            match COMMANDS.get() {
                None => error!("could not get COMMANDS static"),
                Some(send) => {
                    let barrier = Arc::new(Barrier::new(2));
                    if let Err(e) = send
                        .send((Command::MarkUnread(name, link), barrier.clone()))
                        .await
                    {
                        error!(
                            "Could not send MarkUnread command on commands channel: {}",
                            e
                        );
                    } else {
                        barrier.wait().await;
                    }
                }
            };
        }
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
