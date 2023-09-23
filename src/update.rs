use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use log::{debug, error, info, warn};

use serenity::model::prelude::*;
use serenity::prelude::*;

use tokio::runtime::Handle;
use tokio::sync::{mpsc, Barrier};
use tokio::task::JoinSet;
use tokio::time::{sleep_until, Duration, Instant};

use quick_xml::{de, se};

use crate::discord;
use crate::feed::{self, Feed};
use crate::opml::Opml;
use crate::CONFIG;

pub static COMMANDS: OnceLock<mpsc::Sender<(Command, Arc<Barrier>)>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum Command {
    AddFeed(Box<Feed>),
    EditFeed(Message, String, EditArgs),
    RemoveFeed(Message, String),
    ReloadFeed(Message, Option<String>),
    MarkRead(String, String),   // Channel name, item url
    MarkUnread(String, String), // Channel name, item url
    Export(Message, Option<String>),
    Import(Message),
    Exit,
}

#[derive(Debug, Clone, Default, Hash, PartialEq, Eq)]
pub struct EditArgs {
    pub category: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
}

// Run in background, configuring server and updating feeds, etc
pub async fn background_task(feeds: Vec<Feed>, ctx: Context) -> anyhow::Result<()> {
    let feeds = Arc::new(RwLock::new(feeds));
    let (sender, mut commands) = mpsc::channel(8);
    COMMANDS
        .set(sender)
        .map_err(|_| anyhow::anyhow!("error setting COMMANDS"))?;

    let interval = match CONFIG.read() {
        Err(_) => 600, // default to 10 minutes
        Ok(cfg) => cfg.interval,
    };
    let interval = Duration::from_secs(interval);
    let mut to_sleep = Instant::now()
        .checked_add(interval)
        .expect("couldn't add interval to instant");

    let (spawned_sender, mut spawned_channel) = mpsc::channel(8);

    debug!("Starting background loop");
    'L: loop {
        let recv = commands.recv();
        let timer = sleep_until(to_sleep);

        tokio::select! {
            cmdwait = recv => {
                match cmdwait {
                    None => anyhow::bail!("failed recieving on channel"),
                    Some((cmd, wait)) => {
                        let spawned_sender = spawned_sender.clone();
                        let feeds = feeds.clone();
                        let ctx = ctx.clone();
                        Handle::current().spawn(async move {
                            let processed = process_command(cmd, &ctx, feeds).await;
                            wait.wait().await;
                            if let Err(e) = spawned_sender.send(processed).await {
                                error!("Failed to send processed command on channel: {}", e);
                            }
                        });
                    },
                }
            },
            processed = spawned_channel.recv() => if let Some(Some(())) = processed {
                if let Err(e) = exit_feeds_loop(feeds.read().await.as_ref()).await {
                    error!("Error exiting background_task: {}", e);
                }
                break 'L;
            },
            _ = timer => {
                update_feeds(feeds.write().await.as_mut(), false, &ctx).await;
                to_sleep = Instant::now().checked_add(interval).expect("couldn't add interval to instant");
            },
        }
    }
    Ok(())
}

async fn remove_feed(idx: usize, feeds: impl AsRef<RwLock<Vec<Feed>>>) {
    let mut guard = feeds.as_ref().write().await;
    let feeds: &mut Vec<Feed> = guard.as_mut();
    feeds.remove(idx);
}

async fn add_feeds(new: Vec<Feed>, feeds: impl AsRef<RwLock<Vec<Feed>>>, ctx: &Context) {
    discord::setup_channels(&new, ctx).await;

    let mut guard = feeds.as_ref().write().await;
    let feeds: &mut Vec<Feed> = guard.as_mut();

    let start = feeds.len();
    feeds.extend(new.into_iter());

    for feed in &feeds[start..] {
        info!("Adding entries for feed {}", feed.title());
        match &feed {
            Feed::Rss(rss) => {
                for item in &rss.channel.item {
                    let publish = discord::publish_rss_item(&feed.title(), item, ctx).await;
                    if let Err(e) = publish {
                        warn!("failed to publish rss item to feed: {}", e);
                    }
                }
            }
            Feed::Atom(atom) => {
                for entry in &atom.entry {
                    let publish = discord::publish_atom_entry(&feed.title(), entry, ctx).await;
                    if let Err(e) = publish {
                        warn!("failed to publish atom item to feed: {}", e);
                    }
                }
            }
        }
    }
}

async fn process_command(cmd: Command, ctx: &Context, feeds: Arc<RwLock<Vec<Feed>>>) -> Option<()> {
    match cmd {
        Command::AddFeed(feed) => {
            info!("Adding feed {}.", feed.title());
            if feeds.read().await.iter().any(|f| feed.url() == f.url()) {
                info!("Feed {}, already exists.", feed.title());
                return None;
            }
            let push = vec![*feed];

            add_feeds(push, feeds, ctx).await;
            return None;
        }
        Command::EditFeed(msg, id, args) => {
            info!("Editing feed {}.", id);
            let channel_name = discord::title_to_channel_name(&id);
            let location = feeds
                .read()
                .await
                .iter()
                .enumerate()
                .find_map(|(idx, feed)| {
                    if discord::title_to_channel_name(feed.title()) == channel_name
                        || feed.url() == id
                    {
                        Some(idx)
                    } else {
                        None
                    }
                });

            if location.is_none() {
                if let Err(e) = msg.reply(ctx, &format!("Feed {} not found", id)).await {
                    error!("Failed to send message to {}: {}", msg.channel_id.0, e);
                }
                warn!("Could not feed {} to edit.", id);
                return None;
            }
            let location = location.unwrap();

            let push = {
                let mut guard = feeds.write().await;
                let feeds: &mut Vec<Feed> = guard.as_mut();
                if let Some(url) = args.url {
                    info!("Setting feed {} url to {}.", id, url);
                    feeds[location].set_url(&url);
                }

                if let Some(category) = args.category {
                    info!("Setting feed {} discord category to {}.", id, category);
                    let category = if category == "None" {
                        None
                    } else {
                        Some(category)
                    };
                    feeds[location].set_discord_category(&category);
                }

                // Easiest way is to remove the feed then add it again under the new title
                if discord::remove_feed(msg.clone(), &id, &feeds[location..=location], ctx).await
                    != Some(0)
                {
                    if let Err(e) = msg.reply(&ctx, &format!("Feed {} not found", id)).await {
                        error!("Failed to send message to {}: {}", msg.channel_id.0, e);
                    }
                    error!("Removing feed did not find feed {}.", id);
                    return None;
                }

                if let Some(title) = args.title {
                    feeds[location].set_title(&title);
                }

                vec![feeds.remove(location)]
            };

            add_feeds(push, feeds, ctx).await;
            return None;
        }
        Command::RemoveFeed(msg, id) => {
            info!("Removing feed {}", id);

            let remove_result = discord::remove_feed(msg, &id, &feeds.read().await, ctx).await;
            if let Some(idx) = remove_result {
                remove_feed(idx, feeds).await;
            }
        }
        Command::ReloadFeed(msg, id) => {
            info!("Reloading feed {:?}", id);
            if let Some(id) = id {
                let channel_name = discord::title_to_channel_name(&id);
                let location = feeds
                    .read()
                    .await
                    .iter()
                    .enumerate()
                    .find_map(|(idx, feed)| {
                        if discord::title_to_channel_name(feed.title()) == channel_name
                            || feed.url() == id
                        {
                            Some(idx)
                        } else {
                            None
                        }
                    });

                if location.is_none() {
                    if let Err(e) = msg.reply(ctx, &format!("Feed {} not found", id)).await {
                        error!("Failed to send message to {}: {}", msg.channel_id.0, e);
                    }
                    warn!("Could not feed {} to edit.", id);
                    return None;
                }
                let location = location.unwrap();

                let mut guard = feeds.write().await;
                let feeds: &mut Vec<Feed> = guard.as_mut();
                update_feeds(&mut feeds[location..=location], true, ctx).await;
            } else {
                let mut guard = feeds.write().await;
                let feeds: &mut Vec<Feed> = guard.as_mut();
                update_feeds(feeds.as_mut_slice(), true, ctx).await;
            }
        }
        Command::MarkRead(name, link) => {
            let save = match feeds.read().await.iter().enumerate().find(|(_, feed)| {
                let channel_title = crate::discord::title_to_channel_name(feed.title());
                let read_title = format!("read-{}", discord::truncate(&channel_title, 95));
                channel_title == name || read_title == name
            }) {
                None => {
                    warn!("No feed found for MarkRead with name {}.", name);
                    None
                }
                Some((feed_idx, feed)) => match feed {
                    Feed::Rss(rss) => rss
                        .channel
                        .item
                        .iter()
                        .enumerate()
                        .find(|(_, item)| item.link == link)
                        .map(|(idx, _)| (feed_idx, idx))
                        .or_else(|| {
                            error!("Could not find rss item with link {}.", link);
                            None
                        }),
                    Feed::Atom(atom) => atom
                        .entry
                        .iter()
                        .enumerate()
                        .find(|(_, entry)| entry.get_link_href() == link)
                        .map(|(idx, _)| (feed_idx, idx))
                        .or_else(|| {
                            error!("Could not find atom entry with link {}.", link);
                            None
                        }),
                },
            };

            if let Some((feed_idx, idx)) = save {
                {
                    let mut guard = feeds.write().await;
                    let feeds: &mut Vec<Feed> = guard.as_mut();
                    match &mut feeds[feed_idx] {
                        Feed::Rss(ref mut rss) => rss.channel.item[idx].read = Some(()),
                        Feed::Atom(ref mut atom) => atom.entry[idx].read = Some(()),
                    }
                }

                if let Err(e) = crate::feed::export(feeds.read().await.as_ref()).await {
                    error!("Erorr saving feeds: {}.", e);
                }
            }
        }
        Command::MarkUnread(name, link) => {
            let save = match feeds.read().await.iter().enumerate().find(|(_, feed)| {
                let channel_title = crate::discord::title_to_channel_name(feed.title());
                let read_title = format!("read-{}", discord::truncate(&channel_title, 95));
                channel_title == name || read_title == name
            }) {
                None => {
                    warn!("No feed found for MarkUnread with name {}.", name);
                    None
                }
                Some((feed_idx, feed)) => match feed {
                    Feed::Rss(rss) => rss
                        .channel
                        .item
                        .iter()
                        .enumerate()
                        .find(|(_, item)| item.link == link)
                        .map(|(idx, _)| (feed_idx, idx))
                        .or_else(|| {
                            error!("Could not find rss item with link {}.", link);
                            None
                        }),

                    Feed::Atom(atom) => atom
                        .entry
                        .iter()
                        .enumerate()
                        .find(|(_, entry)| entry.get_link_href() == link)
                        .map(|(idx, _)| (feed_idx, idx))
                        .or_else(|| {
                            error!("Could not find atom entry with link {}.", link);
                            None
                        }),
                },
            };

            if let Some((feed_idx, idx)) = save {
                {
                    let mut guard = feeds.write().await;
                    let feeds: &mut Vec<Feed> = guard.as_mut();
                    match &mut feeds[feed_idx] {
                        Feed::Rss(ref mut rss) => rss.channel.item[idx].read = None,
                        Feed::Atom(ref mut atom) => atom.entry[idx].read = None,
                    }
                }

                if let Err(e) = crate::feed::export(feeds.read().await.as_ref()).await {
                    error!("Erorr saving feeds: {}.", e);
                }
            }
        }
        Command::Export(msg, title) => {
            let guard = feeds.read().await;
            let feeds: &[Feed] = guard.as_ref();
            let opml: crate::opml::Opml = (title.unwrap_or_default(), feeds).into();

            let opml = match se::to_string(&opml) {
                Ok(s) => s,
                Err(e) => {
                    if let Err(e) = msg
                        .reply(ctx, &format!("Error serializing opml: {}", e))
                        .await
                    {
                        error!("Failed to send message to {}: {}", msg.channel_id.0, e);
                    }
                    error!("Error serializing opml: {}", e);
                    return None;
                }
            };

            discord::send_str_as_file_reply(msg, opml, ctx).await;
        }
        Command::Import(msg) => {
            if msg.attachments.len() != 1 {
                if let Err(e) = msg.reply(ctx, "Need an atachment to import.").await {
                    error!("Failed to reply to message {}: {}.", msg.id.0, e);
                }
                warn!("Import command used without an attachment.");
                return None;
            }

            let attachment = &msg.attachments[0];
            let opml = match attachment.download().await {
                Err(e) => {
                    if let Err(e) = msg.reply(ctx, "Could not download attachment.").await {
                        error!("Failed to reply to message {}: {}.", msg.id.0, e);
                    }
                    warn!(
                        "Failed to download attachmnet to {} at {}: {}.",
                        msg.id.0, attachment.url, e
                    );
                    return None;
                }
                Ok(v) => String::from_utf8_lossy(v.as_slice()).to_string(),
            };

            let opml: Opml = match de::from_str(&opml) {
                Err(e) => {
                    if let Err(e) = msg
                        .reply(ctx, &format!("Could not parse opml file: {}.", e))
                        .await
                    {
                        error!("Failed to reply to message {}: {}.", msg.id.0, e);
                    }
                    warn!("Could not parse opml file: {}.", e);
                    return None;
                }
                Ok(opml) => opml,
            };

            let mut new_feeds: Vec<Feed> = opml.into();
            debug!("{:?}", new_feeds);

            new_feeds.retain(|feed| {
                Handle::current().block_on(async {
                    if feeds.read().await.iter().any(|f| feed.url() == f.url()) {
                        warn!(
                            "Skipping importing feed {} from OPML, url already exists in database.",
                            feed.url()
                        );
                        return false;
                    }

                    match feed::from_url(feed.url(), Some(feed.title()), None) {
                        Err(e) => {
                            warn!("Could not load feed from url {}: {}.", feed.url(), e);
                        }
                        Ok(feed) => {
                            info!("Adding feed {}.", feed.title());
                            if feeds.read().await.iter().any(|f| feed.url() == f.url()) {
                                info!("Feed {}, already exists.", feed.title());
                                return true;
                            }
                        }
                    };

                    false
                })
            });

            add_feeds(new_feeds, feeds, ctx).await;
            return None;
        }
        Command::Exit => {
            return Some(());
        }
    }

    None
}

async fn diff_feed(update: Feed, feeds: &mut [Feed], ctx: &Context) {
    let feed = feeds.iter_mut().find(|f| f.url() == update.url());
    if let Some(mut feed) = feed {
        info!("Updating feed {}.", feed.title());
        match (update, &mut feed) {
            (Feed::Rss(update), Feed::Rss(ref mut rss)) => {
                debug!("Feed {} is RSS.", rss.channel.title);
                debug!("Updating feed {} items.", rss.channel.title);
                let mut set = HashSet::with_capacity(rss.channel.item.len());
                set.extend(rss.channel.item.iter().map(|i| i.link.clone()));
                for item in update.channel.item {
                    if !set.contains(&item.link) {
                        info!("Feed {} new item: {:?}.", rss.channel.title, item.title);
                        if let Err(e) =
                            discord::publish_rss_item(&rss.channel.title, &item, ctx).await
                        {
                            warn!(
                                "Error publishing rss item {} ({:?}) to discord: {}",
                                item.link, item.title, e
                            );
                        }
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
            (Feed::Atom(update), Feed::Atom(ref mut atom)) => {
                debug!("Feed {} is atom.", atom.title);
                debug!("Updating feed {} items.", atom.title);
                let mut set = HashSet::with_capacity(atom.entry.len());
                set.extend(atom.entry.iter().map(|e| e.id.clone()));
                for entry in update.entry {
                    if !set.contains(&entry.id) {
                        info!("Feed {} hew item: {}.", atom.title, entry.title);
                        if let Err(e) = discord::publish_atom_entry(&atom.title, &entry, ctx).await
                        {
                            warn!(
                                "Error publishing atem item {} to discord: {}",
                                entry.title, e
                            );
                        } else {
                            atom.entry.push(entry);
                        }
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
        info!(
            "Finised sending updates to discord for feed {}.",
            feed.title()
        );
    }
}

async fn update_feeds(feeds: &mut [Feed], force: bool, ctx: &Context) {
    info!("Updating feeds");
    let mut futures = JoinSet::new();
    for feed in feeds.iter_mut() {
        if force || feed.should_update() {
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

async fn exit_feeds_loop(feeds: &[Feed]) -> anyhow::Result<()> {
    debug!("Exiting the background loop");

    feed::export(feeds)
        .await
        .map_err(|e| anyhow::anyhow!("could not save feeds data: {}", e))
}
