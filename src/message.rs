use std::ops::ControlFlow;

use crate::{DELIMITER, JuError, JuResult, digester::Digester};
use bytes::Bytes;
use serde_json::Value;
use zeromq::ZmqMessage;

pub enum EvalResult {
    Success {
        results: Vec<EvalValue>,
    },
    Error {
        ename: Value,
        evalue: Value,
        traceback: Vec<Value>,
    },
}

pub struct EvalValue {
    pub data: Value,
    pub metadata: Value,
}

pub enum MsgSource<T> {
    Shell(T),
    Control(T),
    Execution {
        eval_result: EvalResult,
        original_msg: JuMessage,
    },
}

pub struct JuMessage {
    pub zmq_ids: Vec<Bytes>,
    pub header: Value,
    pub parent_header: Value,
    pub metadata: Value,
    pub content: Value,
}

impl JuMessage {
    pub fn with_content(mut self, content: Value) -> Self {
        self.content = content;
        self
    }

    pub(crate) fn to_zmq_message(self, digester: &Digester) -> ZmqMessage {
        let mut msg: ZmqMessage = Bytes::from_static(DELIMITER).into();

        for id in self.zmq_ids.into_iter().rev() {
            msg.push_front(id);
        }

        let header = Bytes::from(serde_json::to_vec(&self.header).unwrap());
        let parent_header = Bytes::from(serde_json::to_vec(&self.parent_header).unwrap());
        let metadata = Bytes::from(serde_json::to_vec(&self.metadata).unwrap());
        let content = Bytes::from(serde_json::to_vec(&self.content).unwrap());

        let hmac = digester.digest(&header, &parent_header, &metadata, &content);
        msg.push_back(hmac);

        msg.push_back(header);
        msg.push_back(parent_header);
        msg.push_back(metadata);
        msg.push_back(content);
        msg
    }
}

impl std::fmt::Debug for JuMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JupyterMessage {{ zmq_ids: [")?;

        let mut first = true;
        for id in &self.zmq_ids {
            if first {
                first = false;
            } else {
                write!(f, ", ")?;
            }
            write!(f, "{:?}", String::from_utf8_lossy(id))?;
        }

        write!(
            f,
            "], header: {}, parent_header: {}, metadata: {}, content: {} }}",
            self.header, self.parent_header, self.metadata, self.content
        )
    }
}

impl TryFrom<ZmqMessage> for JuMessage {
    type Error = JuError;

    fn try_from(msg: ZmqMessage) -> JuResult<Self> {
        let mut it = msg.iter();

        let zmq_ids = it
            .try_fold(Vec::new(), |mut acc, x| {
                if x.as_ref() == crate::DELIMITER {
                    ControlFlow::Break(acc)
                } else {
                    acc.push(x.clone());
                    ControlFlow::Continue(acc)
                }
            })
            .break_value()
            .ok_or(JuError::MalformedMessage("no delimiter found".into()))?;

        let sig = it
            .next()
            .ok_or(JuError::MalformedMessage("no signature".into()))?
            .to_vec();

        // TODO: verify signature

        let header: Value = serde_json::from_slice(
            it.next()
                .ok_or(JuError::MalformedMessage("no header".into()))?,
        )?;

        let parent_header: Value = serde_json::from_slice(
            it.next()
                .ok_or(JuError::MalformedMessage("no parent header".into()))?,
        )?;

        let metadata: Value = serde_json::from_slice(
            it.next()
                .ok_or(JuError::MalformedMessage("no metadata".into()))?,
        )?;

        let content: Value = serde_json::from_slice(
            it.next()
                .ok_or(JuError::MalformedMessage("no content".into()))?,
        )?;

        Ok(JuMessage {
            zmq_ids,
            header,
            parent_header,
            metadata,
            content,
        })
    }
}
