//! Shows a page on a Cast device for a while, logging the conversation:
//! `RUST_LOG=djinn_cast=debug cargo run -p djinn-cast --example castpage -- <name> <url> <secs>`.

use std::time::Duration;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "djinn_cast=debug".into()),
        )
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [name, url, secs] = args.as_slice() else {
        eprintln!("usage: castpage <device name> <url> <secs>");
        return;
    };
    let caster = djinn_cast::Caster::new();
    caster.discover(Duration::from_secs(4)).await;
    let Some(device) = caster
        .status()
        .into_iter()
        .find(|s| s.device.name.eq_ignore_ascii_case(name))
    else {
        eprintln!("{name} not found");
        return;
    };
    let id = device.device.id.clone();
    println!(
        "casting to {} at {} (screen: {})",
        device.device.name, device.device.addr, device.device.screen
    );
    if let Err(e) = caster
        .start(&id, djinn_cast::CastMedia::Page { url: url.clone() })
        .await
    {
        eprintln!("{e}");
        return;
    }
    let mut changes = caster.subscribe();
    let end = tokio::time::Instant::now() + Duration::from_secs(secs.parse().unwrap_or(20));
    while tokio::time::timeout_at(end, changes.changed())
        .await
        .is_ok()
    {
        if let Some(s) = caster.status().into_iter().find(|s| s.device.id == id) {
            println!(
                "status: active={} playing={} error={:?}",
                s.active, s.playing, s.error
            );
        }
    }
    // KEEP=1 leaves it running, to test casting onto a busy device.
    if std::env::var_os("KEEP").is_none() {
        caster.stop(&id).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
