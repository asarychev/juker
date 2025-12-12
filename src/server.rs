use std::sync::Arc;

use serde_json::json;
use tokio::sync::Notify;
use tracing::{debug, error, info, warn};

use crate::{
    ConnectionInfo, JuKernel, JuMessage, JuResult, server_id::JuServerId, shell_processor::JuShellProcessor, sockets::HBSocket
};

pub struct JuServer {
    control_sock: HBSocket<zeromq::RouterSocket>,
    jsi: JuServerId,
    notify: Arc<Notify>,
}

impl JuServer {
    pub async fn start<K: JuKernel>(ci: &ConnectionInfo, imp: K) -> JuResult<bool> {
        let jsi = JuServerId::new(&ci)?;

        let mut hb_socket = HBSocket::<zeromq::RepSocket>::new(&ci, ci.hb_port).await?;
        tokio::spawn(async move {
            hb_socket.run().await?;
            error!("Heartbeat socket exited unexpectedly");
            anyhow::Ok(())
        });

        let shell_sock = HBSocket::<zeromq::RouterSocket>::new(&ci, ci.shell_port).await?;
        let control_sock = HBSocket::<zeromq::RouterSocket>::new(&ci, ci.control_port).await?;
        let iopub_sock = HBSocket::<zeromq::PubSocket>::new(&ci, ci.iopub_port).await?;

        let notify = Arc::new(Notify::new());

        let shell_processor = JuShellProcessor::new(shell_sock, iopub_sock, jsi.clone(), imp, notify.clone()).await?;

        let srv = Self {
            control_sock,
            jsi,
            notify,
        };

        let jh = tokio::spawn(srv.run());

        shell_processor.run().await?;

        jh.await?
    }

    async fn run(mut self) -> JuResult<bool> {

        loop {
            let msg = self.control_sock.recv().await?;
            debug!("Control socket received Jupyter message: {:?}", msg);

            match msg.header["msg_type"].as_str() {
                Some("shutdown_request") => {
                    let want_restart = match msg.content["restart"].as_bool() {
                        Some(b) => b,
                        None => false,
                    };

                    let reply = self.jsi.new_reply_message(&msg).with_content(json!({
                        "status": "ok",
                        "restart": want_restart,
                    }));

                    self.send_control(reply).await?;

                    info!("Shutdown request received, exiting server loop");
                    self.notify.notify_one();
                    return Ok(want_restart);
                }
                Some(msg_type) => {
                    warn!("Unsupported control message type: {:?}", msg_type);
                }
                None => {
                    error!("Control message missing msg_type field");
                }
            }
        }
    }

    pub(crate) async fn send_control(&mut self, msg: JuMessage) -> JuResult<()> {
        debug!("Sending control message: {:?}", msg);
        self.control_sock.send(msg, &self.jsi.digester).await
    }
}
