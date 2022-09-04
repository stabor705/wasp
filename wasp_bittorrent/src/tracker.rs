use anyhow::Result;
use hyper::{body, client::HttpConnector, Body, Uri};
use serde::Deserialize;

use std::fmt::Display;
use std::net::SocketAddr;

#[derive(Debug)]
struct TrackerError {
    reason: String,
}

impl Display for TrackerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tracker responded with error: {}", self.reason)
    }
}

impl std::error::Error for TrackerError {}

pub struct Client {
    client: hyper::Client<HttpConnector, Body>,
    peer_id: [u8; 20],
}

impl Client {
    pub fn new(peer_id: &[u8; 20]) -> Client {
        Client {
            client: hyper::Client::new(),
            peer_id: peer_id.clone(),
        }
    }

    pub async fn query_peers(&self, uris: &Vec<Uri>) -> Result<Vec<Peer>> {
        for uri in uris {
            let uri = match self.build_uri(uri) {
                Some(uri) => uri,
                None => continue,
            };
            let response = self.client.get(uri).await?;
            //TODO: check encoding type
            let body = body::to_bytes(response.into_body()).await?; // TODO: aggregate?
            let response = serde_bencode::from_bytes::<TrackerResponse>(&body[..])?;
        }
        Ok(())
    }

    fn build_uri(&self, uri: &Uri) -> Option<Uri> {
        if uri.scheme() != http::uri::Scheme::HTTP {
            return None;
        }
        let authority = match uri.authority() {
            Some(authority) => authority,
            None => return None,
        };
        let query = HttpQuery::new()
            .push_pair_urlencoded("info_hash", &self.info_hash)
            .push_pair_urlencoded("peer_id", &self.peer_id)
            .finalize();
        let path_and_query = format!("{}?{}", uri.path(), query);
        Some(
            Uri::builder()
                .scheme("http")
                .authority(authority)
                .path_and_query(path_and_query)
                .build()
                .unwrap(),
        )
    }
}

pub struct Peer {
    addr: SocketAddr,
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
struct TrackerResponse {
    #[serde(rename = "failure reason")]
    failure_reason: String,
    #[serde(rename = "warning message")]
    warning_message: String,
    interval: i64,
    // min_interval,
    #[serde(rename = "tracker id", with = "serde_bytes")]
    tracker_id: Vec<u8>,
    complete: i64,
    incomplete: i64,
    //TODO: Handle both variants
    peers: Vec<TrackerPeer>,
}

#[derive(Deserialize)]
struct TrackerPeer {
    #[serde(rename = "peer id")]
    peer_id: Vec<u8>,
    ip: String,
    port: i64,
}
