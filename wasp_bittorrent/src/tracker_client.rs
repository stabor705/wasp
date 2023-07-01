use crate::peerid::PeerID;

use serde::{Deserialize, Serialize};
use url::Url;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(thiserror::Error, Debug)]
pub enum TrackerClientError {
    #[error("tracker responded with error: {0}")]
    ErrorResponse(String),
    #[error("tracker's reponse was invalid: {0}")]
    InvalidResponse(String),
    #[error("all tried announce URLs did not work")]
    DefunctUrls,
    #[error("HTTP request failed")]
    HttpError {
        #[from]
        source: reqwest::Error,
    },
    #[error("tracker's response decoding failed")]
    DecodingError {
        #[from]
        source: bendy::serde::Error,
    },
}

#[derive(Serialize)]
pub struct PeersQuery {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub port: u16,
    pub ip: IpAddr,
    pub uploaded: i64,
    pub downloaded: i64,
    pub left: i64,
    //TODO: event
}

#[derive(Deserialize)]
struct Peer {
    #[serde(with = "serde_bytes")]
    peer_id: Vec<u8>,
    ip: IpAddr,
    port: u16,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum TrackerResponse {
    Response(PeersQueryResult),
    Error {
        #[serde(rename = "failure reason")]
        failure_reason: String,
    },
}

#[derive(Deserialize)]
pub struct PeersQueryResult {
    interval: i64,
    peers: Vec<Peer>,
}

pub struct TrackerClient {}

impl TrackerClient {
    pub fn new(peer_id: &PeerID) -> TrackerClient {
        TrackerClient {}
    }

    // pub async fn fetch_peers(&self, query: PeersQuery) -> Result<PeersQueryResult> {
    //     for uri in &query.urls {
    //         let uri = match self.build_uri(uri, &query) {
    //             Some(uri) => uri,
    //             None => continue,
    //         };
    //         log::info!("Trying tracker uri {}", uri);
    //         let response = self.client.get(uri).await?;
    //         //TODO: check encoding type
    //         let body = body::to_bytes(response.into_body()).await?; // TODO: aggregate?
    //         let response = serde_bencode::from_bytes::<TrackerResponse>(&body[..])?;
    //         let result = Self::process_response(response);
    //         if result.is_ok() {
    //             return result;
    //         }
    //     }
    //     Err(Error::new(TrackerClientError::DefunctUris))
    // }

    pub async fn fetch_peers(
        &self,
        query: PeersQuery,
        urls: &[Url],
    ) -> Result<PeersQueryResult, TrackerClientError> {
        for url in urls {
            let query_string = serde_qs::to_string(&query).unwrap();
            let mut encoded_url = url.clone();
            encoded_url.set_query(Some(query_string.as_str()));
            match Self::try_fetch_peers(encoded_url).await {
                Ok(result) => return Ok(result),
                Err(error) => match error {
                    TrackerClientError::ErrorResponse(failure_reason) => {
                        return Err(TrackerClientError::ErrorResponse(failure_reason))
                    }
                    _ => {
                        continue;
                    }
                },
            }
        }
        Err(TrackerClientError::DefunctUrls)
    }

    async fn try_fetch_peers(encoded_url: Url) -> Result<PeersQueryResult, TrackerClientError> {
        let bencode = reqwest::get(encoded_url).await?.bytes().await?;
        let response: TrackerResponse = bendy::serde::from_bytes(bencode.as_ref())?;
        match response {
            TrackerResponse::Response(result) => Ok(result),
            TrackerResponse::Error { failure_reason } => {
                Err(TrackerClientError::ErrorResponse(failure_reason))
            }
        }
    }
}

// struct HttpQuery {
//     query: String,
// }

// impl HttpQuery {
//     pub fn new() -> HttpQuery {
//         HttpQuery {
//             query: String::new(),
//         }
//     }

//     pub fn push_pair(mut self, key: &str, value: &str) -> HttpQuery {
//         self.query.push_str(key);
//         self.query.push('=');
//         self.query.push_str(value);
//         self.query.push('&');
//         self
//     }

//     pub fn push_pair_urlencoded(self, key: &str, value: &[u8]) -> HttpQuery {
//         self.push_pair(key, &urlencoding::encode_binary(value))
//     }

//     pub fn finalize(mut self) -> String {
//         self.query.pop();
//         self.query
//     }
// }

// #[derive(Deserialize)]
// #[serde(untagged)]
// enum TrackerResponse {
//     Error {
//         #[serde(rename = "failure reason")]
//         failure_reason: String,
//     },
//     Response {
//         complete: i64,
//         incomplete: i64,
//         interval: i64,
//         peers: TrackerPeers,
//         #[serde(rename = "warning message")]
//         warning_message: Option<String>,
//         #[serde(rename = "min interval")]
//         min_interval: Option<i64>,
//         #[serde(rename = "tracker id")]
//         tracker_id: Option<String>,
//     },
// }

// #[derive(Deserialize)]
// struct DictPeer {
//     #[serde(rename = "peer id", with = "serde_bytes")]
//     peer_id: Vec<u8>,
//     ip: String,
//     port: i64,
// }

// #[derive(Deserialize)]
// #[serde(untagged)]
// enum TrackerPeers {
//     DictModel(Vec<DictPeer>),
//     #[serde(with = "serde_bytes")]
//     BinModel(Vec<u8>),
// }
