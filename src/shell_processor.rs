use std::sync::Arc;

use serde_json::{Value, json};
use tokio::{select, sync::Notify};
use tracing::{debug, error};

use crate::{JuError, JuKernel, JuMessage, JuResult, server_id::JuServerId, sockets::HBSocket};

pub(crate) struct JuShellProcessor<K: JuKernel> {
    shell_sock: HBSocket<zeromq::RouterSocket>,
    iopub_sock: HBSocket<zeromq::PubSocket>,
    jsi: JuServerId,
    execution_count: u32,
    imp: K,
    notify: Arc<Notify>,
}

impl<K: JuKernel> JuShellProcessor<K> {
    pub(crate) async fn new(
        shell_sock: HBSocket<zeromq::RouterSocket>,
        iopub_sock: HBSocket<zeromq::PubSocket>,
        jsi: JuServerId,
        imp: K,
        notify: Arc<Notify>,
    ) -> JuResult<Self> {
        let mut res = Self {
            shell_sock,
            iopub_sock,
            jsi,
            execution_count: 0,
            imp,
            notify,
        };

        let starting_msg = res
            .jsi
            .new_message("status")
            .with_content(serde_json::json!({
                "execution_state": "starting",
            }));

        res.send_pub(starting_msg).await?;
        Ok(res)
    }

    pub(crate) async fn send_pub(&mut self, msg: JuMessage) -> JuResult<()> {
        debug!("Sending iopub message: {:?}", msg);
        self.iopub_sock.send(msg, &self.jsi.digester).await
    }

    pub(crate) async fn send_shell(&mut self, msg: JuMessage) -> JuResult<()> {
        debug!("Sending shell message: {:?}", msg);
        self.shell_sock.send(msg, &self.jsi.digester).await
    }

    pub(crate) async fn run(mut self) -> JuResult<()> {
        let idle_msg = self
            .jsi
            .new_message("status")
            .with_content(serde_json::json!({
                "execution_state": "idle",
            }));

        self.send_pub(idle_msg).await?;

        loop {
            select! {
                _ = self.notify.notified() => {
                    debug!("Shutdown notification received, exiting shell processor loop");
                    return Ok(());
                }
                res = self.shell_sock.recv() => {
                    let msg = res?;
                    debug!("Received shell message: {:?}", msg);

                    debug!("Shell socket received Jupyter message: {:?}", msg);

                    let busy_msg =
                        self.jsi
                            .new_derived_message(&msg, "status")
                            .with_content(serde_json::json!({
                                "execution_state": "busy",
                            }));

                    self.send_pub(busy_msg).await?;

                    match self.process_shell_msg(msg).await {
                        Ok(()) => {}
                        Err(e) => {
                            error!("Error processing shell message: {:?}", e);
                        }
                    }

                    let idle_msg = self
                        .jsi
                        .new_message("status")
                        .with_content(serde_json::json!({
                            "execution_state": "idle",
                        }));

                    self.send_pub(idle_msg).await?;
                }
            }
        }
    }

    async fn process_shell_msg(&mut self, msg: JuMessage) -> JuResult<()> {
        if msg.header["msg_type"] == "kernel_info_request" {
            let info = self.imp.kernel_info();

            let reply = self.jsi.new_reply_message(&msg).with_content(json!({
                "protocol_version": "5.3",
                "implementation": "juker",
                "implementation_version": "0.1.0",
                "language_info": {
                    "name": info.name,
                    "version": info.version,
                    "mimetype": info.mimetype,
                    "file_extension": info.file_extension
                },
                "banner": info.banner,
                "help_links": info.help_links.iter().map(|link| json!({
                    "text": link.text,
                    "url": link.url,
                })).collect::<Vec<_>>(),
            }));
            self.send_shell(reply).await?;
        } else if msg.header["msg_type"] == "is_complete_request" {
            let reply = self.jsi.new_reply_message(&msg).with_content(json!({
                "status": "unknown",
            }));
            self.send_shell(reply).await?;
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

            let code_msg = self
                .jsi
                .new_derived_message(&msg, "execute_input")
                .with_content(json!({
                    "code": code,
                    "execution_count": self.execution_count,
                }));
            self.send_pub(code_msg).await?;

            // TODO: handle interrupts, cancellations, etc.
            let eval_result = self.imp.eval_code(code).await;

            match eval_result {
                crate::message::EvalResult::Success { results } => {
                    debug!("Code executed successfully");

                    let reply = self.jsi.new_reply_message(&msg).with_content(json!({
                        "status": "ok",
                        "execution_count": self.execution_count,
                    }));

                    self.send_shell(reply).await?;

                    for ev in results {
                        let output_msg = self
                            .jsi
                            .new_derived_message(&msg, "execute_result")
                            .with_content(json!({
                                "data": ev.data,
                                "metadata": ev.metadata,
                                "execution_count": self.execution_count,
                            }));
                        self.send_pub(output_msg).await?;
                    }
                }
                crate::message::EvalResult::Error {
                    ename,
                    evalue,
                    traceback,
                } => {
                    error!("Code execution encountered an error");

                    let reply = self.jsi.new_reply_message(&msg).with_content(json!({
                        "status": "error",
                        "ename": ename,
                        "evalue": evalue,
                        "traceback": traceback,
                        "execution_count": self.execution_count,
                    }));

                    self.send_shell(reply).await?;

                    let err_msg = self
                        .jsi
                        .new_derived_message(&msg, "error")
                        .with_content(json!({
                            "ename": ename,
                            "evalue": evalue,
                            "traceback": traceback,
                        }));
                    debug!("Sending iopub error message: {:?}", err_msg);
                    self.send_pub(err_msg).await?;
                }
            }
        } else {
            // TODO: handle other message types

            return Err(JuError::UnsupportedMessageType(
                msg.header["msg_type"].to_string(),
            ));
        }

        Ok(())
    }
}
