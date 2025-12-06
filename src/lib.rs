pub mod message;
pub mod server;
mod con_info;
mod sockets;
mod digester;

pub use message::JuMessage;
pub use con_info::ConnectionInfo;

#[derive(Debug, thiserror::Error)]
pub enum JuError {
    #[error("UTF-8 Error: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),

    #[error("Json Error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Zmq Error: {0}")]
    ZmqError(#[from] zeromq::ZmqError),

    #[error("Malformed Jupyter Message: {0}")]
    MalformedMessage(String),

    #[error("Unknown Digest: {0}")]
    UnknownDigest(String),

    #[error("execute_request has no code: {0}")]
    NoCode(String),
}

pub type JuResult<T> = std::result::Result<T, JuError>;

pub const DELIMITER: &[u8] = b"<IDS|MSG>";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
    }
}
