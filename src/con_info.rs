use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnectionInfo {
    pub(crate) kernel_name: String,
    pub(crate) ip: String,
    pub(crate) control_port: u16,
    pub(crate) shell_port: u16,
    pub(crate) stdin_port: u16,
    pub(crate) hb_port: u16,
    pub(crate) iopub_port: u16,
    pub(crate) key: String,
    pub(crate) transport: String,
    pub(crate) signature_scheme: String,
}