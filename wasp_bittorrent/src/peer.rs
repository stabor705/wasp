use crate::peerid::PeerID;

use anyhow::{Error, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{sink::SinkExt, stream::StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_util::codec::{Decoder, Encoder, Framed};

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug)]
pub struct PeerData {
    pub addr: SocketAddr,
}

#[derive(thiserror::Error, Debug)]
pub enum PeerError {
    #[error("Peer sent and invalid handshake data")]
    InvalidHandshake,
    #[error("Peer closed connection in unexpected moment")]
    UnexpectedEof,
    #[error("Peer sent incorrect message id {0}")]
    InvalidMessageId(u8),
}

pub struct PeerManager {
    peers_data: Vec<PeerData>,
    peers: Vec<Peer>,
    info_hash: Bytes,
    peer_id: Bytes,
    port: u16,
}

impl PeerManager {
    pub fn new(
        peers: Vec<PeerData>,
        info_hash: &[u8; 20],
        peer_id: &PeerID,
        port: u16,
    ) -> PeerManager {
        PeerManager {
            peers_data: peers,
            peers: Vec::new(),
            info_hash: Bytes::copy_from_slice(info_hash),
            peer_id: Bytes::copy_from_slice(peer_id.as_bytes()),
            port,
        }
    }

    pub async fn run(&mut self) {
        let (tx, mut new_peer) = mpsc::channel(30);
        {
            let info_hash = self.info_hash.clone();
            let peer_id = self.peer_id.clone();
            let port = self.port;
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::listen(port, info_hash, peer_id, tx).await {
                    println!("{}", e);
                }
            });
        }
        {
            let tx = tx.clone();
            tokio::spawn(async move { Self::manage_connections(tx).await });
        }

        loop {
            tokio::select! {
                peer_conn = new_peer.recv() => {
                    if let Some(peer_conn) = peer_conn {
                        let peer = Peer::new(peer_conn.get_state());
                        self.peers.push(peer);
                        tokio::spawn(async move {
                            peer_conn.handle().await
                        });
                    }
                }
            }
        }
    }

    async fn listen(
        port: u16,
        info_hash: Bytes,
        peer_id: Bytes,
        chan: mpsc::Sender<PeerConnection>,
    ) -> Result<()> {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)).await?;
        loop {
            match listener.accept().await {
                Ok((conn, _)) => {
                    let info_hash = info_hash.clone();
                    let peer_id = peer_id.clone();
                    let new_tx = chan.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::accept(conn, info_hash, peer_id, new_tx).await {
                            println!("{}", e);
                        }
                    });
                }
                Err(e) => {
                    println!("{}", e);
                }
            }
        }
    }

    async fn connect(peer: &PeerData, info_hash: Bytes, peer_id: Bytes) -> Result<PeerConnection> {
        let conn = TcpStream::connect(peer.addr).await?;
        let mut stream = Framed::new(conn, HandshakeCodec{});
        let frame = HandshakeFrame::new(info_hash.clone(), None);
        stream.send(frame).await?;
        let frame: HandshakeFrame = stream.next().await.ok_or(Error::msg("Handshake Error"))??;
        if frame.info_hash != info_hash {
            return Err(Error::new(PeerError::InvalidHandshake));
        }
        let mut conn = stream.into_inner();
        conn.write_all_buf(&mut peer_id.clone()).await?;
        conn.flush().await?;
        Ok(PeerConnection::new(conn))
    }

    async fn accept(
        mut conn: TcpStream,
        info_hash: Bytes,
        peer_id: Bytes,
        chan: mpsc::Sender<PeerConnection>,
    ) -> Result<()> {
        let mut stream = Framed::new(conn, HandshakeCodec {});
        let frame = stream.next().await.ok_or(Error::msg("Handshake error"))??;
        if frame.info_hash != info_hash {
            return Err(Error::new(PeerError::InvalidHandshake));
        }
        let frame = HandshakeFrame::new(info_hash.clone(), Some(peer_id.clone()));
        stream.send(frame).await?;
        let mut conn = stream.into_inner();
        let mut peer_id = BytesMut::with_capacity(20);
        loop {
            let n = conn.read_buf(&mut peer_id).await?;
            if n == 0 {
                return Err(Error::new(PeerError::UnexpectedEof));
            }
            if !peer_id.has_remaining() {
                break;
            }
        }
        chan.send(PeerConnection::new(conn)).await;
        Ok(())
    }

    async fn manage_connections(chan: mpsc::Sender<PeerConnection>) {
        loop {
            sleep(Duration::from_secs(10)).await;
        }
    }
}

#[derive(Debug)]
struct HandshakeFrame {
    pub pstr: Bytes,
    pub reserved: Bytes,
    pub info_hash: Bytes,
    pub peer_id: Option<Bytes>,
}

impl HandshakeFrame {
    const PSTR: &'static [u8; 19] = b"BitTorrent protocol";

    pub fn new(info_hash: Bytes, peer_id: Option<Bytes>) -> HandshakeFrame {
        HandshakeFrame {
            pstr: Bytes::from_static(Self::PSTR),
            reserved: Bytes::from_static(b"\0\0\0\0\0\0\0\0"),
            info_hash,
            peer_id: peer_id.and_then(|peer_id| Some(peer_id)),
        }
    }
}

struct HandshakeCodec {}

impl Decoder for HandshakeCodec {
    type Item = HandshakeFrame;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if !src.has_remaining() {
            return Ok(None);
        }
        let pstrlen = src.get_u8() as usize;
        if src.remaining() == pstrlen + 8 + 20 {
            return Ok(None);
        }
        if src.remaining() < pstrlen + 8 + 20 + 20 {
            return Ok(None);
        }
        let pstr = src.split_to(pstrlen).freeze();
        let reserved = src.split_to(8).freeze();
        let info_hash = src.split_to(20).freeze();
        let peer_id = if src.remaining() >= 20 {
            Some(src.split_to(20).freeze())
        } else {
            None
        };
        Ok(Some(HandshakeFrame {
            pstr,
            reserved,
            info_hash,
            peer_id,
        }))
    }
}

impl Encoder<HandshakeFrame> for HandshakeCodec {
    type Error = Error;
    fn encode(&mut self, item: HandshakeFrame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.reserve(item.pstr.len() + 48);
        dst.put_u8(item.pstr.len() as u8);
        dst.put(item.pstr);
        dst.put(item.reserved);
        dst.put(item.info_hash);
        if let Some(peer_id) = item.peer_id {
            dst.put(peer_id);
        }
        Ok(())
    }
}

struct PeerState {
    pub am_choking: bool,
    pub am_interested: bool,
    pub peer_choking: bool,
    pub peer_interested: bool,
    pub peer_bitfield: Bytes,
}

impl PeerState {
    pub fn new() -> PeerState {
        PeerState {
            am_choking: true,
            am_interested: false,
            peer_choking: true,
            peer_interested: false,
            peer_bitfield: Bytes::new(),
        }
    }
}

struct PeerConnection {
    conn: TcpStream,
    state: Arc<Mutex<PeerState>>,
}

impl PeerConnection {
    // TODO: Change PeerConnection creation logic.
    // Valid PeerConnection object should be only a result of successful handshake
    pub fn new(conn: TcpStream) -> PeerConnection {
        PeerConnection {
            conn,
            state: Arc::new(Mutex::new(PeerState::new())),
        }
    }

    pub fn get_state(&self) -> Arc<Mutex<PeerState>> {
        self.state.clone()
    }

    pub async fn handle(&self) {
        loop {
        }
    }
}

struct Peer {
    state: Arc<Mutex<PeerState>>,
    uploaded: u64,
    downloaded: u64,
}

impl Peer {
    pub fn new(state: Arc<Mutex<PeerState>>) -> Peer {
        Peer {
            state,
            uploaded: 0,
            downloaded: 0,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have {
        piece_index: u32,
    },
    Bitfield {
        bitfield: Bytes,
    },
    Request {
        index: u32,
        begin: u32,
        length: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        block: Bytes,
    },
    Cancel {
        index: u32,
        begin: u32,
        length: u32,
    },
    Port {
        listen_port: u16,
    },
}

struct MessageCodec {}

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.remaining() < 4 {
            return Ok(None);
        }
        let len = src.get_u32() as usize;
        if len == 0 {
            return Ok(Some(Message::KeepAlive));
        }
        let id = src.get_u8();
        let msg = match id {
            0 => Message::Choke,
            1 => Message::Unchoke,
            2 => Message::Interested,
            3 => Message::NotInterested,
            4 => Message::Have {
                piece_index: src.get_u32(),
            },
            5 => Message::Bitfield {
                bitfield: src.split_to(len - 1).freeze(),
            },
            6 => Message::Request {
                index: src.get_u32(),
                begin: src.get_u32(),
                length: src.get_u32(),
            },
            7 => Message::Piece {
                index: src.get_u32(),
                begin: src.get_u32(),
                block: src.split_to(len - 9).freeze(),
            },
            8 => Message::Cancel {
                index: src.get_u32(),
                begin: src.get_u32(),
                length: src.get_u32(),
            },
            9 => Message::Port {
                listen_port: src.get_u16(),
            },
            _ => return Err(Error::new(PeerError::InvalidMessageId(id))),
        };
        Ok(Some(msg))
    }
}

impl Encoder<Message> for MessageCodec {
    type Error = Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            Message::KeepAlive => dst.extend_from_slice(&[0, 0, 0, 0]),
            Message::Choke => dst.extend_from_slice(&[0, 0, 0, 1, 0]),
            Message::Unchoke => dst.extend_from_slice(&[0, 0, 0, 1, 1]),
            Message::Interested => dst.extend_from_slice(&[0, 0, 0, 1, 2]),
            Message::NotInterested => dst.extend_from_slice(&[0, 0, 0, 1, 3]),
            Message::Have { piece_index } => {
                dst.put_u32(5);
                dst.put_u8(4);
                dst.put_u32(piece_index);
            }
            Message::Bitfield { bitfield } => {
                dst.put_u32(1 + bitfield.len() as u32);
                dst.put_u8(5);
                dst.put(bitfield);
            }
            Message::Request {
                index,
                begin,
                length,
            } => {
                dst.put_u8(6);
                dst.put_u32(index);
                dst.put_u32(begin);
                dst.put_u32(length);
            }
            Message::Piece {
                index,
                begin,
                block,
            } => {
                dst.put_u32(9 + block.len() as u32);
                dst.put_u8(7);
                dst.put_u32(index);
                dst.put_u32(begin);
                dst.put(block);
            }
            Message::Cancel {
                index,
                begin,
                length,
            } => {
                dst.put_u32(13);
                dst.put_u8(8);
                dst.put_u32(index);
                dst.put_u32(begin);
                dst.put_u32(length);
            }
            Message::Port { listen_port } => {
                dst.put_u32(3);
                dst.put_u8(9);
                dst.put_u16(listen_port);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::codec::{FramedRead, FramedWrite};

    #[tokio::test]
    async fn test_handshake_deserializing() {
        let info_hash = Bytes::from_static(b"qwertyuiop;lkjhgffds");
        let peer_id = Bytes::from_static(b"zxcvbnm,.;loikyhrfed");
        let mut buf = BytesMut::with_capacity(49 + 19);
        buf.put_u8(HandshakeFrame::PSTR.len() as u8);
        buf.put(&HandshakeFrame::PSTR[..]);
        buf.put_u64(0);
        buf.put(info_hash.clone());
        buf.put(peer_id.clone());
        let mut stream = FramedRead::new(&buf[..], HandshakeCodec{});
        let frame = stream.next().await.unwrap().unwrap();
        assert_eq!(frame.pstr.len(), 19);
        assert_eq!(&frame.pstr[..], HandshakeFrame::PSTR);
        assert_eq!(frame.info_hash, info_hash);
        assert_eq!(frame.peer_id, Some(peer_id));
    }

    #[tokio::test]
    async fn test_handshake_serializing() {
        let info_hash = Bytes::from_static(b"qwertyuiop;lkjhgffds");
        let mut buf = BytesMut::with_capacity(49 + 19);
        buf.put_u8(HandshakeFrame::PSTR.len() as u8);
        buf.put(&HandshakeFrame::PSTR[..]);
        buf.put_u64(0);
        buf.put(info_hash.clone());
        let frame = HandshakeFrame::new(info_hash, None);
        let mut stream = FramedWrite::new(Vec::new(), HandshakeCodec{});
        stream.send(frame).await.unwrap();
        let serialized = stream.into_inner();
        assert_eq!(buf, &serialized);
    }
}
