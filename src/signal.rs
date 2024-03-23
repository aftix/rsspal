use nix::{
    errno::Errno,
    sys::signal::{
        sigprocmask, SigSet,
        SigmaskHow::{SIG_BLOCK, SIG_UNBLOCK},
        SIGINT, SIGTERM,
    },
};
use std::{future::Future, sync::OnceLock};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::mpsc::{self, error::SendError},
    time::{sleep, Duration},
};
use tracing::{debug, error, info, instrument};

static EXIT_SENDER: OnceLock<mpsc::Sender<()>> = OnceLock::new();

fn sigset() -> SigSet {
    let mut sigset = SigSet::empty();
    sigset.add(SIGINT);
    sigset.add(SIGTERM);
    sigset
}

#[instrument(level = "trace")]
pub fn mask_signals() -> Result<(), Errno> {
    info!("Blocking OS termination signals.");
    sigprocmask(SIG_BLOCK, Some(&sigset()), None)
}

#[instrument(level = "trace")]
pub fn unmask_signals() -> Result<(), Errno> {
    info!("Unblocking OS termination signals.");
    sigprocmask(SIG_UNBLOCK, Some(&sigset()), None)
}

#[instrument(level = "debug", skip(exit))]
pub async fn wait_for_termination(exit: impl Future<Output = ()>) {
    if let Err(errno) = unmask_signals() {
        error!("SIG_UNBLOCK sigprocmask errno: {}", errno);
    }

    let sigint = signal(SignalKind::interrupt());
    let sigterm = signal(SignalKind::terminate());
    if let (Ok(mut sigint), Ok(mut sigterm)) = (sigint, sigterm) {
        let (send, mut recv) = mpsc::channel(1);
        EXIT_SENDER.set(send).expect("setting EXIT_SENDER failed");

        debug!("Blocking select statement on all termination signals.");
        tokio::select! {
            _ = sigint.recv() => exit.await,
            _ = sigterm.recv() => exit.await,
            _ = recv.recv() => exit.await,
        }

        sleep(Duration::from_secs(1)).await;
        info!("Exiting process.");
        std::process::exit(0);
    }
}

#[instrument]
pub async fn send_termination() -> Result<(), SendError<()>> {
    debug!("Sending termination message on channel.");
    if let Some(send) = EXIT_SENDER.get() {
        send.send(()).await
    } else {
        error!("Failed to send termination singnal.");
        Err(SendError(()))
    }
}
