use semver::Version;

use std::clone::Clone;

#[derive(Clone)]
pub struct PeerID {
    bytes: [u8; 20],
}

impl PeerID {
    const CLIENT_ID: &'static [u8; 2] = b"WA";

    pub fn new() -> Self {
        let mut peer_id = [0 as u8; 20];
        let version = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        peer_id[0] = '-' as u8;
        peer_id[1..3].copy_from_slice(Self::CLIENT_ID);
        peer_id[3] = char::from_digit((version.major % 10) as u32, 10).unwrap() as u8;
        peer_id[4] = char::from_digit((version.minor % 10) as u32, 10).unwrap() as u8;
        peer_id[5..7].copy_from_slice(&format!("{:02}", version.patch).as_bytes()[..2]);
        peer_id[7..].fill_with(rand::random);
        PeerID { bytes: peer_id }
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.bytes
    }
}
