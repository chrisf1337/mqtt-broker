use std::net::TcpStream;
use std::io::{Read, Write};
use std::slice::Iter;
use std::iter::Iterator;
use std::sync::{Mutex, Arc};
use std::u16;
use error::{Result, Error};
use uuid::Uuid;
use pktid::PktIdGen;
use self::CtrlPkt::*;

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

bitflags! {
    pub struct ConnAckFlags: u8 {
        const SESSION_PRESENT = 0b00000001;
    }
}

bitflags! {
    pub struct PublishFlags: u8 {
        const DUP    = 0b1000;
        const QOS_LV = 0b0110;
        const RETAIN = 0b0001;
    }
}

#[derive(Debug, Copy, Clone)]
pub enum ConnAckRetCode {
    Accepted = 0,
    UnacceptableProtocolVer = 1,
    IdRejected = 2,
    ServerUnavailable = 3,
    BadUsernameOrPassword = 4,
    NotAuthorized = 5
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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum QosLv {
    AtMostOnce = 0,
    AtLeastOnce = 1,
    ExactlyOnce = 2
}

impl QosLv {
    pub fn from_int(i: u8) -> Result<QosLv> {
        match i {
            0 => Ok(QosLv::AtMostOnce),
            1 => Ok(QosLv::AtLeastOnce),
            2 => Ok(QosLv::ExactlyOnce),
            _ => Err(Error::InvalidQosLv)
        }
    }
}

#[derive(Debug, Clone)]
pub enum CtrlPkt {
    Connect {
        connect_flags: ConnectFlags,
        keep_alive: u16,
        client_id: String,
        will_topic: Option<String>,
        will_message: Option<Vec<u8>>,
        username: Option<String>,
        password: Option<Vec<u8>>
    },
    ConnAck { session_present: bool, return_code: ConnAckRetCode },
    Publish {
        dup: bool,
        qos_lv: QosLv,
        retain: bool,
        topic_name: String,
        pkt_id: Option<u16>,
        payload: Vec<u8>
    },
    PubAck(u16),
    PubRec(u16),
    PubRel,
    PubComp,
    Subscribe { id: u16, subscriptions: Vec<(String, QosLv)> },
    SubAck,
    Unsubscribe,
    UnsubAck,
    PingReq,
    PingResp,
    Disconnect
}

impl CtrlPkt {
    pub fn deserialize(stream: &mut TcpStream) -> Result<CtrlPkt> {
        let (ty, flags) = stream.read_header()?;
        let remaining_len = stream.read_remaining_len()?;
        let data = stream.read_len(remaining_len)?;
        let mut iter = data.iter();
        match ty {
            CtrlPktType::Connect => {
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
                    (Some(iter.read_str()?), Some(iter.read_len_data()?))
                } else {
                    (None, None)
                };
                let username = if connect_flags.contains(ConnectFlags::USERNAME_FLAG) {
                    Some(iter.read_str()?)
                } else {
                    None
                };
                let password = if connect_flags.contains(ConnectFlags::PASSWORD_FLAG) {
                    Some(iter.read_len_data()?)
                } else {
                    None
                };
                Ok(Connect { connect_flags, keep_alive, client_id, will_topic, will_message,
                    username, password })
            }
            CtrlPktType::Publish => {
                let flags = PublishFlags::from_bits_truncate(flags);
                let dup = flags.contains(PublishFlags::DUP);
                let qos_lv = QosLv::from_int((flags | PublishFlags::QOS_LV).bits())?;
                let retain = flags.contains(PublishFlags::RETAIN);
                let (topic_name, len) = iter.read_str_get_len()?;
                let pkt_id = if qos_lv == QosLv::AtLeastOnce || qos_lv == QosLv::ExactlyOnce {
                    Some(iter.read_u16()?)
                } else {
                    None
                };
                let payload_len = remaining_len - (len as usize + 2);
                let payload = iter.read_len(payload_len)?;
                Ok(Publish { dup, qos_lv, retain, topic_name, pkt_id, payload })
            }
            CtrlPktType::Subscribe => {
                if flags != 0b0010 {
                    return Err(Error::InvalidFixedHeaderFlags);
                }
                // TODO: Wildcard topics
                let pkt_id = iter.read_u16()?;
                Ok(Subscribe { id: 0, subscriptions: vec![] })
            }
            CtrlPktType::PingReq => Ok(PingReq),
            CtrlPktType::Disconnect => Ok(Disconnect),
            pkt_type => Err(Error::UnimplementedPktType(pkt_type))
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut buf = vec![];
        match self {
            &ConnAck { .. } | &PingResp | &PubAck(..) | &PubRec(..) => {
                buf.write_header(self)?;
                Ok(buf)
            }
            pkt => Err(Error::UnimplementedPkt(pkt.clone()))
        }
    }
}

pub trait MqttWrite: Write {
    fn write_header(&mut self, pkt: &CtrlPkt) -> Result<()>;
    fn write_remaining_len(&mut self, len: usize) -> Result<()>;
    fn write_u8(&mut self, i: u8) -> Result<()>;
    fn write_u16(&mut self, i: u16) -> Result<()>;
    fn write_str(&mut self, s: &str) -> Result<()>;
}

impl MqttWrite for Vec<u8> {
    fn write_header(&mut self, pkt: &CtrlPkt) -> Result<()> {
        match pkt {
            &ConnAck { session_present, return_code } => {
                self.write_u8((CtrlPktType::ConnAck as u8) << 4)?;
                self.write_remaining_len(2)?;
                self.write_u8(session_present as u8)?;
                self.write_u8(return_code as u8)
            }
            &PingResp => {
                self.write_u8((CtrlPktType::PingResp as u8) << 4)?;
                self.write_remaining_len(0)
            }
            &Publish { dup, qos_lv, retain, ref topic_name, pkt_id, ref payload } => {
                let mut low_bits = PublishFlags::empty();
                if retain {
                    low_bits |= PublishFlags::RETAIN;
                }
                low_bits |= PublishFlags::from_bits_truncate((qos_lv as u8) << 1);
                if dup {
                    low_bits |= PublishFlags::DUP;
                }
                self.write_u8(((CtrlPktType::Publish as u8) << 4) + PublishFlags::bits(&low_bits))?;

                let topic_name_len = topic_name.as_bytes().len() + 2;
                let mut remaining_len = topic_name_len + payload.len();
                // Add 2 for packet id if it exists
                if pkt_id.is_some() {
                    remaining_len += 2;
                }
                self.write_remaining_len(remaining_len)?;
                self.write_str(topic_name)?;
                if pkt_id.is_some() {
                    self.write_u16(pkt_id.unwrap())?;
                }
                Ok(self.write_all(&payload)?)
            }
            &PubAck(id) => {
                self.write_u8((CtrlPktType::PubAck as u8) << 4)?;
                self.write_remaining_len(2)?;
                self.write_u16(id)
            }
            &PubRec(id) => {
                self.write_u8((CtrlPktType::PubRec as u8) << 4)?;
                self.write_remaining_len(2)?;
                self.write_u16(id)
            }
            pkt => Err(Error::UnimplementedPkt(pkt.clone()))
        }
    }

    fn write_remaining_len(&mut self, mut len: usize) -> Result<()> {
        let mut done = false;
        while !done {
            let mut encoded_byte = (len % 128) as u8;
            len /= 128;
            if len > 0 {
                encoded_byte = encoded_byte | 128;
            }
            self.write_u8(encoded_byte)?;
            done = len == 0;
        }
        Ok(())
    }

    fn write_u8(&mut self, i: u8) -> Result<()> {
        Ok(self.write_all(&[i])?)
    }

    fn write_u16(&mut self, i: u16) -> Result<()> {
        let msb = ((i | 0xff00) >> 4) as u8;
        let lsb = (i | 0x00ff) as u8;
        self.write_u8(msb)?;
        self.write_u8(lsb)
    }

    fn write_str(&mut self, s: &str) -> Result<()> {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len > (u16::MAX as usize) {
            return Err(Error::StrTooLong);
        }
        self.write_u16(len as u16)?;
        Ok(self.write_all(bytes)?)
    }
}

pub trait MqttRead: Read {
    fn read_header(&mut self) -> Result<(CtrlPktType, u8)>;
    fn read_remaining_len(&mut self) -> Result<usize>;
    fn read_len(&mut self, len: usize) -> Result<Vec<u8>>;
}

pub trait MqttReadIterator: Iterator {
    fn read_str(&mut self) -> Result<String>;
    fn read_str_get_len(&mut self) -> Result<(String, u16)>;
    fn read_protocol_lv(&mut self) -> Result<u8>;
    fn read_len(&mut self, len: usize) -> Result<Vec<u8>>;
    fn read_len_data(&mut self) -> Result<Vec<u8>>;
    fn read_u8(&mut self) -> Result<u8>;
    fn read_u16(&mut self) -> Result<u16>;
}

impl MqttRead for TcpStream {
    fn read_header(&mut self) -> Result<(CtrlPktType, u8)> {
        let header = try!(self.read_len(1));
        println!("header: {:#010b}", header[0]);
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
            i => Err(Error::InvalidControlPacketType(i))
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

impl<'a> MqttReadIterator for Iter<'a, u8> {
    fn read_str(&mut self) -> Result<String> {
        Ok(self.read_str_get_len()?.0)
    }

    fn read_str_get_len(&mut self) -> Result<(String, u16)> {
        let len = self.read_u16()?;
        let str_buf = self.read_len(len as usize)?;
        Ok((String::from_utf8(str_buf)?, len + 2))
    }

    fn read_len(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![];
        for _ in 0..len {
            buf.push(*self.next().ok_or(Error::ReadErr)?);
        }
        Ok(buf)
    }

    fn read_len_data(&mut self) -> Result<Vec<u8>> {
        let len = self.read_u16()?;
        self.read_len(len as usize)
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
