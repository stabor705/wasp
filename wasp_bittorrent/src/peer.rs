use crate::peerid::PeerID;

use anyhow::{Error, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use std::io::Cursor;
use std::net::SocketAddr;

#[derive(Debug)]
pub struct PeerData {
    pub addr: SocketAddr,
}

#[derive(thiserror::Error, Debug)]
pub enum PeerError {
    #[error("Peer sent and invalid handshake data: {0:?}")]
    InvalidHandshake(HandshakeFrame),
    #[error("Peer closed connection in unexpected moment")]
    UnexpectedEof,
    #[error("Peer sent incorrect message id {0}")]
    InvalidMessageId(u8),
}

pub struct PeerManager {
    peers: Vec<PeerData>,
    connections: Vec<PeerConnection>,
    info_hash: [u8; 20],
    peer_id: PeerID,
}

impl PeerManager {
    pub fn new(peers: Vec<PeerData>, info_hash: [u8; 20], peer_id: PeerID) -> PeerManager {
        PeerManager {
            peers,
            connections: Vec::new(),
            info_hash,
            peer_id,
        }
    }

    pub async fn run() {}

    async fn new_connection(&self, peer: &PeerData) -> Result<()> {
        let connection = TcpStream::connect(peer.addr).await?;
        self.handshake(connection).await?;
        Ok(())
    }

    async fn handshake(&self, mut conn: TcpStream) -> Result<PeerConnection> {
        let handshake_frame = HandshakeFrame::new(&self.info_hash, None);
        conn.write_all_buf(&mut handshake_frame.as_bytes()).await?;
        conn.flush().await?;

        let mut buf = BytesMut::with_capacity((49 + 19) * 2);
        loop {
            let n = conn.read_buf(&mut buf).await?;
            if n == 0 {
                return Err(Error::new(PeerError::UnexpectedEof));
            }
            if HandshakeFrame::check(&buf[..]) {
                let buf = buf.freeze();
                let handshake_frame = HandshakeFrame::from_bytes(buf);
                if !handshake_frame.validate(&self.info_hash) {
                    return Err(Error::new(PeerError::InvalidHandshake(handshake_frame)));
                }
                break;
            }
        }
        conn.write_all(self.peer_id.as_bytes()).await?;
        Ok(PeerConnection::new(conn))
    }
}

#[derive(Debug)]
struct HandshakeFrame {
    pstr: Bytes,
    reserved: Bytes,
    info_hash: Bytes,
    peer_id: Option<Bytes>,
}

impl HandshakeFrame {
    const PSTR: &'static [u8] = b"BitTorrent protocol";

    pub fn new(info_hash: &[u8; 20], peer_id: Option<&[u8]>) -> HandshakeFrame {
        HandshakeFrame {
            pstr: Bytes::from_static(Self::PSTR),
            reserved: Bytes::from_static(b"\0\0\0\0\0\0\0\0"),
            info_hash: Bytes::copy_from_slice(info_hash),
            peer_id: peer_id.and_then(|peer_id| Some(Bytes::copy_from_slice(peer_id))),
        }
    }

    pub fn from_bytes(mut buf: Bytes) -> HandshakeFrame {
        let pstrlen = buf.get_u8() as usize;
        let pstr = buf.split_to(pstrlen);
        let reserved = buf.split_to(8);
        let info_hash = buf.split_to(20);
        let peer_id = if buf.remaining() >= 20 {
            Some(buf.split_to(20))
        } else {
            None
        };
        HandshakeFrame {
            pstr,
            reserved,
            info_hash,
            peer_id,
        }
    }

    pub fn as_bytes(&self) -> Box<dyn Buf> {
        let mut buf = BytesMut::with_capacity(1);
        buf.put_u8(self.pstr.len() as u8);
        let chain = buf
            .chain(self.pstr.clone())
            .chain(self.reserved.clone())
            .chain(self.info_hash.clone());
        if let Some(peer_id) = &self.peer_id {
            Box::new(chain.chain(peer_id.clone()))
        } else {
            Box::new(chain)
        }
    }

    pub fn check(buf: &[u8]) -> bool {
        let mut cursor = Cursor::new(buf);
        let pstrlen = if buf.has_remaining() {
            cursor.get_u8() as usize
        } else {
            return false;
        };
        buf.remaining() >= pstrlen + 20 + 8
    }

    pub fn validate(&self, info_hash: &[u8; 20]) -> bool {
        if self.pstr != Self::PSTR {
            return false;
        }
        if &self.info_hash[..] != info_hash {
            return false;
        }
        true
    }
}

struct PeerConnection {
    conn: TcpStream,
    am_choking: bool,
    am_interested: bool,
    peer_choking: bool,
    peer_interested: bool,
}

impl PeerConnection {
    pub fn new(conn: TcpStream) -> PeerConnection {
        PeerConnection {
            conn,
            am_choking: true,
            am_interested: false,
            peer_choking: true,
            peer_interested: false,
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

impl Message {
    pub fn check(buf: &[u8]) -> bool {
        let mut cursor = Cursor::new(buf);
        let len = if cursor.remaining() >= 4 {
            cursor.get_u32() as usize
        } else {
            return false;
        };
        buf.remaining() >= len
    }

    pub fn from_bytes(mut buf: BytesMut) -> Result<(Message, BytesMut)> {
        let len = buf.get_u32() as usize;
        let rest = buf.split_off(len);
        if len == 0 {
            return Ok((Message::KeepAlive, rest));
        }
        let id = buf.get_u8();
        let msg = match id {
            0 => Message::Choke,
            1 => Message::Unchoke,
            2 => Message::Interested,
            3 => Message::NotInterested,
            4 => Message::Have {
                piece_index: buf.get_u32(),
            },
            5 => Message::Bitfield {
                bitfield: buf.freeze(),
            },
            6 => Message::Request {
                index: buf.get_u32(),
                begin: buf.get_u32(),
                length: buf.get_u32(),
            },
            7 => Message::Piece {
                index: buf.get_u32(),
                begin: buf.get_u32(),
                block: buf.freeze(),
            },
            8 => Message::Cancel {
                index: buf.get_u32(),
                begin: buf.get_u32(),
                length: buf.get_u32(),
            },
            9 => Message::Port {
                listen_port: buf.get_u16(),
            },
            _ => return Err(Error::new(PeerError::InvalidMessageId(id))),
        };
        Ok((msg, rest))
    }

    pub fn as_bytes(&self) -> Box<dyn Buf> {
        match &self {
            Message::KeepAlive => Box::new(Bytes::from_static(&[0, 0, 0, 0])),
            Message::Choke => Box::new(Bytes::from_static(&[0, 0, 0, 1, 0])),
            Message::Unchoke => Box::new(Bytes::from_static(&[0, 0, 0, 1, 1])),
            Message::Interested => Box::new(Bytes::from_static(&[0, 0, 0, 1, 2])),
            Message::NotInterested => Box::new(Bytes::from_static(&[0, 0, 0, 1, 3])),
            Message::Have { piece_index } => {
                let mut buf = BytesMut::with_capacity(5 + 4);
                buf.put_u32(5);
                buf.put_u8(4);
                buf.put_u32(*piece_index);
                Box::new(buf.freeze())
            }
            Message::Bitfield { bitfield } => {
                let mut buf = BytesMut::with_capacity(4 + 1);
                buf.put_u32(1 + bitfield.len() as u32);
                buf.put_u8(5);
                Box::new(buf.chain(bitfield.clone()))
            }
            Message::Request {
                index,
                begin,
                length,
            } => {
                let mut buf = BytesMut::with_capacity(4 + 13);
                buf.put_u8(6);
                buf.put_u32(*index);
                buf.put_u32(*begin);
                buf.put_u32(*length);
                Box::new(buf.freeze())
            }
            Message::Piece {
                index,
                begin,
                block,
            } => {
                let mut buf = BytesMut::with_capacity(4 + 9);
                buf.put_u32(9 + block.len() as u32);
                buf.put_u8(7);
                buf.put_u32(*index);
                buf.put_u32(*begin);
                Box::new(buf.chain(block.clone()))
            }
            Message::Cancel {
                index,
                begin,
                length,
            } => {
                let mut buf = BytesMut::with_capacity(4 + 13);
                buf.put_u32(13);
                buf.put_u8(8);
                buf.put_u32(*index);
                buf.put_u32(*begin);
                buf.put_u32(*length);
                Box::new(buf.freeze())
            }
            Message::Port { listen_port } => {
                let mut buf = BytesMut::with_capacity(4 + 3);
                buf.put_u32(3);
                buf.put_u8(9);
                buf.put_u16(*listen_port);
                Box::new(buf.freeze())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hanshake_parsing() {
        let info_hash = b"asdfghjkloiuytrewqas";
        let peer_id = b"qwertyuioplkjhgfdsaz";
        let mut handshake_frame = BytesMut::with_capacity(49 + 19);
        handshake_frame.put_u8(19);
        handshake_frame.put(HandshakeFrame::PSTR);
        handshake_frame.put_u64(0);
        handshake_frame.put(&info_hash[..]);
        handshake_frame.put(&peer_id[..]);
        assert!(HandshakeFrame::check(&handshake_frame));
        let handshake_frame = HandshakeFrame::from_bytes(Bytes::copy_from_slice(&handshake_frame));
        assert!(handshake_frame.validate(info_hash));
        assert_eq!(
            handshake_frame.pstr,
            Bytes::from_static(HandshakeFrame::PSTR)
        );
        assert_eq!(
            handshake_frame.peer_id,
            Some(Bytes::from_static(peer_id))
        );
    }

    #[test]
    fn test_handshake_serializing() {
        let info_hash = b"qwertyuioplkjhgfdsaz";
        let handshake_frame = HandshakeFrame::new(info_hash, None);
        let mut expected = BytesMut::with_capacity(29 + 19);
        expected.put_u8(19);
        expected.put(HandshakeFrame::PSTR);
        expected.put_u64(0);
        expected.put(Bytes::from_static(info_hash));
        assert_eq!(handshake_frame.as_bytes().copy_to_bytes(29 + 19), expected);
    }

    #[test]
    fn test_message_parsing() {
        let msg = Message::Piece {
            index: 16,
            begin: 13,
            block: Bytes::from_static(&[1, 2, 3]),
        };
        let mut expected = BytesMut::with_capacity(16);
        expected.put_u32(12);
        expected.put_u8(7);
        expected.put_u32(16);
        expected.put_u32(13);
        expected.put(Bytes::from_static(&[1, 2, 3]));
        assert_eq!(msg.as_bytes().copy_to_bytes(16), expected);
    }

    #[test]
    fn test_message_serializing() {
        let mut buf = BytesMut::with_capacity(9);
        buf.put_u32(5);
        buf.put_u8(4);
        buf.put_u32(123);
        let msg = Message::Have { piece_index: 123 };
        assert_eq!(Message::from_bytes(buf).unwrap().0, msg);
    }
}
