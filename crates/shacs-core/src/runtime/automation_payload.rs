use super::{AutomationWorkEnvelope, DurableDispatchError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct AutomationWorkPayload {
    envelope_hex: String,
}

impl AutomationWorkPayload {
    pub(super) fn from_envelope(
        envelope: &AutomationWorkEnvelope,
    ) -> Result<Self, DurableDispatchError> {
        Ok(Self {
            envelope_hex: encode_hex(&serde_json::to_vec(envelope)?),
        })
    }

    pub(super) fn into_envelope(self) -> Result<AutomationWorkEnvelope, DurableDispatchError> {
        Ok(serde_json::from_slice(&decode_hex(&self.envelope_hex)?)?)
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>, DurableDispatchError> {
    if encoded.len() % 2 != 0 {
        return Err(malformed());
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?))
        .collect()
}

fn hex_digit(value: u8) -> Result<u8, DurableDispatchError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(malformed()),
    }
}

fn malformed() -> DurableDispatchError {
    DurableDispatchError::InvalidWork("automation payload encoding is malformed".to_owned())
}
