use std::collections::{HashMap, HashSet};

use regex::{Regex, RegexSet};

use log::{debug, error, info, warn};

use serenity::builder::CreateEmbed;
use serenity::model::prelude::*;
use serenity::prelude::*;

use lazy_static::lazy_static;

use crate::admin_commands::GUILDS;
use crate::feed::atom::Entry;
use crate::feed::rss::RssItem;
use crate::feed::Feed;

mod api_params;

pub fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        None => s,
        Some((idx, _)) => &s[..idx],
    }
}

pub fn title_to_channel_name(s: impl AsRef<str>) -> String {
    lazy_static! {
        static ref SPACE_REGEX: Regex = Regex::new(r"\s+").unwrap();
        static ref SPECIAL_REGEX: Regex = Regex::new(r"[^\w-]").unwrap();
        static ref ENDS_REGEX: RegexSet = RegexSet::new(&[r"^-+", r"-+$",]).unwrap();
    }

    let s = SPACE_REGEX.replace_all(s.as_ref(), "-");
    let s = SPECIAL_REGEX.replace_all(&s, "");
    let s = SPECIAL_REGEX.replace_all(&s, "");
    truncate(&s, 95).to_lowercase().to_string()
}

async fn mark(
    guild_id: GuildId,
    msg: Message,
    ctx: &Context,
    channel_name: impl FnOnce(&str) -> String,
    emoji: char,
) -> anyhow::Result<()> {
    if msg.embeds.len() != 1 {
        anyhow::bail!("Message {} does not appear to be a feed item.", msg.id);
    }

    let channel = msg
        .channel(&ctx)
        .await?
        .guild()
        .ok_or_else(|| anyhow::anyhow!("message {} is not in guild channel", msg.id))?;
    let channel_name = channel_name(channel.name());
    let embed: CreateEmbed = msg.embeds[0].clone().into();

    let mut channels = ctx.http.get_channels(channel.guild_id.0).await?;
    debug!("Searching for threads");
    match guild_id.get_active_threads(ctx).await {
        Err(e) => warn!("Failed to get active threads in {}: {}", guild_id.0, e),
        Ok(threads) => {
            channels.extend(threads.threads.into_iter());
        }
    }
    let mut archived_threads = Vec::new();
    for chan in &channels {
        if let Ok(threads) = ctx
            .http
            .get_channel_archived_public_threads(chan.id.0, None, None)
            .await
        {
            archived_threads.extend(threads.threads.into_iter());
        }
    }
    channels.extend(archived_threads.into_iter());
    let new_channel = channels
        .iter()
        .find(|ch| ch.name == channel_name)
        .ok_or_else(|| anyhow::anyhow!("thread for read items not found ({})", channel_name))?;

    let new_msg = new_channel
        .send_message(&ctx, |msg| {
            msg.add_embed(|e| {
                e.clone_from(&embed);
                e
            })
        })
        .await?;
    msg.delete(&ctx).await?;
    new_msg.react(&ctx, emoji).await?;

    Ok(())
}

pub async fn mark_read(guild_id: GuildId, msg: Message, ctx: &Context) -> anyhow::Result<()> {
    mark(
        guild_id,
        msg,
        ctx,
        |name| format!("read-{}", truncate(&name, 95)),
        '📕',
    )
    .await
}

pub async fn mark_unread(guild_id: GuildId, msg: Message, ctx: &Context) -> anyhow::Result<()> {
    mark(
        guild_id,
        msg,
        ctx,
        |name| {
            truncate(name, 100)
                .strip_prefix("read-")
                .map(String::from)
                .unwrap_or_else(|| name.to_string())
        },
        '📖',
    )
    .await
}

// Returns index of feed to remove in feeds
pub async fn remove_feed(msg: Message, id: &str, feeds: &[Feed], ctx: &Context) -> Option<usize> {
    let channel_name = title_to_channel_name(id);
    let location = feeds.iter().enumerate().find_map(|(idx, feed)| {
        if title_to_channel_name(feed.title()) == channel_name || feed.url() == id {
            Some(idx)
        } else {
            None
        }
    });

    if location.is_none() {
        if let Err(e) = msg.reply(ctx, &format!("Feed {} not found", id)).await {
            error!("Failed to send message to {}: {}", msg.channel_id.0, e);
        }
        warn!("Could not find feed {} to remove.", id);
        return None;
    }

    let location = location.unwrap();

    let guilds = GUILDS
        .get()
        .expect("failed to read GUILDS static variable")
        .clone();

    let title = feeds[location].title();
    let channel_name = title_to_channel_name(title);

    for guild in guilds {
        let channels = match ctx.http.get_channels(guild.0).await {
            Err(e) => {
                warn!("Failed to get channels for guild {}: {}", guild.0, e);
                continue;
            }
            Ok(c) => c,
        };

        let channel = channels.iter().find(|c| c.name() == channel_name);
        if let Some(channel) = channel {
            match ctx.http.delete_channel(channel.id.0).await {
                Err(e) => error!("Failed deleting channel {} in guild {}: {}", id, guild.0, e),
                _ => (),
            }
        } else {
            warn!(
                "Removing feed {} but channel {} not found in guild {}.",
                id, channel_name, guild.0
            );
        }

        remove_empty_categories(&guild, &channels, ctx).await;
    }
    Some(location)
}

pub async fn publish_atom_entry(
    feed_name: &str,
    entry: &Entry,
    ctx: &Context,
) -> anyhow::Result<()> {
    info!("Publishing item {} to feed {}", entry.title, feed_name);
    let channel_name = if entry.read.is_some() {
        title_to_channel_name(feed_name)
    } else {
        format!("read-{}", &title_to_channel_name(feed_name))
    };

    let guilds = {
        if let Some(g) = GUILDS.get() {
            g.clone()
        } else {
            error!("Could not get GUILDS static variable.");
            anyhow::bail!("could not access GUILDS static variable");
        }
    };

    let embed_cb = entry.to_embed();

    for guild in guilds {
        let channels = guild.channels(ctx).await?;
        let to_publish = channels.iter().find_map(|(_, c)| {
            if c.name == channel_name {
                Some(c)
            } else {
                None
            }
        });
        if let Some(channel) = to_publish {
            info!(
                "Publishing item {}, feed {}, on guild {}.",
                entry.title, feed_name, guild.0
            );
            let msg = channel
                .send_message(ctx, |msg| msg.embed(&embed_cb))
                .await?;
            let emoji = if entry.read.is_some() { '📕' } else { '📖' };
            if let Err(e) = msg.react(ctx, emoji).await {
                warn!("Unable to react to message for {}: {}", entry.title, e);
            }
        }
    }

    Ok(())
}

pub async fn publish_rss_item(
    feed_name: &str,
    item: &RssItem,
    ctx: &Context,
) -> anyhow::Result<()> {
    info!("Publishing item {} to feed {}", item.link, feed_name);
    let channel_name = if item.read.is_none() {
        title_to_channel_name(feed_name)
    } else {
        format!("read-{}", &title_to_channel_name(feed_name))
    };

    let guilds = {
        if let Some(g) = GUILDS.get() {
            g.clone()
        } else {
            error!("Could not get GUILDS static variable.");
            anyhow::bail!("could not access GUILDS static variable");
        }
    };

    let embed_cb = item.to_embed();

    for guild in guilds {
        let channels = guild.channels(ctx).await?;
        let to_publish = channels.iter().find_map(|(_, c)| {
            if c.name == channel_name {
                Some(c)
            } else {
                None
            }
        });
        if let Some(channel) = to_publish {
            info!(
                "Publishing item {}, feed {}, channel {}, on guild {}.",
                item.link, feed_name, channel.name, guild.0
            );
            let msg = channel
                .send_message(ctx, |msg| msg.embed(&embed_cb))
                .await?;
            let emoji = if item.read.is_some() { '📕' } else { '📖' };
            if let Err(e) = msg.react(ctx, emoji).await {
                warn!("Unable to react to message for {}: {}", item.link, e);
            }
        }
    }

    Ok(())
}

pub async fn setup_channels(feeds: &[Feed], ctx: &Context) {
    info!("Setting up discord servers to fit read roles.");
    let guilds = {
        if let Some(g) = GUILDS.get() {
            g.clone()
        } else {
            error!("Could not get GUILDS static variable.");
            return;
        }
    };

    for guild in guilds {
        info!("Setting up guild {}.", guild);
        let mut channels = match ctx.http.get_channels(guild.0).await {
            Ok(c) => c,
            Err(e) => {
                error!("Could not get channels for guild {}: {}", guild.0, e);
                break;
            }
        };
        match guild.get_active_threads(ctx).await {
            Err(e) => warn!("Failed to get active threads in {}: {}", guild.0, e),
            Ok(threads) => {
                channels.extend(threads.threads.into_iter());
            }
        }
        let mut archived_threads = Vec::new();
        for chan in &channels {
            if let Ok(threads) = ctx
                .http
                .get_channel_archived_public_threads(chan.id.0, None, None)
                .await
            {
                archived_threads.extend(threads.threads.into_iter());
            }
        }
        channels.extend(archived_threads.into_iter());

        let mut channels_by_name: HashMap<_, _> = channels
            .iter()
            .cloned()
            .map(|c| (c.name.clone(), c))
            .collect();
        let mut channels: HashMap<_, _> = channels.into_iter().map(|c| (c.id, c)).collect();

        for feed in feeds {
            let chan_name = title_to_channel_name(&feed.title());
            if setup_channel_category(
                guild.0,
                feed.discord_category(),
                &mut channels,
                &mut channels_by_name,
                ctx,
            )
            .await
            .is_none()
            {
                return;
            }

            let add_channel = if let Some(channel) = channels_by_name.get(&chan_name) {
                update_channel_metadata(&channel, &feed, &channels_by_name, ctx).await
            } else {
                create_channel(guild.0, &feed, &channels_by_name, ctx).await
            };

            if let Some((new_chan, new_thread)) = add_channel {
                channels.insert(new_chan.id.clone(), new_chan.clone());
                channels_by_name.insert(new_chan.name.clone(), new_chan);

                channels.insert(new_thread.id.clone(), new_thread.clone());
                channels_by_name.insert(new_thread.name.clone(), new_thread);
            }
        }

        remove_empty_categories(
            &guild,
            channels.into_values().collect::<Vec<_>>().as_slice(),
            ctx,
        )
        .await;
    }
}

type NameMap = HashMap<String, GuildChannel>;
type IdMap = HashMap<ChannelId, GuildChannel>;

async fn setup_channel_category(
    guild_id: u64,
    name: Option<String>,
    by_id: &mut IdMap,
    by_name: &mut NameMap,
    ctx: &Context,
) -> Option<()> {
    if name.is_none() {
        return Some(());
    }

    let name = title_to_channel_name(&name.unwrap());

    if by_name.contains_key(&name) {
        return Some(());
    }

    // Need to set a category channel that doesn't exist
    let create = api_params::create_channel(&name, true, None, None);
    match ctx
        .http
        .create_channel(guild_id, &create, Some("rsspal creating category"))
        .await
    {
        Err(e) => {
            error!("Could not create category {}: {}", name, e);
            None
        }
        Ok(channel) => {
            info!("Created channel category {}.", channel.name);
            by_name.insert(channel.name.clone(), channel.clone());
            by_id.insert(channel.id, channel);
            Some(())
        }
    }
}

async fn remove_empty_categories(guild: &GuildId, channels: &[GuildChannel], ctx: &Context) {
    let parents: HashSet<_> = channels.iter().filter_map(|c| c.parent_id).collect();
    let empty_categories: Vec<_> = channels
        .iter()
        .filter(|&c| c.kind == ChannelType::Category && !parents.contains(&c.id))
        .cloned()
        .collect();

    for category in empty_categories {
        if let Err(e) = ctx.http.delete_channel(category.id.0).await {
            warn!(
                "Could not delete empty category {} in guild {}: {}",
                category.name, guild.0, e
            );
        }
    }
}

// Optionally returns new (feed channel, feed channel thread)
async fn update_channel_metadata(
    channel: &GuildChannel,
    feed: &Feed,
    by_name: &NameMap,
    ctx: &Context,
) -> Option<(GuildChannel, GuildChannel)> {
    let name = title_to_channel_name(&feed.title());
    let read_title = format!("read-{}", truncate(&name, 95));

    let parent_id = if let Some(category) = feed.discord_category() {
        if let Some(parent) = by_name.get(&title_to_channel_name(&category)) {
            Some(parent.id.0)
        } else {
            warn!(
                "Feed {} wants category {} but it does not exist.",
                feed.title(),
                category
            );
            None
        }
    } else {
        None
    };

    info!("Modifying channel {}.", channel.id.0);
    let modify = api_params::modify_channel(Some(&name), parent_id, None, false);
    let new_channel = match ctx
        .http
        .edit_channel(
            channel.id.0,
            &modify,
            Some("rsspal editing channel metadata"),
        )
        .await
    {
        Err(e) => {
            error!("Failed to edit channel {}: {}", channel.id.0, e);
            return None;
        }
        Ok(chan) => chan,
    };

    // Now, edit or create thread within the channel
    let new_thread = match by_name.get(&read_title) {
        None => {
            let msg = match new_channel
                .send_message(ctx, |msg| msg.content("View read news items"))
                .await
            {
                Err(e) => {
                    error!("Failed to send message to {}: {}", new_channel.id.0, e);
                    return None;
                }
                Ok(msg) => msg,
            };
            let create = api_params::create_thread(&read_title);
            match ctx
                .http
                .create_public_thread(new_channel.id.0, msg.id.0, &create)
                .await
            {
                Err(e) => {
                    error!("Failed to create thread {}: {}", read_title, e);
                    return None;
                }
                Ok(thread) => thread,
            }
        }
        Some(thread) => {
            let edit = api_params::modify_channel(
                Some(&read_title),
                None,
                Some(&format!("Read items: {}", feed.description())),
                false,
            );
            match ctx
                .http
                .edit_channel(thread.id.0, &edit, Some("rsspal editing read thread"))
                .await
            {
                Err(e) => {
                    error!("Failed to edit thread {}: {}", thread.id.0, e);
                    return None;
                }
                Ok(thread) => thread,
            }
        }
    };

    Some((new_channel, new_thread))
}

// Optionally returns new (feed channel, feed channel thread)
async fn create_channel(
    guild_id: u64,
    feed: &Feed,
    by_name: &NameMap,
    ctx: &Context,
) -> Option<(GuildChannel, GuildChannel)> {
    let name = title_to_channel_name(&feed.title());
    let read_title = format!("read-{}", truncate(&name, 95));

    let parent_id = if let Some(category) = feed.discord_category() {
        if let Some(parent) = by_name.get(&title_to_channel_name(&category)) {
            Some(parent.id.0)
        } else {
            warn!(
                "Feed {} wants category {} but it does not exist.",
                feed.title(),
                category
            );
            None
        }
    } else {
        None
    };

    info!("Creating channel {}.", name);
    let create = api_params::create_channel(&name, false, Some(&feed.description()), parent_id);
    let new_channel = match ctx
        .http
        .create_channel(guild_id, &create, Some("rsspal creating channel"))
        .await
    {
        Err(e) => {
            error!("Failed to create channel {}: {}", name, e);
            return None;
        }
        Ok(chan) => chan,
    };

    let msg = match new_channel
        .send_message(ctx, |msg| msg.content("View read news items"))
        .await
    {
        Err(e) => {
            error!("Failed to send message to {}: {}", new_channel.id.0, e);
            return Some((new_channel.clone(), new_channel));
        }
        Ok(msg) => msg,
    };

    let create = api_params::create_thread(&read_title);
    let new_thread = match ctx
        .http
        .create_public_thread(new_channel.id.0, msg.id.0, &create)
        .await
    {
        Err(e) => {
            error!("Failed to create thread {}: {}", read_title, e);
            return Some((new_channel.clone(), new_channel));
        }
        Ok(thread) => thread,
    };

    Some((new_channel, new_thread))
}
