use std::net::{IpAddr, SocketAddr};

use arcrelay_network::PeerAdvertisement;
use arcrelay_peer::ServiceInstanceId;

/// Arc Input's read-only projection of the process-wide discovery registry.
/// The feature deliberately owns no advertiser, browser, or discovery task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub service_instance_id: ServiceInstanceId,
    pub display_name: String,
    pub addresses: Vec<IpAddr>,
    pub connection_addresses: Vec<SocketAddr>,
    pub port: u16,
    pub certificate_sha256: String,
    pub certificate_sha256_bytes: [u8; 32],
    pub capability_digest: String,
}

impl TryFrom<&PeerAdvertisement> for DiscoveredPeer {
    type Error = arcrelay_peer::PeerIdError;

    fn try_from(peer: &PeerAdvertisement) -> Result<Self, Self::Error> {
        Ok(Self {
            service_instance_id: ServiceInstanceId::parse(peer.device_id.to_string())?,
            display_name: peer.metadata.name.clone(),
            addresses: peer.addresses.clone(),
            connection_addresses: peer.connection_addresses.clone(),
            port: peer.port,
            certificate_sha256: hex(&peer.certificate_sha256),
            certificate_sha256_bytes: peer.certificate_sha256,
            capability_digest: String::new(),
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}
