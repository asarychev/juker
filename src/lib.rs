pub mod message;
pub mod server;
mod con_info;
mod sockets;
mod digester;
mod api;
mod shell_processor;
mod server_id;

pub use message::JuMessage;
pub use con_info::ConnectionInfo;
pub use api::{JuKernel, JuKernelInfo, JuHelpLink};

#[derive(Debug, thiserror::Error)]
pub enum JuError {
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),

    #[error(transparent)]
    JsonError(#[from] serde_json::Error),

    #[error(transparent)]
    ZmqError(#[from] zeromq::ZmqError),

    #[error(transparent)]
    TryLockError(#[from] tokio::sync::TryLockError),

    #[error(transparent)]
    JoinError(#[from] tokio::task::JoinError),

    #[error("Unsupported Jupyter Message type: {0}")]
    UnsupportedMessageType(String),

    #[error("Malformed Jupyter Message: {0}")]
    MalformedMessage(String),

    #[error("Unknown Digest: {0}")]
    UnknownDigest(String),

    #[error("execute_request has no code: {0}")]
    NoCode(String),

    #[error("Juker encountered an error: {0}")]
    GeneralJukerError(String),
}

pub type JuResult<T> = std::result::Result<T, JuError>;

pub(crate) const DELIMITER: &[u8] = b"<IDS|MSG>";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
    }
}
