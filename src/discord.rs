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

pub fn title_to_channel_name(s: impl AsRef<str>) -> String {
    lazy_static! {
        static ref SPACE_REGEX: Regex = Regex::new(r"\s+").unwrap();
        static ref SPECIAL_REGEX: Regex = Regex::new(r"[^\w-]").unwrap();
        static ref ENDS_REGEX: RegexSet = RegexSet::new(&[r"^-+", r"-+$",]).unwrap();
    }

    let s = SPACE_REGEX.replace_all(s.as_ref(), "-");
    let s = SPECIAL_REGEX.replace_all(&s, "");
    let s = SPECIAL_REGEX.replace_all(&s, "");
    s[..95].to_string()
}

async fn mark(
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
    msg.delete(&ctx).await?;

    let channels = ctx.http.get_channels(channel.guild_id.0).await?;
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
    new_msg.react(&ctx, emoji).await?;

    Ok(())
}

pub async fn mark_read(msg: Message, ctx: &Context) -> anyhow::Result<()> {
    mark(msg, ctx, |name| format!("read-{}", &name[..95]), '📕').await
}

pub async fn mark_unread(msg: Message, ctx: &Context) -> anyhow::Result<()> {
    mark(
        msg,
        ctx,
        |name| {
            name.strip_prefix("read-")
                .map(String::from)
                .unwrap_or_else(|| name.to_string())
        },
        '📖',
    )
    .await
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
        format!("read-{}", &title_to_channel_name(feed_name)[..95])
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
    let channel_name = if item.read.is_some() {
        title_to_channel_name(feed_name)
    } else {
        format!("read-{}", &title_to_channel_name(feed_name)[..95])
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
                "Publishing item {}, feed {}, on guild {}.",
                item.link, feed_name, guild.0
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
        let channels = match ctx.http.get_channels(guild.0).await {
            Ok(c) => c,
            Err(e) => {
                error!("Could not get channels for guild {}: {}", guild.0, e);
                break;
            }
        };
        let mut channels_by_name: HashMap<_, _> = channels
            .iter()
            .cloned()
            .map(|c| (c.name.clone(), c))
            .collect();
        let mut channels: HashMap<_, _> = channels.into_iter().map(|c| (c.id, c)).collect();

        for feed in feeds {
            let chan_name = title_to_channel_name(&feed.title());

            // Setup the channels
            let new_channel = if let Some(channel) = channels_by_name.get(&chan_name) {
                // Channel exists
                info!(
                    "Found channel {} in guild {}, ensuring correct metadata.",
                    channel.name, guild.0
                );

                // Set correct channel category
                let category = feed.discord_category();
                setup_channel_metadata(
                    &channels_by_name,
                    category.as_ref().map(String::as_str),
                    &channel,
                    ctx,
                )
                .await
            } else {
                // Channel does not exist
                // Make sure to get the correct category
                let parent = if let Some(category) = feed.discord_category() {
                    // Needs a category
                    let category = title_to_channel_name(category);

                    // Make sure category exists
                    let cat_channel = if let Some(cat_chan) = channels_by_name.get(&category) {
                        Some(cat_chan.clone())
                    } else {
                        let create = api_params::create_channel(&category, true, None, None);
                        match ctx
                            .http
                            .create_channel(guild.0, &create, Some("rsspal creating category"))
                            .await
                        {
                            Err(e) => {
                                error!(
                                    "Error creating category {} in guild {}: {}",
                                    category, guild.0, e
                                );
                                None
                            }
                            Ok(cat_chan) => Some(cat_chan),
                        }
                    };

                    // add category channel to hash maps
                    if let Some(chan) = cat_channel.as_ref() {
                        channels.insert(chan.id.clone(), chan.clone());
                        channels_by_name.insert(chan.name.clone(), chan.clone());
                    }

                    cat_channel
                } else {
                    // No category
                    None
                };
                // Now make the feed channel
                let create = api_params::create_channel(
                    &chan_name,
                    false,
                    Some(&feed.description()),
                    parent.map(|parent| parent.id.0),
                );

                match ctx
                    .http
                    .create_channel(guild.0, &create, Some("rsspal creating feed channel"))
                    .await
                {
                    Err(e) => error!(
                        "Failed creating channel {} in guild {}: {}",
                        chan_name, guild.0, e
                    ),
                    Ok(chan) => {
                        // add new channel to hashmaps
                        channels.insert(chan.id.clone(), chan.clone());
                        channels_by_name.insert(chan.name.clone(), chan.clone());

                        // Create the thread for read items
                        let thread = api_params::create_thread(&chan_name);
                        if let Err(e) = ctx.http.create_private_thread(chan.id.0, &thread).await {
                            error!(
                                "Unable to create read thread for channel {}: {}",
                                chan_name, e
                            );
                        }
                    }
                };

                None
            };

            // At the last, create any new channels needed
            if let Some(new) = new_channel {
                channels_by_name.insert(new.name.clone(), new.clone());
                channels.insert(new.id.clone(), new);
            }
        }

        // Remove empty channel categories
        let channels: Vec<_> = channels.values().cloned().collect();
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
}

type NameMap = HashMap<String, GuildChannel>;

async fn setup_channel_metadata(
    by_name: &NameMap,
    category: Option<&str>,
    channel: &GuildChannel,
    ctx: &Context,
) -> Option<GuildChannel> {
    debug!(
        "Setting up channel category {:?} for {} in guild {}.",
        category, channel.name, channel.guild_id
    );

    match category {
        // Case when feed has a discord category to belong to
        Some(category) => {
            let category = category.to_owned();
            // If category exists, you can just set the right parent_id
            let (parent, new) = if let Some(cat_chan) = by_name.get(&category) {
                (cat_chan.clone(), false)
            } else {
                // Create non existant category
                let create = api_params::create_channel(&category, true, None, None);

                match ctx
                    .http
                    .create_channel(
                        channel.guild_id.0,
                        &create,
                        Some("rsspal creating category"),
                    )
                    .await
                {
                    Err(e) => {
                        error!("Error creating category {}: {}", category, e);
                        return None;
                    }
                    Ok(cat_chan) => (cat_chan, true),
                }
            };
            let modify = api_params::modify_channel(
                Some(&channel.name),
                Some(parent.id.0),
                channel.topic.as_ref().map(String::as_str),
                true,
            );
            if let Err(e) = ctx
                .http
                .edit_channel(channel.id.0, &modify, Some("rsspal updating channel"))
                .await
            {
                error!("Error editing channel {}: {}", channel.name, e);
            }

            if new {
                Some(parent)
            } else {
                None
            }
        }
        // Case for feeds without a category
        None => {
            let modify = api_params::modify_channel(
                Some(&channel.name),
                None,
                channel.topic.as_ref().map(String::as_str),
                true,
            );
            if let Err(e) = ctx
                .http
                .edit_channel(channel.id.0, &modify, Some("rsspal updating channel"))
                .await
            {
                error!("Error editing channel {}: {}", channel.name, e);
            }

            None
        }
    }
}
