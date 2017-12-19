use std::net::TcpStream;
use std::io::{Read};
use std::slice::Iter;
use std::iter::Iterator;
use error::{Result, Error};
use uuid::Uuid;

pub const MAX_PAYLOAD_SIZE: usize = 268435455;

bitflags! {
    pub struct ConnectFlags: u8 {
        const USERNAME_FLAG = 0b10000000;
        const PASSWORD_FLAG = 0b01000000;
        const WILL_RETAIN   = 0b00100000;
        const WILL_QOS      = 0b00011000;
        const WILL_FLAG     = 0b00000100;
        const CLEAN_SESSION = 0b00000010;
    }
}

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
pub enum CtrlPkt {
    Connect {
        connect_flags: ConnectFlags,
        keep_alive: u16,
        client_id: String,
        will_topic: Option<String>,
        will_message: Option<String>,
        username: Option<String>,
        password: Option<String>
    },
    ConnAck,
    Publish,
    PubAck,
    PubRec,
    PubRel,
    PubComp,
    Subscribe,
    SubAck,
    Unsubscribe0,
    UnsubAck1,
    PingReq2,
    PingResp3,
    Disconnect4
}

impl CtrlPkt {
    pub fn deserialize(mut stream: TcpStream) -> Result<CtrlPkt> {
        let (ty, flags) = stream.read_fixed_header()?;
        match ty {
            CtrlPktType::Connect => {
                let len = stream.read_remaining_len()?;
                let data = stream.read_len(len)?;
                let mut iter = data.iter();

                let protocol = iter.read_str()?;
                if protocol != "MQTT" {
                    return Err(Error::InvalidProtocol);
                }
                let protocol_lv = iter.read_protocol_lv()?;
                if protocol_lv != 4 {
                    return Err(Error::UnacceptableProtocolLv);
                }
                let connect_flags = ConnectFlags::from_bits_truncate(iter.read_u8()?);
                let keep_alive = iter.read_u16()?;

                let mut client_id = iter.read_str()?;
                if client_id.len() == 0 {
                    if !connect_flags.contains(ConnectFlags::CLEAN_SESSION) {
                        return Err(Error::IdRejected);
                    }
                    client_id = Uuid::new_v4().hyphenated().to_string();
                };
                let (will_topic, will_message) = if connect_flags.contains(ConnectFlags::WILL_FLAG) {
                    (Some(iter.read_str()?), Some(iter.read_str()?))
                } else {
                    (None, None)
                };
                let username = if connect_flags.contains(ConnectFlags::USERNAME_FLAG) {
                    Some(iter.read_str()?)
                } else {
                    None
                };
                let password = if connect_flags.contains(ConnectFlags::PASSWORD_FLAG) {
                    Some(iter.read_str()?)
                } else {
                    None
                };
                println!("{} {} {:010b} {} {} {:?} {:?} {:?} {:?}", protocol, protocol_lv,
                    connect_flags, keep_alive, client_id, will_topic, will_message,
                    username, password);

                Ok(CtrlPkt::Connect {
                    connect_flags: connect_flags,
                    keep_alive: keep_alive,
                    client_id: client_id,
                    will_topic: will_topic,
                    will_message: will_message,
                    username: username,
                    password: password
                })
            }
            _ => Err(Error::Unimplemented)
        }
    }
}

pub trait MqttStream: Read {
    fn read_fixed_header(&mut self) -> Result<(CtrlPktType, u8)>;
    fn read_remaining_len(&mut self) -> Result<usize>;
    fn read_len(&mut self, len: usize) -> Result<Vec<u8>>;
}

pub trait MqttReadStream: Iterator {
    fn read_str(&mut self) -> Result<String>;
    fn read_protocol_lv(&mut self) -> Result<u8>;
    fn read_len(&mut self, len: usize) -> Result<Vec<u8>>;
    fn read_u8(&mut self) -> Result<u8>;
    fn read_u16(&mut self) -> Result<u16>;
}

impl MqttStream for TcpStream {
    fn read_fixed_header(&mut self) -> Result<(CtrlPktType, u8)> {
        let header = try!(self.read_len(1));
        let ty = try!(match header[0] >> 4 {
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
        let flags = header[0] & 0x0f;
        Ok((ty, flags))
    }

    fn read_remaining_len(&mut self) -> Result<usize> {
        let mut done = false;
        let mut multiplier: usize = 1;
        let mut value: usize = 0;
        while !done {
            let encoded_byte = self.read_len(1)?[0];
            value += ((encoded_byte & 127) as usize) * multiplier;
            multiplier *= 128;
            if multiplier > 128 * 128 * 128 {
                return Err(Error::MalformedRemainingLen);
            }
            done = (encoded_byte & 128) == 0;
        }
        Ok(value)
    }

    fn read_len(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0; len];
        let _ = self.read_exact(&mut buf)?;
        Ok(buf)
    }
}

impl<'a> MqttReadStream for Iter<'a, u8> {
    fn read_str(&mut self) -> Result<String> {
        let len = self.read_u16()? as usize;
        let str_buf = self.read_len(len)?;
        Ok(String::from_utf8(str_buf)?)
    }

    fn read_len(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![];
        for _ in 0..len {
            buf.push(*self.next().ok_or(Error::ReadErr)?);
        }
        Ok(buf)
    }

    fn read_protocol_lv(&mut self) -> Result<u8> {
        self.read_u8()
    }

    fn read_u8(&mut self) -> Result<u8> {
        let buf = self.read_len(1)?;
        Ok(buf[0])
    }

    fn read_u16(&mut self) -> Result<u16> {
        let msb = *self.next().ok_or(Error::ReadErr)?;
        let lsb = *self.next().ok_or(Error::ReadErr)?;
        Ok(((msb as u16) << 8) + lsb as u16)
    }
}
