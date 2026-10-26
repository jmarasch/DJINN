//! Asks a Cast device for its status, changing nothing:
//! `cargo run -p djinn-cast --example probe -- 192.168.0.84`.

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let Some(ip) = std::env::args().nth(1).and_then(|a| a.parse().ok()) else {
        eprintln!("usage: probe <ip>");
        return;
    };
    match djinn_cast::cast::probe(ip, 8009).await {
        Ok(status) => println!("{status:#}"),
        Err(e) => eprintln!("{e}"),
    }
}
