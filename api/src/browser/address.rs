// Copyright © 2026 Jalapeno Labs

//! The address a browser names in its WebRTC candidates: where viewers reach the host.
//!
//! It is the address `ELYSIUM_PUBLIC_URL`'s host resolves to, since that is where viewers
//! reach Elysium already, and nginx publishes the media ports beside the web port.
//! `localhost` is the loopback address. A host that resolves to several addresses uses the
//! first IPv4 one, which Neko advertises reliably; an IPv6-only host uses its first address.

use std::net::{IpAddr, Ipv4Addr};

use url::{Host, Url};

/// The public URL's host did not resolve to an address.
#[derive(Debug, thiserror::Error)]
#[error(
    "cannot resolve {host}, the host in ELYSIUM_PUBLIC_URL, to the address browsers stream from: {reason}"
)]
pub struct AddressError {
    host: String,
    reason: String,
}

/// Resolves `public_url`'s host to the address viewers stream from.
///
/// # Errors
/// [`AddressError`] when the host does not resolve, or resolves to nothing.
pub async fn public_address(public_url: &Url) -> Result<IpAddr, AddressError> {
    let domain = match public_url.host() {
        Some(Host::Ipv4(address)) => return Ok(IpAddr::V4(address)),
        Some(Host::Ipv6(address)) => return Ok(IpAddr::V6(address)),
        Some(Host::Domain("localhost")) => return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        Some(Host::Domain(domain)) => domain,
        None => {
            return Err(AddressError {
                host: String::new(),
                reason: "it has no host".to_owned(),
            });
        }
    };

    let addresses: Vec<IpAddr> = tokio::net::lookup_host((domain, 0))
        .await
        .map_err(|error| AddressError {
            host: domain.to_owned(),
            reason: error.to_string(),
        })?
        .map(|socket| socket.ip())
        .collect();
    choose(&addresses).ok_or_else(|| AddressError {
        host: domain.to_owned(),
        reason: "it has no address records".to_owned(),
    })
}

/// The first IPv4 address, or the first address of any kind when there is none.
fn choose(addresses: &[IpAddr]) -> Option<IpAddr> {
    addresses
        .iter()
        .find(|address| address.is_ipv4())
        .or_else(|| addresses.first())
        .copied()
}

#[cfg(test)]
mod tests {
    use std::net::Ipv6Addr;

    use super::*;

    #[tokio::test]
    async fn localhost_streams_from_loopback() {
        let url = Url::parse("http://localhost:4000").expect("a static URL parses");
        assert_eq!(
            public_address(&url).await.expect("resolves"),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
    }

    #[test]
    fn ipv4_is_preferred_over_an_earlier_ipv6_address() {
        let ipv6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        let ipv4 = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
        assert_eq!(choose(&[ipv6, ipv4]), Some(ipv4));
        assert_eq!(choose(&[ipv6]), Some(ipv6));
        assert_eq!(choose(&[]), None);
    }
}
