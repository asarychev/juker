use serde_json::{Value, json};
use uuid::Uuid;

use crate::{ConnectionInfo, JuMessage, JuResult, digester::Digester};

#[derive(Clone)]
pub(crate) struct JuServerId {
    pub session_id: Uuid,
    pub digester: Digester,
}

impl JuServerId {
    pub(crate) fn new(ci: &ConnectionInfo) -> JuResult<Self> {
        let digester = Digester::new(&ci)?;

        Ok(Self {
            session_id: Uuid::new_v4(),
            digester,
        })
    }

    pub(crate) fn new_header<T: Into<String>>(&self, msg_type: T) -> Value {
        json!({
            "msg_id": Uuid::new_v4().to_string(),
            "username": "kernel",
            "session": self.session_id.to_string(),
            "msg_type": msg_type.into(),
            "version": "5.3",
            "date": chrono::Utc::now().to_rfc3339(),
        })
    }

    pub(crate) fn new_message<T: Into<String>>(&self, msg_type: T) -> JuMessage {
        JuMessage {
            zmq_ids: Vec::new(),
            header: self.new_header(msg_type),
            parent_header: json!({}),
            metadata: json!({}),
            content: json!({}),
        }
    }

    pub(crate) fn new_derived_message<T: Into<String>>(&self, msg: &JuMessage, msg_type: T) -> JuMessage {
        let header = self.new_header(msg_type);

        JuMessage {
            zmq_ids: Vec::new(),
            header,
            parent_header: msg.header.clone(),
            metadata: json!({}),
            content: json!({}),
        }
    }

    pub(crate) fn new_reply_message(&self, msg: &JuMessage) -> JuMessage {
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
