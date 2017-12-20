use std::result;
use std::io;
use std::string;
use ctrlpkt::{CtrlPkt, CtrlPktType};

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    PayloadTooLong,
    InvalidControlPacketType(u8),
    MalformedRemainingLen,
    FromUtf8Err,
    MalformedUtf8Str,
    StrTooLong,
    ReadErr,
    InvalidProtocol,
    UnacceptableProtocolLv,
    IdRejected,
    InvalidWillRetain,
    InvalidQosLv,
    InvalidFixedHeaderFlags,

    UnimplementedPkt(CtrlPkt),
    UnimplementedPktType(CtrlPktType),

    Io(io::Error)
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<string::FromUtf8Error> for Error {
    fn from(_: string::FromUtf8Error) -> Error {
        Error::FromUtf8Err
    }
}
