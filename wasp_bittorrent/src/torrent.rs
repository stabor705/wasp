use anyhow::Error;
use hyper::Uri;
use serde::{Deserialize, Serialize};
use sha1::Sha1;

use std::path::PathBuf;

#[derive(Deserialize, Serialize)]
struct MetainfoFile {
    length: i64,
    path: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Info {
    #[serde(rename = "piece length")]
    piece_length: i64,
    #[serde(with = "serde_bytes")]
    pieces: Vec<u8>,
    name: String,
    length: i64,
    files: Option<Vec<MetainfoFile>>,
    private: Option<bool>,
    #[serde(rename = "meta version")]
    meta_version: Option<u32>,
}

#[derive(Deserialize, Serialize)]
struct Metainfo {
    announce: String,
    info: Info,
    //TODO: Optional fields
}

struct File {
    path: PathBuf,
    length: i64,
}

pub struct Torrent {
    links: Vec<Uri>,
    piece_length: i64,
    pieces: Vec<[u8; 20]>,
    files: Vec<File>,
    info_hash: [u8; 20],
    peer_id: [u8; 20],
}

impl Torrent {
    pub fn from_bencode(bencode: &[u8]) -> Result<Torrent> {
        let metainfo = serde_bencode::from_bytes::<Metainfo>(bencode)?;
        let mut pieces = Vec::new();
        for chunk in metainfo.info.pieces.chunks_exact(20) {
            let mut piece = [0 as u8; 20];
            piece.copy_from_slice(chunk);
            pieces.push(piece);
        }
        //TODO: It's not very correct
        let info_bencoded = serde_bencode::to_bytes(&metainfo.info)?;
        let mut hasher = Sha1::new();
        hasher.update(&info_bencoded);
        let info_hash: [u8; 20] = hasher.finalize().into();

        Ok(Torrent {
            links: vec![Uri::from_str(metainfo.announce.as_str())?],
            piece_length: metainfo.info.piece_length,
            pieces,
            files: Vec::new(),
            info_hash,
            peer_id: Torrent::generate_peer_id(['W' as u8, 'A' as u8]),
        })
    }

    fn generate_peer_id(client_id: [u8; 2]) -> [u8; 20] {
        let mut peer_id = [0 as u8; 20];
        peer_id[0] = '-' as u8;
        peer_id[1..3].copy_from_slice(&client_id);
        //TODO: version number
        peer_id[3..7].fill(1);
        peer_id[7..].fill_with(rand::random);
        peer_id
    }

    pub async fn get_peers(&self) -> Result<()> {}
}
