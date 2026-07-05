use std::io::{self, BufRead, BufReader, Read};

pub fn split_sse_frame_texts(body: &str) -> Vec<String> {
    let normalized = body.replace("\r\n", "\n");
    let mut frames = Vec::new();
    let mut current = String::new();
    for line in normalized.split_inclusive('\n') {
        current.push_str(line);
        if line.trim_end_matches('\n').is_empty() {
            push_frame(&mut frames, &mut current);
        }
    }
    if !current.is_empty() {
        push_frame(&mut frames, &mut current);
    }
    frames
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
    let mut reader = BufReader::new(reader);
    let mut body = String::new();
    let mut current = String::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(&mut map_error)?;
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

fn push_frame(frames: &mut Vec<String>, current: &mut String) {
    if !current.trim().is_empty() {
        frames.push(std::mem::take(current));
    } else {
        current.clear();
    }
}
