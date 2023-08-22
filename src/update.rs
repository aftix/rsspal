use std::sync::{Arc, OnceLock};

use log::{debug, error, info};

use serenity::prelude::*;

use tokio::fs::{create_dir_all, File};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Barrier};
use tokio::time::{sleep_until, Duration, Instant};

use crate::feed::{self, Feed};
use crate::CONFIG;

pub static COMMANDS: OnceLock<mpsc::Sender<(Command, Arc<Barrier>)>> = OnceLock::new();

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub enum Command {
    AddFeed(Feed),
    Exit,
}

// Run in background, configuring server and updating feeds, etc
pub async fn background_task(mut feeds: Vec<Feed>, ctx: Context) -> anyhow::Result<()> {
    let (sender, mut commands) = mpsc::channel(8);
    COMMANDS
        .set(sender)
        .map_err(|_| anyhow::anyhow!("error setting COMMANDS"))?;

    // Default to 10 minutes
    let interval = CONFIG.get().map(|cfg| cfg.interval).unwrap_or(600);
    let interval = Duration::from_secs(interval);
    let mut to_sleep = Instant::now()
        .checked_add(interval)
        .expect("couldn't add interval to instant");

    debug!("Starting background loop");
    'L: loop {
        let recv = commands.recv();
        let timer = sleep_until(to_sleep);
        tokio::select! {
            cmdwait = recv => {
                let (command, wait) = cmdwait
                    .ok_or_else(|| anyhow::anyhow!("failed recieveing on channel"))?;
                match command {
                    Command::AddFeed(feed) => feeds.push(feed),
                    Command::Exit => {
                        if let Err(e) = exit_feeds_loop(feeds).await {
                            error!("Error exiting background_task: {}", e);
                        }
                        wait.wait().await;
                        break 'L;
                    }
                }
                wait.wait().await;

            },
            _ = timer => {
                update_feeds(&mut feeds, &ctx).await;
                to_sleep = Instant::now().checked_add(interval).expect("couldn't add interval to instant");
            },
        }
    }

    Ok(())
}

async fn update_feeds(feeds: &mut [Feed], ctx: &Context) {
    info!("Updating feeds")
}

async fn exit_feeds_loop(feeds: Vec<Feed>) -> anyhow::Result<()> {
    debug!("Exiting the background loop");
    let path = {
        let config = CONFIG
            .get()
            .ok_or_else(|| anyhow::anyhow!("could not get CONFIG"))?;

        config.save()?;

        config.data_dir.clone()
    };

    feed::export(&path, &feeds)
        .await
        .map_err(|e| anyhow::anyhow!("could not save feeds data: {}", e))
}
