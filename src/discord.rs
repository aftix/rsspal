use std::collections::HashMap;

use regex::{Regex, RegexSet};

use log::{debug, error, info};

use serenity::model::prelude::*;
use serenity::prelude::*;

use lazy_static::lazy_static;

use crate::admin_commands::GUILDS;
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
    s.to_string()
}

pub async fn setup_channels(feeds: &[Feed], ctx: &Context) {
    info!("Setting up discord servers to fit read roles.");
    let guilds = {
        if let Some(g) = GUILDS.get() {
            g
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
