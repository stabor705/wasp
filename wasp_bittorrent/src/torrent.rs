use crate::peer::Peer;
use crate::peerid::PeerID;
use crate::tracker::{self, Query};

use anyhow::Result;
use hyper::Uri;
use serde::{Deserialize, Serialize};
use serde_bencode::value::Value;
use sha1::{Digest, Sha1};

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

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
    //private: Option<bool>,
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
    uris: Vec<Uri>,
    piece_length: i64,
    pieces: Vec<[u8; 20]>,
    files: Vec<File>,
    info_hash: [u8; 20],
    peer_id: PeerID,
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

        let dict = serde_bencode::from_bytes::<HashMap<String, Value>>(bencode)?;
        let info = serde_bencode::to_bytes(&dict["info"])?;
        let mut hasher = Sha1::new();
        hasher.update(info);
        let info_hash: [u8; 20] = hasher.finalize().into();

        Ok(Torrent {
            uris: vec![Uri::from_str(metainfo.announce.as_str())?],
            piece_length: metainfo.info.piece_length,
            pieces,
            files: Vec::new(),
            info_hash,
            peer_id: PeerID::default(),
        })
    }

    pub async fn get_peers(&self) -> Result<Vec<Peer>> {
        let client = tracker::Client::new(&self.peer_id);
        let info_hash = [0 as u8; 20];
        let query = Query {
            uris: self.uris.clone(),
            info_hash: self.info_hash,
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: 0,
        };
        let results = client.query_peers(query).await?;
        Ok(results.peers)
    }
}
