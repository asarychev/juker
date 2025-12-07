use std::{cell::Cell, sync::Arc};

use futures::{StreamExt, stream::FuturesUnordered};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::{
    ConnectionInfo, JuError, JuMessage, JuResult,
    digester::Digester,
    message::{EvalResult, EvalValue, MsgSource},
    sockets::HBSocket,
};

pub struct JuServer {
    shell_sock: Arc<Mutex<HBSocket<zeromq::RouterSocket>>>,
    control_sock: Arc<Mutex<HBSocket<zeromq::RouterSocket>>>,
    iopub_sock: HBSocket<zeromq::PubSocket>,
    digester: Digester,
    execution_count: Cell<u32>,
    session_id: Uuid,
}

type FutOut = MsgSource<Result<JuMessage, JuError>>;

impl JuServer {
    pub async fn new(ci: &ConnectionInfo) -> JuResult<Self> {
        let digester = Digester::new(&ci)?;

        let mut hb_socket = HBSocket::<zeromq::RepSocket>::new(&ci, ci.hb_port).await?;
        tokio::spawn(async move {
            hb_socket.run().await?;
            error!("Heartbeat socket exited unexpectedly");
            anyhow::Ok(())
        });

        let shell_sock = Arc::new(Mutex::new(
            HBSocket::<zeromq::RouterSocket>::new(&ci, ci.shell_port).await?,
        ));
        let control_sock = Arc::new(Mutex::new(
            HBSocket::<zeromq::RouterSocket>::new(&ci, ci.control_port).await?,
        ));
        let iopub_sock = HBSocket::<zeromq::PubSocket>::new(&ci, ci.iopub_port).await?;

        Ok(Self {
            shell_sock,
            control_sock,
            iopub_sock,
            digester,
            execution_count: Cell::new(0),
            session_id: Uuid::new_v4(),
        })
    }

    pub async fn run(&mut self) -> JuResult<bool> {
        let mut futs: FuturesUnordered<std::pin::Pin<Box<dyn Future<Output = FutOut>>>> =
            FuturesUnordered::new();

        let mut g1 = self.shell_sock.try_lock()?;
        let mut g2 = self.control_sock.try_lock()?;

        let fut1 = async move { MsgSource::Shell(g1.recv().await) };
        let fut2 = async move { MsgSource::Control(g2.recv().await) };
        futs.push(Box::pin(fut1));
        futs.push(Box::pin(fut2));
        // futs.push(self.shell_sock.recv());
        // futs.push(self.control_sock.recv());

        loop {
            match futs.next().await {
                Some(MsgSource::Shell(res)) => {
                    match res {
                        Ok(msg) => {
                            debug!("Shell socket received Jupyter message: {:?}", msg);

                            let busy_msg = self.new_derived_message(&msg, "status").with_content(
                                serde_json::json!({
                                    "execution_state": "busy",
                                }),
                            );

                            debug!("Sending shell busy message: {:?}", busy_msg);
                            self.iopub_sock.send(busy_msg, &self.digester).await?;

                            match self.process_shell_msg(msg) {
                                Ok(ShellResult::Execute { code_msg, action }) => {
                                    debug!("Sending execute_input message: {:?}", code_msg);
                                    self.iopub_sock.send(code_msg, &self.digester).await?;

                                    // We need to get code execution future results before processing
                                    // next shell message
                                    futs.push(Box::into_pin(action));

                                    continue; // Stop listening for new shell messages until code execution completes
                                }
                                Ok(ShellResult::Other(reply)) => {
                                    debug!("Sending shell reply message: {:?}", reply);
                                    self.shell_sock
                                        .try_lock()?
                                        .send(reply, &self.digester)
                                        .await?;
                                }
                                Err(e) => {
                                    error!("Error processing shell message: {:?}", e);
                                }
                            }

                            // Prepare to receive next shell message
                            let mut g = self.shell_sock.try_lock()?;
                            let fut = async move { MsgSource::Shell(g.recv().await) };
                            futs.push(Box::pin(fut));

                            let idle_msg =
                                self.new_message("status").with_content(serde_json::json!({
                                    "execution_state": "idle",
                                }));

                            debug!("Sending shell idle message: {:?}", idle_msg);
                            self.iopub_sock.send(idle_msg, &self.digester).await?;
                        }

                        Err(e) => {
                            error!("Shell socket returned an error: {:?}", e);
                            return Err(e);
                        }
                    }
                }
                Some(MsgSource::Control(res)) => {
                    match res {
                        Ok(msg) => {
                            debug!("Control socket received Jupyter message: {:?}", msg);

                            match msg.header["msg_type"].as_str() {
                                Some("shutdown_request") => {
                                    let want_restart = match msg.content["restart"].as_bool() {
                                        Some(b) => b,
                                        None => false,
                                    };

                                    let reply = self.new_reply_message(&msg).with_content(json!({
                                        "status": "ok",
                                        "restart": want_restart,
                                    }));

                                    debug!("Sending control shutdown reply message: {:?}", reply);
                                    self.control_sock
                                        .try_lock()?
                                        .send(reply, &self.digester)
                                        .await?;

                                    info!("Shutdown request received, exiting server loop");
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

                        Err(e) => {
                            error!("Control socket returned an error: {:?}", e);
                            return Err(e);
                        }
                    }

                    let mut g = self.control_sock.try_lock()?;
                    let fut = async move { MsgSource::Control(g.recv().await) };
                    futs.push(Box::pin(fut));
                }
                Some(MsgSource::Execution {
                    eval_result,
                    original_msg,
                }) => {
                    match eval_result {
                        EvalResult::Success { results: ers } => {
                            debug!("Code execution completed successfully");

                            let reply = self.new_reply_message(&original_msg).with_content(json!({
                                "status": "ok",
                                "execution_count": self.execution_count.get(),
                            }));

                            debug!("Sending shell execution reply message: {:?}", reply);
                            self.shell_sock
                                .try_lock()?
                                .send(reply, &self.digester)
                                .await?;

                            for ev in ers {
                                let output_msg = self
                                    .new_derived_message(&original_msg, "execute_result")
                                    .with_content(json!({
                                        "data": ev.data,
                                        "metadata": ev.metadata,
                                        "execution_count": self.execution_count,
                                    }));
                                debug!("Sending iopub execute_result message: {:?}", output_msg);
                                self.iopub_sock.send(output_msg, &self.digester).await?;
                            }
                        }
                        EvalResult::Error {
                            ename,
                            evalue,
                            traceback,
                        } => {
                            error!("Code execution encountered an error");

                            let reply = self.new_reply_message(&original_msg).with_content(json!({
                                "status": "error",
                                "ename": ename,
                                "evalue": evalue,
                                "traceback": traceback,
                                "execution_count": self.execution_count.get(),
                            }));

                            debug!("Sending shell execution reply message: {:?}", reply);
                            self.shell_sock
                                .try_lock()?
                                .send(reply, &self.digester)
                                .await?;

                            let err_msg = self
                                .new_derived_message(&original_msg, "error")
                                .with_content(json!({
                                    "ename": ename,
                                    "evalue": evalue,
                                    "traceback": traceback,
                                }));
                            debug!("Sending iopub error message: {:?}", err_msg);
                            self.iopub_sock.send(err_msg, &self.digester).await?;
                        }
                    }

                    // Ready for next shell message
                    let mut g = self.shell_sock.try_lock()?;
                    let fut = async move { MsgSource::Shell(g.recv().await) };
                    futs.push(Box::pin(fut));

                    let idle_msg = self.new_message("status").with_content(serde_json::json!({
                        "execution_state": "idle",
                    }));

                    debug!("Sending shell idle message: {:?}", idle_msg);
                    self.iopub_sock.send(idle_msg, &self.digester).await?;
                }
                None => {
                    error!("No more futures to process, exiting server loop");
                    return Err(JuError::GeneralJukerError(
                        "No more futures to process".into(),
                    ));
                }
            }
        }
    }

    fn process_shell_msg(&self, msg: JuMessage) -> JuResult<ShellResult> {
        if msg.header["msg_type"] == "kernel_info_request" {
            let reply = self.new_reply_message(&msg).with_content(json!({
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
            Ok(ShellResult::Other(reply))
        } else if msg.header["msg_type"] == "is_complete_request" {
            let reply = self.new_reply_message(&msg).with_content(json!({
                "status": "unknown",
            }));
            Ok(ShellResult::Other(reply))
        } else if msg.header["msg_type"] == "execute_request" {
            self.execution_count.set(self.execution_count.get() + 1);

            let code = match &msg.content["code"] {
                Value::String(s) => s.clone(),
                _ => {
                    return Err(JuError::NoCode(
                        "execute_request message missing 'code' field".into(),
                    ));
                }
            };

            let code_msg = self
                .new_derived_message(&msg, "execute_input")
                .with_content(json!({
                    "code": code,
                    "execution_count": self.execution_count.get(),
                }));

            let action = async move {
                let eval_result = eval_code(code).await;
                MsgSource::Execution {
                    eval_result,
                    original_msg: msg,
                }
            };

            Ok(ShellResult::Execute {
                code_msg,
                action: Box::new(action),
            })
        } else {
            // TODO: handle other message types

            Err(JuError::UnsupportedMessageType(
                msg.header["msg_type"].to_string(),
            ))
        }
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

    fn new_header<T: Into<String>>(&self, msg_type: T) -> Value {
        json!({
            "msg_id": Uuid::new_v4().to_string(),
            "username": "kernel",
            "session": self.session_id.to_string(),
            "msg_type": msg_type.into(),
            "version": "5.3",
            "date": chrono::Utc::now().to_rfc3339(),
        })
    }

    fn new_message<T: Into<String>>(&self, msg_type: T) -> JuMessage {
        JuMessage {
            zmq_ids: Vec::new(),
            header: self.new_header(msg_type),
            parent_header: json!({}),
            metadata: json!({}),
            content: json!({}),
        }
    }

    fn new_derived_message<T: Into<String>>(&self, msg: &JuMessage, msg_type: T) -> JuMessage {
        let header = self.new_header(msg_type);

        JuMessage {
            zmq_ids: Vec::new(),
            header,
            parent_header: msg.header.clone(),
            metadata: json!({}),
            content: json!({}),
        }
    }

    fn new_reply_message(&self, msg: &JuMessage) -> JuMessage {
        let msg_type = msg
            .header
            .get("msg_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .replace("_request", "_reply");
        let header = self.new_header(msg_type);

        JuMessage {
            zmq_ids: msg.zmq_ids.clone(),
            header,
            parent_header: msg.header.clone(),
            metadata: json!({}),
            content: json!({}),
        }
    }
}

enum ShellResult {
    Execute {
        code_msg: JuMessage,
        action: Box<dyn std::future::Future<Output = MsgSource<JuResult<JuMessage>>>>,
    },
    Other(JuMessage),
}

async fn eval_code(code: String) -> EvalResult {
    if code.starts_with("err") {
        EvalResult::Error {
            ename: json!("Error"),
            evalue: json!("An error occurred during code execution"),
            traceback: vec![json!("Traceback (most recent call last):"), json!("  ...")],
        }
    } else {
        EvalResult::Success {
            results: vec![EvalValue {
                data: json!({
                    "text/plain": format!("Executed code: {}", code),
                }),
                metadata: json!({}),
            }],
        }
    }
}
