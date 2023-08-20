use std::future::Future;
use std::sync::OnceLock;

use nix::errno::Errno;
use nix::sys::signal::{
    sigprocmask, SigSet,
    SigmaskHow::{SIG_BLOCK, SIG_UNBLOCK},
    SIGINT, SIGTERM,
};

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc::{self, error::SendError};
use tokio::time::{sleep_until, Duration, Instant};

static EXIT_SENDER: OnceLock<mpsc::Sender<()>> = OnceLock::new();

fn sigset() -> SigSet {
    let mut sigset = SigSet::empty();
    sigset.add(SIGINT);
    sigset.add(SIGTERM);
    sigset
}

pub fn mask_signals() -> Result<(), Errno> {
    sigprocmask(SIG_BLOCK, Some(&sigset()), None)
}

pub fn unmask_signals() -> Result<(), Errno> {
    sigprocmask(SIG_UNBLOCK, Some(&sigset()), None)
}

pub async fn wait_for_termination(exit: impl Future<Output = ()>) {
    if let Err(errno) = unmask_signals() {
        eprintln!("SIG_UNBLOCK sigprocmask errno: {}", errno);
    }

    let sigint = signal(SignalKind::interrupt());
    let sigterm = signal(SignalKind::terminate());
    if let (Ok(mut sigint), Ok(mut sigterm)) = (sigint, sigterm) {
        let (send, mut recv) = mpsc::channel(1);
        EXIT_SENDER.set(send).expect("setting EXIT_SENDER failed");

        tokio::select! {
            _ = sigint.recv() => exit.await,
            _ = sigterm.recv() => exit.await,
            _ = recv.recv() => exit.await,
        }

        let now = Instant::now();
        let duration = Duration::from_secs(1);
        sleep_until(
            now.checked_add(duration)
                .expect("could not add 1 second to current instant"),
        )
        .await;
        std::process::exit(0);
    }
}

pub async fn send_termination() -> Result<(), SendError<()>> {
    if let Some(send) = EXIT_SENDER.get() {
        send.send(()).await
    } else {
        Err(SendError(()))
    }
}
