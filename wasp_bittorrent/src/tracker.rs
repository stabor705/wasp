use crate::peer::Peer;
use crate::peerid::PeerID;

use anyhow::{Error, Result};
use hyper::{body, client::HttpConnector, Body, Uri};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(thiserror::Error, Debug)]
pub enum TrackerError {
    #[error("Tracker responded with error: {0}")]
    ErrorResponse(String),
    #[error("Tracker's reponse was invalid: {0}")]
    InvalidResponse(String),
    #[error("All tried announce URIs did not work")]
    DefunctUris,
}

#[derive(Default)]
pub struct Query {
    pub uris: Vec<Uri>,
    pub info_hash: [u8; 20],
    pub port: u16,
    pub uploaded: i64,
    pub downloaded: i64,
    pub left: i64,
}

pub struct QueryResult {
    pub peers: Vec<Peer>,
    pub seeders: i64,
    pub leechers: i64,
}

pub struct Client {
    client: hyper::Client<HttpConnector, Body>,
    peer_id: PeerID,
}

impl Client {
    pub fn new(peer_id: &PeerID) -> Client {
        Client {
            client: hyper::Client::new(),
            peer_id: peer_id.clone(),
        }
    }

    pub async fn query_peers(&self, query: Query) -> Result<QueryResult> {
        for uri in &query.uris {
            let uri = match self.build_uri(uri, &query) {
                Some(uri) => uri,
                None => continue,
            };
            log::info!("Trying tracker uri {}", uri);
            let response = self.client.get(uri).await?;
            //TODO: check encoding type
            let body = body::to_bytes(response.into_body()).await?; // TODO: aggregate?
            let response = serde_bencode::from_bytes::<TrackerResponse>(&body[..])?;
            let result = Self::process_response(response);
            if result.is_ok() {
                return result;
            }
        }
        Err(Error::new(TrackerError::DefunctUris))
    }

    fn build_uri(&self, uri: &Uri, query: &Query) -> Option<Uri> {
        let scheme = match uri.scheme() {
            Some(scheme) => scheme,
            None => return None,
        };
        let authority = match uri.authority() {
            Some(authority) => authority,
            None => return None,
        };

        let query = HttpQuery::new()
            .push_pair_urlencoded("info_hash", &query.info_hash)
            .push_pair_urlencoded("peer_id", self.peer_id.as_bytes())
            .push_pair("port", query.port.to_string().as_str())
            .push_pair("uploaded", query.uploaded.to_string().as_str())
            .push_pair("downloaded", query.downloaded.to_string().as_str())
            .push_pair("left", query.left.to_string().as_str())
            .push_pair("compact", "1")
            .push_pair("event", "started")
            .finalize();
        let path_and_query = format!("{}?{}", uri.path(), query);

        Some(
            Uri::builder()
                .scheme(scheme.clone())
                .authority(authority.clone())
                .path_and_query(path_and_query)
                .build()
                .unwrap(),
        )
    }

    fn process_response(response: TrackerResponse) -> Result<QueryResult> {
        match response {
            TrackerResponse::Error { failure_reason } => {
                Err(Error::new(TrackerError::ErrorResponse(failure_reason)))
            }
            TrackerResponse::Response {
                warning_message,
                interval,
                tracker_id,
                min_interval,
                complete,
                incomplete,
                peers,
            } => {
                let peers = Self::process_peers(peers)?;
                // TODO: do something with interval
                if let Some(warning) = warning_message {
                    log::warn!("Tracker warning: {}", warning);
                }
                // TODO: do something with tracker_id
                Ok(QueryResult {
                    peers,
                    seeders: complete,
                    leechers: incomplete,
                })
            }
        }
    }

    fn process_peers(tracker_peers: TrackerPeers) -> Result<Vec<Peer>> {
        let mut peers = Vec::new();
        match tracker_peers {
            TrackerPeers::DictModel(dict_peers) => {
                unimplemented!();
            }
            TrackerPeers::BinModel(bin_peers) => {
                if bin_peers.len() % 6 != 0 {
                    return Err(Error::new(TrackerError::InvalidResponse(
                        "binary peers data should be multiple of 6".to_owned(),
                    )));
                }
                for chunk in bin_peers.chunks_exact(6) {
                    let ip = Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
                    let mut port = chunk[4] as u16;
                    port = port << 8;
                    port = port + chunk[5] as u16;
                    peers.push(Peer {
                        addr: SocketAddr::new(IpAddr::V4(ip), port),
                    });
                }
            }
        }
        Ok(peers)
    }
}

struct HttpQuery {
    query: String,
}

impl HttpQuery {
    pub fn new() -> HttpQuery {
        HttpQuery {
            query: String::new(),
        }
    }

    pub fn push_pair(mut self, key: &str, value: &str) -> HttpQuery {
        self.query.push_str(key);
        self.query.push('=');
        self.query.push_str(value);
        self.query.push('&');
        self
    }

    pub fn push_pair_urlencoded(self, key: &str, value: &[u8]) -> HttpQuery {
        self.push_pair(key, &urlencoding::encode_binary(value))
    }

    pub fn finalize(mut self) -> String {
        self.query.pop();
        self.query
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TrackerResponse {
    Error {
        #[serde(rename = "failure reason")]
        failure_reason: String,
    },
    Response {
        complete: i64,
        incomplete: i64,
        interval: i64,
        peers: TrackerPeers,
        #[serde(rename = "warning message")]
        warning_message: Option<String>,
        #[serde(rename = "min interval")]
        min_interval: Option<i64>,
        #[serde(rename = "tracker id")]
        tracker_id: Option<String>,
    },
}

#[derive(Deserialize)]
struct DictPeer {
    #[serde(rename = "peer id", with = "serde_bytes")]
    peer_id: Vec<u8>,
    ip: String,
    port: i64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TrackerPeers {
    DictModel(Vec<DictPeer>),
    #[serde(with = "serde_bytes")]
    BinModel(Vec<u8>),
}
