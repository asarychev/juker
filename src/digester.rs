use bytes::Bytes;

use crate::{ConnectionInfo, JuError, JuResult};

#[derive(Clone)]
pub(crate) enum Digester {
    HmacSha256(hmac_sha256::HMAC),
    None,
}

impl Digester {
    pub(crate) fn new(ci: &ConnectionInfo) -> JuResult<Self> {
        if ci.signature_scheme.is_empty() {
            Ok(Self::None)
        } else if ci.signature_scheme == "hmac-sha256" {
            Ok(Self::HmacSha256(hmac_sha256::HMAC::new(ci.key.as_bytes())))
        } else {
            Err(JuError::UnknownDigest(ci.signature_scheme.clone()))
        }
    }

    pub(crate) fn digest(&self, d1: &Bytes, d2: &Bytes, d3: &Bytes, d4: &Bytes) -> Bytes {
        match self {
            Digester::HmacSha256(hmac) => {
                let mut mac = hmac.clone();
                mac.update(d1);
                mac.update(d2);
                mac.update(d3);
                mac.update(d4);

                let hex = hex::encode(mac.finalize().as_slice());
                hex.into()
            }
            Digester::None => Bytes::new(),
        }
    }
}
