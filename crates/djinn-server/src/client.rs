//! Who is on the other end of a request: their IP address and, for
//! devices on the same network, their hardware (MAC) address.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use djinn_platform::neighbors;

use crate::tls::Peer;

/// Marks a request that came over DJINN's own HTTPS.
#[derive(Clone, Copy, Debug)]
pub struct DirectTls;

/// Turns what the listener knows about a connection into what handlers read.
pub async fn note_peer(mut request: Request, next: Next) -> Response {
    if let Some(ConnectInfo(peer)) = request.extensions().get::<ConnectInfo<Peer>>().copied() {
        request.extensions_mut().insert(ConnectInfo(peer.addr));
        if peer.tls {
            request.extensions_mut().insert(DirectTls);
        }
    }
    next.run(request).await
}

/// The client's IP address. `None` when the server wasn't started with
/// connection info (tests).
pub struct ClientIp(pub Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|info| info.0.ip().to_canonical()),
        ))
    }
}

/// Looks up the MAC address for `ip` in the neighbor table.
pub async fn mac_for(ip: Option<IpAddr>) -> Option<String> {
    let ip = ip?;
    tokio::task::spawn_blocking(move || neighbors::mac_for(ip))
        .await
        .ok()
        .flatten()
}
