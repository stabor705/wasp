use crate::peerid::PeerID;

use serde::{Deserialize, Serialize};
use serde_bencode::value::Value;
use sha1::{Digest, Sha1};
use url::Url;

use std::collections::HashMap;
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
    //private: Option<bool>,
    #[serde(rename = "meta version")]
    meta_version: Option<u32>,
}

#[derive(Deserialize, Serialize)]
struct Metainfo {
    announce: Url,
    #[serde(rename = "announce-list")]
    announce_list: Option<Vec<Url>>,
    info: Info,
    //TODO: Optional fields
}

struct File {
    path: PathBuf,
    length: i64,
}

pub struct Torrent {
    urls: Vec<Url>,
    piece_length: i64,
    pieces: Vec<[u8; 20]>,
    files: Vec<File>,
    info_hash: [u8; 20],
    peer_id: PeerID,
}

#[derive(thiserror::Error, Debug)]
pub enum TorrentFileCodingError {
    #[error("pieces value not multiple of 20")]
    InvalidPiecesLength,
    #[error("provided bencode encoding was incorrect")]
    IncorrectEncoding {
        #[from]
        source: serde_bencode::Error,
    },
}

impl Torrent {
    pub fn from_bencode(bencode: &[u8]) -> Result<Torrent, TorrentFileCodingError> {
        let metainfo = serde_bencode::from_bytes::<Metainfo>(bencode)?;

        let urls = if let Some(announce_list) = metainfo.announce_list {
            announce_list
        } else {
            vec![metainfo.announce]
        };
        if metainfo.info.pieces.len() % 20 != 0 {
            return Err(TorrentFileCodingError::InvalidPiecesLength);
        }
        let pieces: Vec<[u8; 20]> = metainfo
            .info
            .pieces
            .chunks_exact(20)
            .map(|chunk| <[u8; 20]>::try_from(chunk).unwrap())
            .collect();

        let dict = serde_bencode::from_bytes::<HashMap<String, Value>>(bencode)?;
        let info = serde_bencode::to_bytes(&dict["info"])?;
        let mut hasher = Sha1::new();
        hasher.update(info);
        let info_hash: [u8; 20] = hasher.finalize().into();

        Ok(Torrent {
            urls,
            piece_length: metainfo.info.piece_length,
            pieces,
            files: Vec::new(),
            info_hash,
            peer_id: PeerID::new(),
        })
    }
}
