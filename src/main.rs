use std::env;
use std::sync::OnceLock;

use tokio::signal::unix::{signal, SignalKind};
use tokio::spawn;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

use serenity::async_trait;
use serenity::framework::standard::{
    macros::{command, group},
    CommandError, CommandResult,
};
use serenity::framework::StandardFramework;
use serenity::model::{channel::Message, gateway::Ready};
use serenity::prelude::*;
use tokio::time::sleep_until;

static EXIT_SENDER: OnceLock<mpsc::Sender<()>> = OnceLock::new();

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is ready", ready.user.name);
        ctx.online().await;

        spawn(async move {
            use nix::sys::signal::{sigprocmask, SigSet, SigmaskHow::SIG_UNBLOCK, SIGINT, SIGTERM};
            let mut sigset = SigSet::empty();
            sigset.add(SIGTERM);
            sigset.add(SIGINT);
            if let Err(errno) = sigprocmask(SIG_UNBLOCK, Some(&sigset), None) {
                eprintln!("SIG_UNBLOCK sigprocmask errno: {}", errno);
            }
            if let (Ok(mut sigint), Ok(mut sigterm)) = (
                signal(SignalKind::interrupt()),
                signal(SignalKind::terminate()),
            ) {
                let (send, mut recv) = mpsc::channel(1);
                EXIT_SENDER.set(send).expect("setting EXIT_SENDER failed");

                let exit = async {
                    println!("Recieved SIGINT, cleaning up bot.");
                    ctx.invisible().await;
                    println!("{} set to invisible", ready.user.name);

                    let now = Instant::now();
                    let duration = Duration::from_secs(1);
                    sleep_until(now.checked_add(duration).unwrap()).await;
                    std::process::exit(0);
                };

                tokio::select! {
                    _ = sigint.recv() => exit.await,
                    _ = sigterm.recv() => exit.await,
                    _ = recv.recv() => exit.await,
                }
                match (sigint.recv().await, sigterm.recv().await) {
                    (Some(()), _) | (_, Some(())) => {}
                    _ => eprintln!("error recieving signals from for handlers"),
                }
            } else {
                eprintln!("error creating sigint signal handler");
            }
        });
    }
}

#[group("general")]
#[commands(ping, exit)]
struct General;

#[tokio::main]
async fn main() {
    use nix::sys::signal::{sigprocmask, SigSet, SigmaskHow::SIG_BLOCK, SIGINT, SIGTERM};
    let mut sigset = SigSet::all();
    sigset.add(SIGTERM);
    sigset.add(SIGINT);
    if let Err(errno) = sigprocmask(SIG_BLOCK, Some(&sigset), None) {
        eprintln!("SIG_BLOCK sigprocmask errno: {}", errno);
    }

    let framework = StandardFramework::new()
        .configure(|c| c.allow_dm(false))
        .group(&GENERAL_GROUP);
    let token = env::var("DISCORD_TOKEN").expect("need DISCORD_TOKEN environment variable set");
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("error creating client");

    if let Err(err) = client.start().await {
        eprintln!("Error while running client: {:?}", err);
    }
}

#[command]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .send_message(&ctx.http, |create| create.content("Pong!"))
        .await?;
    Ok(())
}

#[command]
async fn exit(_ctx: &Context, _msg: &Message) -> CommandResult {
    if let Some(send) = EXIT_SENDER.get() {
        send.send(()).await.map_err(CommandError::from)?;
    } else {
        eprintln!("could not get EXIT_SENDER value");
    }

    Ok(())
}
