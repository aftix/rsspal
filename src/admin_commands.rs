use std::sync::{Arc, OnceLock};

use regex::Regex;

use lazy_static::lazy_static;

use log::{debug, error, info, warn};

use serenity::framework::standard::Args;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
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

use crate::config::Config;
use crate::feed;
use crate::signal::{send_termination, wait_for_termination};
use crate::update::{background_task, Command, EditArgs, COMMANDS};
use crate::{discord, CONFIG};

pub static GUILDS: OnceLock<Vec<GuildId>> = OnceLock::new();
pub static USER_ID: OnceLock<UserId> = OnceLock::new();

#[group]
#[commands(ping, exit, add, remove, poll, edit, reload, export, import)]
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

        if reaction.user_id.is_some_and(|id| id == current_user) {
            debug!("Reaction given by bot, ignoring.");
            return;
        }

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

        let guild_id = if let Some(id) = reaction.guild_id {
            id
        } else {
            warn!("Reaction did not occur in a guild, ignoring.");
            return;
        };

        // Check for the emoji to mark a message read
        if reaction.emoji == '📖'.into() {
            if let Err(e) = discord::mark_read(guild_id, msg, &ctx).await {
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
            if let Err(e) = discord::mark_unread(guild_id, msg, &ctx).await {
                error!("failed to mark item as unread: {}", e);
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
#[num_args(0)]
#[description("Gracefully shutdown the bot.")]
pub async fn exit(_ctx: &Context, _msg: &Message) -> CommandResult {
    info!("Recieved exit command.");
    send_termination().await.map_err(|e| CommandError::from(e))
}

#[command]
#[num_args(0)]
#[description("Check connectivity with a ping pong.")]
pub async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    info!("Recieved ping command.");
    msg.channel_id
        .send_message(&ctx.http, |create| create.content("Pong!"))
        .await?;
    Ok(())
}

#[command]
#[description("Add a feed to the bot list.")]
#[usage("~add <URL> [title]")]
#[min_args(1)]
#[max_args(2)]
pub async fn add(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    info!("Recieved add command");
    let url: String = match args.single() {
        Err(e) => {
            info!(
                "Add command used without correct format ({}): {} ",
                e, msg.content
            );
            match msg.reply(ctx, "Incorrect command usage.").await {
                Err(err) => {
                    error!(
                        "Error replying to message {}: {} and parsing command: {}",
                        msg.id.0, err, e
                    );
                    return Err(anyhow::anyhow!(
                        "error replying to message {}: {} and parsing command: {}",
                        msg.id.0,
                        err,
                        e
                    )
                    .into());
                }
                Ok(_) => return Err(anyhow::anyhow!("error parsing arguments: {}", e).into()),
            }
        }
        Ok(arg) => arg,
    };

    let title: Option<String> = args.single().ok();

    match feed::from_url(&url, title, None) {
        Err(e) => {
            match msg
                .reply(ctx, &format!("Failed to load feed from {}: {}", url, e))
                .await
            {
                Err(err) => {
                    error!(
                        "Failed loading feed from {}: {} and sending reply to {}: {}",
                        url, e, msg.id.0, err
                    );
                    Err(anyhow::anyhow!(
                        "Failed loading feed from {}: {} and sending reply to {}: {}",
                        url,
                        e,
                        msg.id.0,
                        err
                    )
                    .into())
                }
                _ => {
                    error!("Failed loading feed from {}: {}", url, e);
                    Err(anyhow::anyhow!("Failed loading feed from {}: {}", url, e).into())
                }
            }
        }
        Ok(feed) => {
            let barrier = Arc::new(Barrier::new(2));
            let send = COMMANDS.get().expect("failed to get COMMANDS static");
            if let Err(e) = send.send((Command::AddFeed(feed), barrier.clone())).await {
                error!("Failed to send command: {}", e);
                return Err(anyhow::anyhow!("failed to send command: {}", e).into());
            }
            barrier.wait().await;
            Ok(())
        }
    }
}

#[command]
#[description("Remove a feed from the bot.")]
#[usage("~remove <url|title>")]
#[num_args(1)]
pub async fn remove(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    info!("Recieved remove command.");
    let id: String = match args.parse() {
        Err(e) => {
            error!(
                "Add command used without proper form ({}): {}",
                e, msg.content,
            );
            match msg.reply(ctx, "Incorrect command usage.").await {
                Err(err) => {
                    error!(
                        "Error replying to message {}: {} and parsing command: {}",
                        msg.id.0, err, e
                    );
                    return Err(anyhow::anyhow!(
                        "Error replying to message {}: {} and parsing command: {}",
                        msg.id.0,
                        err,
                        e
                    )
                    .into());
                }
                Ok(_) => return Err(anyhow::anyhow!("error parsing arguments: {}", e).into()),
            }
        }
        Ok(s) => s,
    };

    let barrier = Arc::new(Barrier::new(2));
    let send = COMMANDS.get().expect("failed to get COMMANDS static");
    if let Err(e) = send
        .send((Command::RemoveFeed(msg.clone(), id), barrier.clone()))
        .await
    {
        error!("Failed to send command: {}", e);
        match msg.reply(ctx, "Internal error").await {
            Err(err) => {
                error!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e, msg.id.0, err
                );
                return Err(anyhow::anyhow!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e,
                    msg.id.0,
                    err
                )
                .into());
            }
            Ok(_) => {
                error!("Failed to send command: {}", e);
                return Err(anyhow::anyhow!("Failed to send command: {}", e).into());
            }
        }
    }

    barrier.wait().await;
    Ok(())
}

fn parse_edit_args(raw_args: &[String]) -> anyhow::Result<EditArgs> {
    lazy_static! {
        static ref SPACE_REGEX: Regex = Regex::new(r"\s+").unwrap();
    }
    let mut args = EditArgs::default();

    for (idx, arg) in raw_args.iter().enumerate() {
        let cleaned = SPACE_REGEX.replace_all(arg, "");
        let parts: Vec<_> = cleaned.split('=').collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "Key value pair {} ({}) is not properly formatted. ({:?})",
                arg,
                idx,
                parts
            );
        }

        match parts[0].to_lowercase().as_ref() {
            "title" => args.title = Some(parts[1].to_string()),
            "category" => args.category = Some(parts[1].to_string()),
            "url" | "link" => args.url = Some(parts[1].to_string()),
            _ => warn!("Encountered unknown KEY for edit command: {}.", parts[0]),
        }
    }

    Ok(args)
}

#[command]
#[description("Edit feed. Keys are url, title, and category.")]
#[usage("~edit <feed> <KEY=VALUE>...")]
#[min_args(2)]
pub async fn edit(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    info!("Recieved edit command.");
    let id: String = match args.parse() {
        Err(e) => match msg.reply(ctx, "Failed to parse first argument.").await {
            Err(err) => {
                warn!(
                    "Failed to parse first argument to edit: {} and reply to message {}: {}",
                    e, msg.id.0, err
                );
                return Err(anyhow::anyhow!(
                    "Failed to parse first argument to edit: {} and reply to message {}: {}",
                    e,
                    msg.id.0,
                    err
                )
                .into());
            }
            Ok(_) => {
                warn!("Failed to parse first argumnet to edit: {}", e);
                return Err(
                    anyhow::anyhow!("Failed to parse first argumnet to edit: {}", e).into(),
                );
            }
        },
        Ok(s) => s,
    };
    args.advance();

    debug!("Parsing edit arguments: {} ({}).", args.rest(), args.len());
    let mut keyvals = Vec::with_capacity(args.len());
    while !args.is_empty() {
        let keyval: String = match args.parse() {
            Err(e) => match msg.reply(ctx, "Failed to parse first argument.").await {
                Err(err) => {
                    warn!(
                        "Failed to parse argument to edit: {} and reply to message {}: {}",
                        e, msg.id.0, err
                    );
                    return Err(anyhow::anyhow!(
                        "Failed to parse argument to edit: {} and reply to message {}: {}",
                        e,
                        msg.id.0,
                        err
                    )
                    .into());
                }
                Ok(_) => {
                    warn!("Failed to parse first argumnet to edit: {}", e);
                    return Err(
                        anyhow::anyhow!("Failed to parse first argumnet to edit: {}", e).into(),
                    );
                }
            },
            Ok(s) => s,
        };
        keyvals.push(keyval);
        args.advance();
    }

    if keyvals.is_empty() {
        match msg.reply(ctx, "No attributes given to edit.").await {
            Err(e) => {
                warn!(
                    "No attributes given to edit and failed to reply to message {}: {}.",
                    msg.id.0, e
                );
                return Err(anyhow::anyhow!(
                    "No attributes given to edit and failed to reply to message {}: {}.",
                    msg.id.0,
                    e
                )
                .into());
            }
            Ok(_) => {
                warn!("No attributes given to edit.");
                return Err(anyhow::anyhow!("No attributes given to edit.").into());
            }
        }
    }

    let edit_args = match parse_edit_args(&keyvals) {
        Err(e) => {
            error!("Failed to parse the arguments to ~edit: {}.", e);
            return Err(anyhow::anyhow!(e).into());
        }
        Ok(args) => args,
    };

    debug!("Edit args are {:?}.", edit_args);

    let send = COMMANDS.get().expect("failed to get COMMANDS static");
    let barrier = Arc::new(Barrier::new(2));
    if let Err(e) = send
        .send((
            Command::EditFeed(msg.clone(), id, edit_args),
            barrier.clone(),
        ))
        .await
    {
        error!("Failed to send on COMMANDS channel: {}", e);
        match msg.reply(ctx, "Internal error").await {
            Err(err) => {
                error!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e, msg.id.0, err
                );
                return Err(anyhow::anyhow!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e,
                    msg.id.0,
                    err
                )
                .into());
            }
            Ok(_) => {
                error!("Failed to send command: {}", e);
                return Err(anyhow::anyhow!("Failed to send command: {}", e).into());
            }
        }
    }

    barrier.wait().await;
    Ok(())
}

#[command]
#[description("Reload feed.")]
#[usage("~poll [url|title]")]
#[max_args(1)]
pub async fn reload(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    info!("Recieved reload command.");
    let id = if args.is_empty() {
        None
    } else {
        match args.parse::<String>() {
            Err(e) => match msg.reply(ctx, "Failed to parse argument.").await {
                Err(err) => {
                    error!(
                        "Error parsing reload argument: {} and replying to message {}: {}",
                        e, msg.id.0, err
                    );
                    return Err(anyhow::anyhow!(
                        "Error parsing reload argument: {} and replying to message {}: {}",
                        e,
                        msg.id.0,
                        err
                    )
                    .into());
                }
                Ok(_) => {
                    error!("Error parsing reload argument: {}.", e);
                    return Err(anyhow::anyhow!("Error parsing reload argument: {}.", e).into());
                }
            },
            Ok(s) => Some(s),
        }
    };

    let barrier = Arc::new(Barrier::new(2));
    let send = COMMANDS
        .get()
        .expect("failed to read COMMANDS static")
        .clone();
    if let Err(e) = send
        .send((Command::ReloadFeed(msg.clone(), id), barrier.clone()))
        .await
    {
        error!("Failed to send on COMMANDS channel: {}", e);
        match msg.reply(ctx, "Internal error").await {
            Err(err) => {
                error!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e, msg.id.0, err
                );
                return Err(anyhow::anyhow!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e,
                    msg.id.0,
                    err
                )
                .into());
            }
            Ok(_) => {
                error!("Failed to send command: {}", e);
                return Err(anyhow::anyhow!("Failed to send command: {}", e).into());
            }
        }
    }

    barrier.wait().await;
    Ok(())
}

#[command]
#[description("Import OPML feed list")]
#[usage("~import <opml file attached to message>")]
#[num_args(0)]
pub async fn import(ctx: &Context, msg: &Message) -> CommandResult {
    info!("Recieved import command.");

    let barrier = Arc::new(Barrier::new(2));
    let send = COMMANDS
        .get()
        .expect("Failed to get COMMANDS static")
        .clone();

    if let Err(e) = send
        .send((Command::Import(msg.clone()), barrier.clone()))
        .await
    {
        error!("Failed to send on COMMANDS channel: {}", e);
        match msg.reply(ctx, "Internal error").await {
            Err(err) => {
                error!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e, msg.id.0, err
                );
                return Err(anyhow::anyhow!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e,
                    msg.id.0,
                    err
                )
                .into());
            }
            Ok(_) => {
                error!("Failed to send command: {}", e);
                return Err(anyhow::anyhow!("Failed to send command: {}", e).into());
            }
        }
    }

    barrier.wait().await;
    Ok(())
}

#[command]
#[description("Export OPML feed list")]
#[usage("~export [opml title]")]
#[max_args(1)]
pub async fn export(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    info!("Recieved export command.");

    let title = if args.is_empty() {
        None
    } else {
        match args.parse::<String>() {
            Err(e) => match msg.reply(ctx, "Failed to parse argument.").await {
                Err(err) => {
                    error!(
                        "Error parsing export argument: {} and replying to message {}: {}",
                        e, msg.id.0, err
                    );
                    return Err(anyhow::anyhow!(
                        "Error parsing export argument: {} and replying to message {}: {}",
                        e,
                        msg.id.0,
                        err
                    )
                    .into());
                }
                Ok(_) => {
                    error!("Error parsing reload argument: {}.", e);
                    return Err(anyhow::anyhow!("Error parsing export argument: {}.", e).into());
                }
            },
            Ok(s) => Some(s),
        }
    };

    let barrier = Arc::new(Barrier::new(2));
    let send = COMMANDS
        .get()
        .expect("Failed to get COMMANDS static")
        .clone();

    if let Err(e) = send
        .send((Command::Export(msg.clone(), title), barrier.clone()))
        .await
    {
        error!("Failed to send on COMMANDS channel: {}", e);
        match msg.reply(ctx, "Internal error").await {
            Err(err) => {
                error!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e, msg.id.0, err
                );
                return Err(anyhow::anyhow!(
                    "Failed to send command: {} and reply to message {}: {}",
                    e,
                    msg.id.0,
                    err
                )
                .into());
            }
            Ok(_) => {
                error!("Failed to send command: {}", e);
                return Err(anyhow::anyhow!("Failed to send command: {}", e).into());
            }
        }
    }

    barrier.wait().await;
    Ok(())
}

#[command]
#[description("Set polling interval")]
#[usage("~poll <seconds>")]
#[num_args(1)]
pub async fn poll(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    info!("Recieved poll command.");
    match args.parse::<u64>() {
        Err(e) => {
            match msg
                .reply(ctx, "Argument must be an unsigned integer.")
                .await
            {
                Err(err) => {
                    error!("Failed to get new poll interval: {} and send error reply to message {}: {}", e, msg.id.0, err);
                    Err(anyhow::anyhow!("Failed to get new poll interval: {} and send error reply to message {}: {}", e, msg.id.0, err).into())
                }
                Ok(_) => {
                    error!("Failed to get new poll interval: {}", e);
                    Err(anyhow::anyhow!("Failed to get new poll interval: {}", e).into())
                }
            }
        }
        Ok(interval) => {
            let config_path = {
                let mut config = match CONFIG.read() {
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to read CONFIG static {}", e).into())
                    }
                    Ok(cfg) => cfg.clone(),
                };
                config.interval = interval;
                let file = config.config_file.clone();
                match CONFIG.write() {
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to write CONFIG static {}", e).into())
                    }
                    Ok(mut cfg) => *cfg = config,
                }
                file
            };

            match File::open(&config_path).await {
                Err(e) => {
                    warn!("Could not open configuration file: {}", e);
                    Err(anyhow::anyhow!("Could not open configuration file: {}", e).into())
                }
                Ok(mut f) => {
                    let mut s = String::new();
                    if let Err(e) = f.read_to_string(&mut s).await {
                        warn!("Could not read configuration file: {}", e);
                        return Err(
                            anyhow::anyhow!("Could not read configuration file: {}", e).into()
                        );
                    }

                    let mut cfg: Config = match toml::from_str(&s) {
                        Err(e) => {
                            warn!("Could not deserialize configuration file: {}", e);
                            return Err(anyhow::anyhow!(
                                "Could not deserialize configuration file: {}",
                                e
                            )
                            .into());
                        }
                        Ok(c) => c,
                    };

                    cfg.interval = interval;
                    cfg.config_file = config_path;

                    match cfg.save() {
                        Ok(_) => Ok(()),
                        Err(e) => {
                            warn!("Error saving configuration file: {}", e);
                            Err(anyhow::anyhow!("Error saving configuration file: {}", e).into())
                        }
                    }
                }
            }
        }
    }
}
