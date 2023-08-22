use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use log::{debug, error, info};

use serenity::prelude::*;

use tokio::sync::{mpsc, Barrier};
use tokio::task::JoinSet;
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

async fn diff_feed(update: Feed, feeds: &mut [Feed], ctx: &Context) {
    let feed = feeds.iter_mut().find(|f| f.url() == update.url());
    if let Some(feed) = feed {
        info!("Updating feed {}.", feed.title());
        match (update, feed) {
            (Feed::RSS(update), Feed::RSS(ref mut rss)) => {
                debug!("Feed {} is RSS.", rss.channel.title);
                debug!("Updating feed {} items.", rss.channel.title);
                let mut set = HashSet::with_capacity(rss.channel.item.len());
                set.extend(rss.channel.item.iter().map(|i| i.link.clone()));
                for item in update.channel.item {
                    if !set.contains(&item.link) {
                        info!("Feed {} new item: {:?}.", rss.channel.title, item.title);
                        rss.channel.item.push(item);
                    }
                }

                debug!("Updating feed {} metadata.", rss.channel.title);
                rss.channel.description = update.channel.description;
                rss.channel.copyright = update.channel.copyright;
                rss.channel.managing_editor = update.channel.managing_editor;
                rss.channel.web_master = update.channel.web_master;
                rss.channel.pub_date = update.channel.pub_date;
                rss.channel.category = update.channel.category;
                rss.channel.docs = update.channel.docs;
                rss.channel.ttl = update.channel.ttl;
                rss.channel.image = update.channel.image;
                rss.channel.skip_hours = update.channel.skip_hours;
                rss.channel.skip_days = update.channel.skip_days;
                rss.channel.last_updated = Some(chrono::offset::Utc::now());
            }
            (Feed::ATOM(update), Feed::ATOM(ref mut atom)) => {
                debug!("Feed {} is atom.", atom.title);
                debug!("Updating feed {} items.", atom.title);
                let mut set = HashSet::with_capacity(atom.entry.len());
                set.extend(atom.entry.iter().map(|e| e.id.clone()));
                for entry in update.entry {
                    if !set.contains(&entry.id) {
                        info!("Feed {} hew item: {}.", atom.title, entry.title);
                        atom.entry.push(entry);
                    }
                }

                debug!("Updating feed {} metadata", atom.title);
                atom.id = update.id;
                atom.updated = update.updated;
                atom.author = update.author;
                atom.link = update.link;
                atom.category = update.category;
                atom.icon = update.icon;
                atom.logo = update.logo;
                atom.rights = update.rights;
                atom.subtitle = update.subtitle;
                atom.ttl = update.ttl;
                atom.skip_days = update.skip_days;
                atom.skip_hours = update.skip_hours;
                atom.last_updated = Some(chrono::offset::Utc::now());
            }
            _ => error!("Mismatched feed type between update and current feed",),
        }
    }
}

async fn update_feeds(feeds: &mut Vec<Feed>, ctx: &Context) {
    info!("Updating feeds");
    let mut futures = JoinSet::new();
    for feed in feeds.iter_mut() {
        if feed.should_update() {
            let url = feed.url();
            futures.spawn(async {
                info!("Updating feed at {}.", url);
                feed::from_url(url, None, None)
            });
        }
    }

    while let Some(res) = futures.join_next().await {
        match res {
            Err(e) => error!("Error joining update feed task: {}", e),
            Ok(Err(e)) => error!("Error updating feed: {}", e),
            Ok(Ok(update)) => diff_feed(update, feeds, ctx).await,
        };
    }

    if let Err(e) = feed::export(feeds).await {
        error!("Error writing feeds to file: {}", e);
    }
}

async fn exit_feeds_loop(feeds: Vec<Feed>) -> anyhow::Result<()> {
    debug!("Exiting the background loop");
    {
        let config = CONFIG
            .get()
            .ok_or_else(|| anyhow::anyhow!("could not get CONFIG"))?;
        config.save()?;
    }

    feed::export(&feeds)
        .await
        .map_err(|e| anyhow::anyhow!("could not save feeds data: {}", e))
}
