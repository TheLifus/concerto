use crate::error::Result;
use crate::install_event::{InstallEvent, InstallSummary};
use crate::output::{event_message, recv_finished, summary_message, worker_disconnected};
use std::sync::mpsc;
use std::time::Duration;

pub(super) fn run(
    events: mpsc::Receiver<InstallEvent>,
    result: mpsc::Receiver<Result<InstallSummary>>,
) -> Result<()> {
    loop {
        if let Some(result) = recv_finished(&result).map_err(|_| worker_disconnected())? {
            drain_remaining_events(&events);

            return result.map(|summary| {
                println!("{}", summary_message(summary));
            });
        }

        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => {
                println!("{}", event_message(&event));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return result
                    .recv()
                    .map_err(|_| worker_disconnected())?
                    .map(|summary| {
                        println!("{}", summary_message(summary));
                    });
            }
        }
    }
}

fn drain_remaining_events(events: &mpsc::Receiver<InstallEvent>) {
    while let Ok(event) = events.recv() {
        println!("{}", event_message(&event));
    }
}
