use std::sync::Arc;

use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::Barrier;
use tokio::task::spawn;

use serenity::async_trait;
use serenity::framework::standard::{
    macros::{command, group},
    CommandError, CommandResult,
};
use serenity::model::{channel::Message, gateway::Ready};
use serenity::prelude::*;

use crate::feed::Feed;
use crate::signal::{send_termination, wait_for_termination};
use crate::update::{background_task, Command, COMMANDS};
use crate::CONFIG;

#[group]
#[commands(ping, exit)]
pub struct Admin;

pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    // Real main function since everything needs access to the context
    async fn ready(&self, ctx: Context, ready: Ready) {
        // Get the stored configuration
        let config = CONFIG.get().expect("couldn't read config global");

        // Read data into memory
        let mut database = File::open(config.data_dir.join("database.toml"))
            .await
            .expect(&format!(
                "could not open database {:?}",
                config.data_dir.join("database.toml")
            ));

        let mut buf = Vec::new();
        if let Ok(meta) = database.metadata().await {
            buf.reserve(meta.len() as usize);
        }
        database
            .read_to_end(&mut buf)
            .await
            .expect("could not read the database file");
        let feeds: Vec<Feed> = toml::from_str(&String::from_utf8_lossy(buf.as_slice()))
            .expect("could not parse database toml");

        spawn(background_task(feeds, ctx.clone()));

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
    let barrier = Arc::new(Barrier::new(2));
    if let Some(s) = COMMANDS.get().cloned() {
        if let Err(e) = s.send((Command::Exit, barrier.clone())).await {
            eprintln!("error sending exit command: {:?}", e);
        } else {
            barrier.wait().await;
        }
    }
    send_termination().await.map_err(|e| CommandError::from(e))
}

#[command]
pub async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .send_message(&ctx.http, |create| create.content("Pong!"))
        .await?;
    Ok(())
}
