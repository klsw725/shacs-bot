use std::io::{self, BufRead, BufReader, Read};

pub fn split_sse_frame_texts(body: &str) -> Vec<String> {
    split_sse_frame_texts_bounded(body, usize::MAX, usize::MAX, usize::MAX)
        .unwrap_or_else(|_| Vec::new())
}

pub fn split_sse_frame_texts_bounded(
    body: &str,
    max_line_bytes: usize,
    max_frame_bytes: usize,
    max_aggregate_bytes: usize,
) -> io::Result<Vec<String>> {
    if body.len() > max_aggregate_bytes {
        return Err(limit_error("SSE aggregate limit exceeded"));
    }
    let normalized = body.replace("\r\n", "\n");
    let mut frames = Vec::new();
    let mut current = String::new();
    for line in normalized.split_inclusive('\n') {
        if line.len() > max_line_bytes {
            return Err(limit_error("SSE line limit exceeded"));
        }
        if current.len().saturating_add(line.len()) > max_frame_bytes {
            return Err(limit_error("SSE frame limit exceeded"));
        }
        current.push_str(line);
        if line.trim_end_matches('\n').is_empty() {
            push_frame(&mut frames, &mut current);
        }
    }
    if !current.is_empty() {
        push_frame(&mut frames, &mut current);
    }
    Ok(frames)
}

pub fn read_sse_frame_texts<R, E, F, M>(
    reader: R,
    mut on_frame: F,
    mut map_error: M,
) -> Result<String, E>
where
    R: Read,
    F: FnMut(&str) -> Result<bool, E>,
    M: FnMut(io::Error) -> E,
{
    read_sse_frame_texts_bounded(
        reader,
        &mut on_frame,
        &mut map_error,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    )
}

pub fn read_sse_frame_texts_bounded<R, E, F, M>(
    reader: R,
    mut on_frame: F,
    mut map_error: M,
    max_line_bytes: usize,
    max_frame_bytes: usize,
    max_aggregate_bytes: usize,
) -> Result<String, E>
where
    R: Read,
    F: FnMut(&str) -> Result<bool, E>,
    M: FnMut(io::Error) -> E,
{
    let mut reader = BufReader::new(reader);
    let mut body = String::new();
    let mut current = String::new();
    let mut aggregate_bytes = 0usize;
    loop {
        let mut line = String::new();
        let read_limit = u64::try_from(max_line_bytes.saturating_add(1)).unwrap_or(u64::MAX);
        let read = reader
            .by_ref()
            .take(read_limit)
            .read_line(&mut line)
            .map_err(&mut map_error)?;
        if read == 0 {
            if !current.is_empty() {
                let done = on_frame(&current)?;
                body.push_str(&current);
                if done {
                    break;
                }
            }
            break;
        }
        if read > max_line_bytes {
            return Err(map_error(limit_error("SSE line limit exceeded")));
        }
        aggregate_bytes = aggregate_bytes.saturating_add(read);
        if aggregate_bytes > max_aggregate_bytes {
            return Err(map_error(limit_error("SSE aggregate limit exceeded")));
        }
        if current.len().saturating_add(read) > max_frame_bytes {
            return Err(map_error(limit_error("SSE frame limit exceeded")));
        }
        current.push_str(&line);
        if line.trim_end_matches(['\r', '\n']).is_empty() {
            let done = on_frame(&current)?;
            body.push_str(&current);
            current.clear();
            if done {
                break;
            }
        }
    }
    Ok(body)
}

fn limit_error(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn push_frame(frames: &mut Vec<String>, current: &mut String) {
    if !current.trim().is_empty() {
        frames.push(std::mem::take(current));
    } else {
        current.clear();
    }
}
