use std::clone;

use serde_json::{Value, json};
use tracing::{debug, error};
use uuid::Uuid;

use crate::{ConnectionInfo, JuError, JuMessage, JuResult, digester::Digester, sockets::HBSocket};

pub struct JuServer {
    shell_sock: HBSocket<zeromq::RouterSocket>,
    control_sock: HBSocket<zeromq::RouterSocket>,
    iopub_sock: HBSocket<zeromq::PubSocket>,
    digester: Digester,
    execution_count: u32,
    session_id: Uuid,
}

impl JuServer {
    pub async fn new(ci: ConnectionInfo) -> JuResult<Self> {
        let digester = Digester::new(&ci)?;

        let mut hb_socket = HBSocket::<zeromq::RepSocket>::new(&ci, ci.hb_port).await?;
        tokio::spawn(async move {
            hb_socket.run().await?;
            error!("Heartbeat socket exited unexpectedly");
            anyhow::Ok(())
        });

        let shell_sock = HBSocket::<zeromq::RouterSocket>::new(&ci, ci.shell_port).await?;
        let control_sock = HBSocket::<zeromq::RouterSocket>::new(&ci, ci.control_port).await?;
        let iopub_sock = HBSocket::<zeromq::PubSocket>::new(&ci, ci.iopub_port).await?;

        Ok(Self {
            shell_sock,
            control_sock,
            iopub_sock,
            digester,
            execution_count: 0,
            session_id: Uuid::new_v4(),
        })
    }

    pub async fn run(&mut self) -> JuResult<()> {
        loop {
            tokio::select! {
                res = self.shell_sock.recv() => {
                    match res {
                        Ok(msg) => {
                            debug!("Shell socket received Jupyter message: {:?}", msg);

                            let busy_msg = msg.create_derived("status").with_content(serde_json::json!({
                                "execution_state": "busy",
                            }));

                            let idle_msg = msg.create_derived("status").with_content(serde_json::json!({
                                "execution_state": "idle",
                            }));

                            debug!("Sending shell busy message: {:?}", busy_msg);
                            self.iopub_sock.send(busy_msg, &self.digester).await?;

                            // TODO: handle errors properly
                            let (code_msg_opt, action) = self.process_shell_msg(msg)?;
                            tokio::pin!(action);

                            if let Some(code_msg) = code_msg_opt {
                                debug!("Sending execute_input message: {:?}", code_msg);
                                self.iopub_sock.send(code_msg, &self.digester).await?;
                            }

                            tokio::select! {
                                res = &mut action => {
                                    match res {
                                        Ok((reply, pub_msgs)) => {
                                            debug!("Sending shell reply message: {:?}", reply);
                                            self.shell_sock.send(reply, &self.digester).await?;

                                            for pub_msg in pub_msgs {
                                                debug!("Sending shell pub message: {:?}", pub_msg);
                                                self.iopub_sock.send(pub_msg, &self.digester).await?;
                                            }
                                        }
                                        Err(e) => {
                                            error!("Error processing shell message: {:?}", e);
                                        }
                                    }

                                    debug!("Sending shell idle message: {:?}", idle_msg);
                                    self.iopub_sock.send(idle_msg, &self.digester).await?;
                                }
                                res = self.control_sock.recv() => {
                                    self.process_control(res).await?;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Error receiving shell message: {:?}", e);
                        }
                    }
                }
                res = self.control_sock.recv() => {
                    self.process_control(res).await?;
                }
            }
        }
        // error!("Juker application run completed");
        // Ok(())
    }

    fn process_shell_msg(
        &mut self,
        msg: JuMessage,
    ) -> JuResult<(
        Option<JuMessage>,
        impl Future<Output = Result<(JuMessage, Vec<JuMessage>), JuError>> + Send + use<>,
    )> {
        let mut reply = msg.create_reply();
        let mut code_msg_opt = None;
        let mut pub_msgs = Vec::new();

        if msg.header["msg_type"] == "kernel_info_request" {
            reply = reply.with_content(json!({
                "protocol_version": "5.3",
                "implementation": "juker",
                "implementation_version": "0.1.0",
                "language_info": {
                    "name": "rust",
                    "version": "1.70.0",
                    "mimetype": "text/x-rust",
                    "file_extension": ".rs"
                },
                "banner": "Juker Rust Jupyter Kernel",
                "help_links": [
                    {
                        "text": "Juker Documentation",
                        "url": "https://github.com/asaryche/juker"
                    }
                ]
            }));
        } else if msg.header["msg_type"] == "is_complete_request" {
            reply = reply.with_content(json!({
                "status": "unknown",
            }));
        } else if msg.header["msg_type"] == "execute_request" {
            self.execution_count += 1;

            let code = match &msg.content["code"] {
                Value::String(s) => s.clone(),
                _ => {
                    return Err(JuError::NoCode(
                        "execute_request message missing 'code' field".into(),
                    ));
                }
            };

            code_msg_opt = Some(msg.create_derived("execute_input").with_content(json!({
                "code": code,
                "execution_count": self.execution_count,
            })));

            reply = reply.with_content(json!({
                "status": "ok",
                "execution_count": self.execution_count,
            }));

            let output_msg = msg.create_derived("execute_result").with_content(json!({
                "data": {
                    "text/plain": format!("Executed code #{}.", self.execution_count),
                },
                "metadata": {},
                "execution_count": self.execution_count,
            }));
            pub_msgs.push(output_msg);

            let ms = 42; // TODO: measure execution time

            let output_msg = msg.create_derived("execute_result").with_content(json!({
                "data": {
                    "text/html": format!(
                            "<span style=\"color: rgba(0,0,0,0.4);\">Took {ms}ms</span>"
                        ),
                },
                "metadata": {},
                "execution_count": self.execution_count,
            }));
            pub_msgs.push(output_msg);
        } else {
            // TODO: handle other message types
        }

        let fut = async move { Ok((reply, pub_msgs)) };

        Ok((code_msg_opt, fut))
    }

    async fn process_control(&self, r: JuResult<JuMessage>) -> JuResult<JuMessage> {
        match r.as_ref() {
            Ok(msg) => {
                debug!("Control socket received Jupyter message: {:?}", msg);
            }
            Err(e) => {
                error!("Error receiving control message: {:?}", e);
            }
        }
        r
    }
}

async fn eval_code(code: &str) -> JuResult<String> {
    Ok(format!("Executed code: {}", code))
}
