use std::sync::OnceLock;

use serde_json::Value;
use tokio::sync::broadcast;

fn channel() -> &'static broadcast::Sender<String> {
    static EVENTS: OnceLock<broadcast::Sender<String>> = OnceLock::new();
    EVENTS.get_or_init(|| broadcast::channel(256).0)
}

pub(crate) fn subscribe() -> broadcast::Receiver<String> {
    channel().subscribe()
}

pub(crate) fn publish(event: Value) -> bool {
    // This reports the presence of observers, not delivery acknowledgement; no observers is normal.
    channel().send(event.to_string()).is_ok()
}
