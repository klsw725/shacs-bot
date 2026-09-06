pub(super) fn percent_decode(text: &str) -> Option<String> {
    let input = text.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    let mut changed = false;
    while index < input.len() {
        if input[index] == b'%'
            && input.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
            && input.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            let high = hex(input[index + 1]);
            let low = hex(input[index + 2]);
            output.push((high << 4) | low);
            index += 3;
            changed = true;
        } else {
            output.push(input[index]);
            index += 1;
        }
    }
    changed.then(|| String::from_utf8_lossy(&output).into_owned())
}

pub(super) fn decoded_layers(text: &str) -> Result<Vec<String>, ()> {
    const MAX_LAYERS: usize = 4;
    let mut layers = Vec::new();
    let mut current = text.to_owned();
    for _ in 0..MAX_LAYERS {
        let Some(decoded) = percent_decode(&current) else {
            return Ok(layers);
        };
        layers.push(decoded.clone());
        current = decoded;
    }
    if percent_decode(&current).is_some() {
        Err(())
    } else {
        Ok(layers)
    }
}

fn hex(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}
