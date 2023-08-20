use tokio::task::spawn;

use serenity::async_trait;
use serenity::framework::standard::{
    macros::{command, group},
    CommandError, CommandResult,
};
use serenity::model::{channel::Message, gateway::Ready};
use serenity::prelude::*;

use crate::signal::{send_termination, wait_for_termination};

#[group]
#[commands(ping, exit)]
pub struct Admin;

pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is ready", ready.user.name);
        ctx.online().await;

        let exit = async move {
            println!("Recieved SIGINT, cleaning up bot.");
            ctx.invisible().await;
            println!("{} set to invisible", ready.user.name);
        };

        spawn(wait_for_termination(exit));
    }
}

#[command]
pub async fn exit(_ctx: &Context, _msg: &Message) -> CommandResult {
    send_termination().await.map_err(|e| CommandError::from(e))
}

#[command]
pub async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .send_message(&ctx.http, |create| create.content("Pong!"))
        .await?;
    Ok(())
}
