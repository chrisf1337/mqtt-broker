use std::net::TcpStream;
use std::io::{Read, Bytes};
use error::{Result, Error};

const MAX_PAYLOAD_SIZE: usize = 268435455;

#[derive(Debug, Copy, Clone)]
pub enum CtrlPktType {
    Connect = 1,
    ConnAck = 2,
    Publish = 3,
    PubAck = 4,
    PubRec = 5,
    PubRel = 6,
    PubComp = 7,
    Subscribe = 8,
    SubAck = 9,
    Unsubscribe = 10,
    UnsubAck = 11,
    PingReq = 12,
    PingResp = 13,
    Disconnect = 14
}

#[derive(Debug)]
pub struct CtrlPkt {
    pub ty: CtrlPktType,
    pub flags: i8
}

impl CtrlPkt {
    fn deserialize(bytes: &Bytes<TcpStream>) {

    }
}

fn read_fixed_header(bytes: &mut Read) -> Result<(CtrlPktType, u8)> {
    let mut buf1: [u8; 1] = [0; 1];
    let _ = try!(bytes.read_exact(&mut buf1));
    let ty = try!(match buf1[0] >> 4 {
        1 => Ok(CtrlPktType::Connect),
        2 => Ok(CtrlPktType::ConnAck),
        3 => Ok(CtrlPktType::Publish),
        4 => Ok(CtrlPktType::PubAck),
        5 => Ok(CtrlPktType::PubRec),
        6 => Ok(CtrlPktType::PubRel),
        7 => Ok(CtrlPktType::PubComp),
        8 => Ok(CtrlPktType::Subscribe),
        9 => Ok(CtrlPktType::SubAck),
        10 => Ok(CtrlPktType::Unsubscribe),
        11 => Ok(CtrlPktType::UnsubAck),
        12 => Ok(CtrlPktType::PingReq),
        13 => Ok(CtrlPktType::PingResp),
        14 => Ok(CtrlPktType::Disconnect),
        _ => Err(Error::InvalidControlPacketType)
    });
    let flags = buf1[0] | 0x0f;
    Ok((ty, flags))
}
