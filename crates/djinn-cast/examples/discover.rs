//! Lists cast devices on the network, changing nothing:
//! `cargo run -p djinn-cast --example discover`.

#[tokio::main]
async fn main() {
    let caster = djinn_cast::Caster::new();
    for status in caster.discover(std::time::Duration::from_secs(4)).await {
        let d = &status.device;
        println!(
            "{:<6} {:<28} {:<22} group={} {}",
            format!("{:?}", d.kind),
            d.name,
            d.model.as_deref().unwrap_or(""),
            d.group,
            d.id
        );
    }
}
