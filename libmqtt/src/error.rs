use std::result;
use std::io;
use std::string;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    PayloadTooLong,
    InvalidControlPacketType,
    MalformedRemainingLen,
    FromUtf8Err,
    MalformedUtf8Str,
    ReadErr,
    InvalidProtocol,
    UnacceptableProtocolLv,
    IdRejected,
    InvalidWillRetain,
    Unimplemented,

    CloseNetworkConn,

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
