use tracing::info;
use tracing::trace;
use zeromq::{ Socket, SocketRecv, SocketSend };

use crate::{ ConnectionInfo, JuMessage, JuResult, digester::Digester };

pub(crate) struct HBSocket<S> {
    sock: S,
    port: u16,
}

impl<S: Socket + SocketRecv> HBSocket<S> {
    pub(crate) async fn recv(&mut self) -> JuResult<JuMessage> {
        let msg = self.sock.recv().await?.try_into()?;
        Ok(msg)
    }
}

impl<S: Socket + SocketSend> HBSocket<S> {
    pub(crate) async fn send(&mut self, msg: JuMessage, digester: &Digester) -> JuResult<()> {
        let zmsg = msg.to_zmq_message(digester);
        self.sock.send(zmsg).await?;
        Ok(())
    }
}

impl<S: Socket> HBSocket<S> {
    pub(crate) async fn new(ci: &ConnectionInfo, port: u16) -> JuResult<Self> {
        let mut sock = S::new();
        let ep = sock.bind(&format!("{}://{}:{}", ci.transport, ci.ip, port)).await?;
        info!("Created ZeroMQ REP socket: {:?}", ep);

        Ok(Self { sock, port })
    }
}

impl<S: Socket + SocketRecv + SocketSend> HBSocket<S> {
    pub(crate) async fn run(&mut self) -> JuResult<()> {
        loop {
            let msg = self.sock.recv().await?;
            trace!("{} socket received message: {:?}", self.port, msg);
            self.sock.send(msg).await?;
        }
    }
}
