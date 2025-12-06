use std::{ io::{self, Write}, net::{ Ipv4Addr, Ipv6Addr, ToSocketAddrs, UdpSocket } };

use thiserror::Error;
use tracing_subscriber::fmt;

#[derive(Error, Debug)]
pub enum UdpTWError {
    #[error("address resolution yielded no results")]
    NoAddressFound,
    #[error(transparent)] Io(#[from] io::Error),
}

pub type Result<T, E = UdpTWError> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct UdpTracingWriter {
    addr: std::net::SocketAddr,
    sock: UdpSocket,
}

impl UdpTracingWriter {
    pub fn new<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let addr = addr.to_socket_addrs()?.next().ok_or(UdpTWError::NoAddressFound)?;
        let sock = match addr {
            std::net::SocketAddr::V4(_) => { UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))? }
            std::net::SocketAddr::V6(_) => { UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0))? }
        };
        // sock.connect(addr)?;
        Ok(UdpTracingWriter { addr, sock })
    }
}

impl<'a> fmt::MakeWriter<'a> for UdpTracingWriter {
    type Writer = &'a UdpTracingWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self
    }
}

impl Write for UdpTracingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sock.send_to(buf, self.addr)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> Write for &'a UdpTracingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sock.send_to(buf, self.addr)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn it_works() {
        // let result = add(2, 2);
        // assert_eq!(result, 4);
    }
}
