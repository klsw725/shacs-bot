use super::{ControlledChildError, ControlledChildStream};
use std::io::{Read, Result as IoResult};
use std::thread::JoinHandle;

pub(super) fn spawn_reader(
    reader: impl Read + Send + 'static,
    limit: usize,
) -> JoinHandle<IoResult<ControlledChildStream>> {
    std::thread::spawn(move || read_bounded(reader, limit))
}

pub(super) fn join_reader(
    reader: JoinHandle<IoResult<ControlledChildStream>>,
) -> Result<ControlledChildStream, ControlledChildError> {
    reader
        .join()
        .map_err(|_| ControlledChildError::OutputThread)?
        .map_err(|error| ControlledChildError::OutputRead(error.to_string()))
}

fn read_bounded(mut reader: impl Read, limit: usize) -> IoResult<ControlledChildStream> {
    let mut captured = Vec::with_capacity(limit.min(8_192));
    let mut buffer = [0_u8; 8_192];
    let mut total_bytes = 0_u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        let remaining = limit.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(ControlledChildStream {
        truncated: total_bytes > u64::try_from(captured.len()).unwrap_or(u64::MAX),
        captured,
        total_bytes,
    })
}
