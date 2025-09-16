use tracing::{Level, subscriber::set_global_default};
use tracing_subscriber::FmtSubscriber;

pub fn init_logger() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    // Best-effort; avoid panicking if a global subscriber already exists
    let _ = set_global_default(subscriber);
}
