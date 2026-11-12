//! Spec 050 PRD 010 scope 6: the paste path of a takeover episode, on the
//! real manager and its real Unix socket, with the fake DevTools peer and the
//! manual clock of the takeover tests.

use super::spec050_browser_takeover::{
    eventually, open, taken, Setup, Stage, OWNER, SECRET, TAKEOVER_MS, VIEW_ROUTE,
};
use serde_json::json;
use shacs_cli::browser::{
    self, paste_digest, BrowserControl, PASTE_AUTH_DELAY_MS, PASTE_AUTH_FAILURES,
    PASTE_CONNECTIONS, PASTE_STAGE_LIMIT_MS, PASTE_TEXT_LIMIT,
};
use shacs_core::runtime::{
    BrowserCloseOutcome, BrowserError, BrowserHost, BrowserReturnOutcome, BrowserReturnTarget,
};
use std::error::Error;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::{fs, thread};

/// How long an answer that must come may take, in real time.
const WAIT: Duration = Duration::from_secs(5);
/// How long nothing must come for the peer to count as silent.
const SILENCE: Duration = Duration::from_millis(120);
/// The time the server gets to read what was sent before the clock moves.
const SETTLE: Duration = Duration::from_millis(150);

#[derive(Debug, PartialEq, Eq)]
enum Heard {
    Nothing,
    Closed,
    Line(String),
}

struct Client {
    stream: UnixStream,
    pending: Vec<u8>,
    label: String,
    nonce: String,
}

fn socket(stage: &Stage) -> Result<PathBuf, Box<dyn Error>> {
    Ok(stage.root()?.join(browser::PASTE_SOCKET))
}

impl Client {
    fn raw(stage: &Stage) -> Result<Self, Box<dyn Error>> {
        let stream = UnixStream::connect(socket(stage)?)?;
        stream.set_write_timeout(Some(WAIT))?;
        Ok(Self {
            stream,
            pending: Vec::new(),
            label: String::new(),
            nonce: String::new(),
        })
    }

    /// A connection the server greeted.
    fn open(stage: &Stage) -> Result<Self, Box<dyn Error>> {
        let mut client = Self::raw(stage)?;
        let greeting = client.answer()?;
        let parts: Vec<&str> = greeting.split(' ').collect();
        let [name, version, label, nonce] = parts[..] else {
            return Err(format!("greeting: {greeting}").into());
        };
        assert_eq!((name, version), ("shacs-paste", "1"));
        assert_eq!(nonce.len(), 32);
        assert!(nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        client.label = label.to_owned();
        client.nonce = nonce.to_owned();
        Ok(client)
    }

    /// What the server says within `wait`: a line, the end, or nothing.
    fn heard(&mut self, wait: Duration) -> Result<Heard, Box<dyn Error>> {
        let until = Instant::now() + wait;
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                return Ok(Heard::Line(
                    String::from_utf8(line[..end].to_vec()).map_err(|_| "not a text line")?,
                ));
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Heard::Nothing);
            }
            // macOS refuses a socket option once the peer has hung up. The
            // read below does not wait then, whatever the limit was.
            match self.stream.set_read_timeout(Some(left)) {
                Err(error) if error.kind() != ErrorKind::InvalidInput => return Err(error.into()),
                _ => {}
            }
            let mut buffer = [0; 256];
            match self.stream.read(&mut buffer) {
                Ok(0) => return Ok(Heard::Closed),
                Ok(read) => self.pending.extend_from_slice(&buffer[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                    ) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::ConnectionReset | ErrorKind::BrokenPipe
                    ) =>
                {
                    return Ok(Heard::Closed)
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn answer(&mut self) -> Result<String, Box<dyn Error>> {
        match self.heard(WAIT)? {
            Heard::Line(line) => Ok(line),
            other => Err(format!("no line: {other:?}").into()),
        }
    }

    fn closed(&mut self) -> Result<(), Box<dyn Error>> {
        match self.heard(WAIT)? {
            Heard::Closed => Ok(()),
            other => Err(format!("not closed: {other:?}").into()),
        }
    }

    fn quiet(&mut self) -> Result<(), Box<dyn Error>> {
        match self.heard(SILENCE)? {
            Heard::Nothing => Ok(()),
            other => Err(format!("not silent: {other:?}").into()),
        }
    }

    /// Sends bytes. A server that already hung up is not an error here; what
    /// it said before is read afterwards.
    fn say(&mut self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        match self.stream.write_all(bytes) {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::BrokenPipe | ErrorKind::ConnectionReset
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn auth_line(&self) -> String {
        format!("auth {}\n", paste_digest(&self.nonce, SECRET))
    }

    /// Answers the challenge with the view password. The line is returned so
    /// a test can send it again somewhere else.
    fn auth(&mut self) -> Result<String, Box<dyn Error>> {
        let line = self.auth_line();
        self.say(line.as_bytes())?;
        assert_eq!(self.answer()?, "ok");
        Ok(line)
    }

    fn paste(&mut self, text: &[u8]) -> Result<(), Box<dyn Error>> {
        self.say(format!("paste {}\n", text.len()).as_bytes())?;
        self.say(text)
    }
}

/// An authenticated connection.
fn trusted(stage: &Stage) -> Result<Client, Box<dyn Error>> {
    let mut client = Client::open(stage)?;
    client.auth()?;
    Ok(client)
}

/// One text on its own connection and what the server said to it.
fn pasted(stage: &Stage, text: &[u8]) -> Result<String, Box<dyn Error>> {
    let mut client = trusted(stage)?;
    client.paste(text)?;
    let answer = client.answer()?;
    client.closed()?;
    Ok(answer)
}

/// One wrong answer. Its `error auth` waits a second by the clock, counted
/// from when the server had the line, so the clock is moved a second at a
/// time until the answer is there.
fn wrong(stage: &Stage, line: impl FnOnce(&Client) -> String) -> Result<(), Box<dyn Error>> {
    let mut client = Client::open(stage)?;
    let line = line(&client);
    client.say(line.as_bytes())?;
    for _ in 0..50 {
        match client.heard(SILENCE)? {
            Heard::Nothing => stage.set_clock(stage.now() + PASTE_AUTH_DELAY_MS),
            Heard::Line(line) => {
                assert_eq!(line, "error auth");
                return client.closed();
            }
            Heard::Closed => return Err("a wrong answer was not answered".into()),
        }
    }
    Err("no error auth".into())
}

fn zeros(_: &Client) -> String {
    format!("auth {}\n", "0".repeat(64))
}

fn inserted(stage: &Stage) -> Vec<String> {
    stage.page.lock().expect("page").inserted.clone()
}

fn sent(stage: &Stage) -> Vec<String> {
    stage.page.lock().expect("page").methods.clone()
}

/// Everything the manager shows or keeps outside the page: the state file,
/// status, inspect, `/browser status`, the episode records and the files of
/// the generation folder.
fn shown(stage: &Stage) -> Result<String, Box<dyn Error>> {
    let mut all = fs::read_to_string(
        stage
            .bundle
            .context
            .data_dir
            .join("runtime/browser-state.json"),
    )?;
    all.push_str(&format!("\n{:?}", stage.host.status()?));
    all.push_str(&format!("\n{:?}", stage.inspected("owner")?));
    all.push_str(&format!("\n{:?}", stage.host.describe()));
    all.push_str(&format!("\n{:?}", stage.records()));
    for entry in fs::read_dir(stage.root()?)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            all.push('\n');
            all.push_str(&String::from_utf8_lossy(&fs::read(entry.path())?));
        }
    }
    Ok(all)
}

fn times(shown: &str, number: usize) -> usize {
    let number = number.to_string();
    shown
        .split(|character: char| !character.is_ascii_digit())
        .filter(|token| *token == number)
        .count()
}

#[test]
fn paste_inserts_utf8_text_once_in_user_state() -> Result<(), Box<dyn Error>> {
    let stage = Stage::with(Setup {
        state_dir: true,
        ..Setup::default()
    })?;
    stage.run(OWNER, &open())?;
    // Outside an episode there is no socket and no `paste` route.
    assert!(fs::symlink_metadata(socket(&stage)?).is_err());
    assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));

    let takeover = taken(stage.take())?;
    let metadata = fs::symlink_metadata(socket(&stage)?)?;
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(stage.routes(), Some(stage.two_routes()?));

    let sentence = "안녕하세요, 값을 붙여 넣습니다. 받침: 닭·삶·읽다·없음! ";
    let text = format!("{}\r\n둘째 줄\t탭 뒤 — 끝?\r\n", sentence.repeat(7));
    let expected = text.replace("\r\n", "\n");
    let lengths = [
        text.len(),
        expected.len(),
        text.chars().count(),
        expected.chars().count(),
        text.encode_utf16().count(),
    ];
    let before = shown(&stage)?;
    let commands = sent(&stage).len();
    // The focus is in a password field: the text is the user's own input.
    stage.page.lock().expect("page").password_focus = true;

    let mut client = trusted(&stage)?;
    assert_eq!(client.label, takeover.label);
    client.paste(text.as_bytes())?;
    assert_eq!(client.answer()?, "done");
    assert_eq!(inserted(&stage), std::slice::from_ref(&expected));
    // One command went to the page, and it is the insert.
    assert_eq!(sent(&stage)[commands..], ["Input.insertText"]);

    // One connection is one text.
    client.paste("한 번 더".as_bytes())?;
    client.closed()?;
    assert_eq!(inserted(&stage).len(), 1);
    assert_eq!(sent(&stage).len(), commands + 1);

    // Neither the text nor a length of it is kept anywhere.
    let after = shown(&stage)?;
    for part in [sentence.trim(), "붙여", "둘째 줄", "탭 뒤"] {
        assert!(!after.contains(part), "the text is kept: {part}");
    }
    for length in lengths {
        assert_eq!(
            times(&after, length),
            times(&before, length),
            "a length of the text is kept: {length}"
        );
    }

    // The listener stays for the next text of the episode.
    assert_eq!(pasted(&stage, "둘째 글".as_bytes())?, "done");
    assert_eq!(inserted(&stage), [expected.clone(), "둘째 글".to_owned()]);
    assert!(stage.user());

    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    assert!(fs::symlink_metadata(socket(&stage)?).is_err());
    assert!(UnixStream::connect(socket(&stage)?).is_err());
    assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));
    let after = shown(&stage)?;
    assert!(!after.contains("붙여") && !after.contains("둘째"));
    for length in lengths {
        assert_eq!(times(&after, length), times(&before, length));
    }

    // The same field refuses the assistant's `type`, as it did before.
    let read = stage.look(OWNER)?;
    let element = read.elements.first().ok_or("no numbered element")?.element;
    assert_eq!(
        stage.run(
            OWNER,
            &json!({"action":"type","element":element,"revision":read.revision,"text":"secret"}),
        ),
        Err(BrowserError::InputDenied)
    );
    assert_eq!(inserted(&stage).len(), 2);
    Ok(())
}

#[test]
fn paste_rejects_bad_auth_replay_and_oversize() -> Result<(), Box<dyn Error>> {
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let takeover = taken(stage.take())?;

    // A wrong answer is told so one second later by the clock, not before.
    let mut client = Client::open(&stage)?;
    client.say(zeros(&client).as_bytes())?;
    thread::sleep(SETTLE);
    let asked = stage.now();
    client.quiet()?;
    stage.set_clock(asked + PASTE_AUTH_DELAY_MS - 1);
    client.quiet()?;
    stage.set_clock(asked + PASTE_AUTH_DELAY_MS);
    assert_eq!(client.answer()?, "error auth");
    client.closed()?;
    let mut wrongs = 1;

    // A right answer still gets in after that, and its text goes in.
    let mut first = Client::open(&stage)?;
    let overheard = first.auth()?;
    first.paste(b"first")?;
    assert_eq!(first.answer()?, "done");
    first.closed()?;
    assert_eq!(inserted(&stage), ["first"]);

    // The same answer on another connection is wrong: the nonce is new.
    let second = Client::open(&stage)?;
    assert_ne!(second.nonce, first.nonce);
    drop(second);
    wrong(&stage, |_| overheard.clone())?;
    wrongs += 1;
    // Other wrong answers: another password, an upper-case digest, a short
    // one, and a line that is not an answer.
    wrong(&stage, |client| {
        format!("auth {}\n", paste_digest(&client.nonce, "another-password"))
    })?;
    wrong(&stage, |client| {
        client
            .auth_line()
            .to_uppercase()
            .replacen("AUTH", "auth", 1)
    })?;
    wrong(&stage, |_| "auth abc\n".to_owned())?;
    wrong(&stage, |_| "hello\n".to_owned())?;
    wrongs += 4;
    assert_eq!(inserted(&stage), ["first"]);

    // A line longer than 256 bytes is not read to its end.
    let mut long = Client::open(&stage)?;
    long.say(&[b'a'; 300])?;
    long.closed()?;
    let mut long = trusted(&stage)?;
    long.say(format!("paste {}\n", "1".repeat(300)).as_bytes())?;
    long.closed()?;
    // Neither is a `paste` line that names no plain length.
    for line in [
        "paste abc\n",
        "paste -1\n",
        "paste 1 2\n",
        "paste\n",
        "put 3\n",
    ] {
        let mut client = trusted(&stage)?;
        client.say(line.as_bytes())?;
        client.closed()?;
    }

    // The length is refused before any of the text is read.
    let mut client = trusted(&stage)?;
    client.say(format!("paste {}\n", PASTE_TEXT_LIMIT + 1).as_bytes())?;
    assert_eq!(client.answer()?, "error too_long");
    client.closed()?;
    let mut client = trusted(&stage)?;
    client.say(b"paste 0\n")?;
    assert_eq!(client.answer()?, "error invalid_text");
    client.closed()?;

    // Text that is not UTF-8, or has NUL or another control character.
    for text in [
        &[0xff, 0xfe][..],
        &[0xed, 0x95][..],
        b"a\0b",
        b"a\x1bb",
        b"a\rb",
        b"a\x08",
    ] {
        assert_eq!(pasted(&stage, text)?, "error invalid_text");
    }
    assert_eq!(inserted(&stage), ["first"]);

    // The longest text goes in whole.
    let longest = "가".repeat(PASTE_TEXT_LIMIT / 3) + "ab";
    assert_eq!(longest.len(), PASTE_TEXT_LIMIT);
    assert_eq!(pasted(&stage, longest.as_bytes())?, "done");
    assert_eq!(inserted(&stage), ["first".to_owned(), longest.clone()]);

    // None of that was a wrong answer: the listener goes with the tenth.
    while wrongs < PASTE_AUTH_FAILURES - 1 {
        wrong(&stage, zeros)?;
        wrongs += 1;
    }
    assert!(fs::symlink_metadata(socket(&stage)?).is_ok());
    assert_eq!(stage.routes(), Some(stage.two_routes()?));
    drop(trusted(&stage)?);
    wrong(&stage, zeros)?;
    assert!(fs::symlink_metadata(socket(&stage)?).is_err());
    assert!(UnixStream::connect(socket(&stage)?).is_err());
    assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));
    // The episode goes on without the paste path.
    assert!(stage.user());
    assert_eq!(stage.host.status()?.control, BrowserControl::User);
    assert_eq!(inserted(&stage), ["first".to_owned(), longest]);
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));

    // The next episode has its paste path again.
    let again = taken(stage.take())?;
    assert_eq!(pasted(&stage, b"next")?, "done");
    assert_eq!(stage.give_back(&again), BrowserReturnOutcome::Returned);
    Ok(())
}

/// A connection that stops talking at each of the three stages is dropped
/// when ten seconds of the clock are over, and not a millisecond before.
fn stalls(stage: &Stage) -> Result<(), Box<dyn Error>> {
    // The `auth` line.
    let mut client = Client::open(stage)?;
    let asked = stage.now();
    stage.set_clock(asked + PASTE_STAGE_LIMIT_MS - 1);
    client.quiet()?;
    stage.set_clock(asked + PASTE_STAGE_LIMIT_MS);
    client.closed()?;

    // The `paste` line.
    let mut client = trusted(stage)?;
    let asked = stage.now();
    stage.set_clock(asked + PASTE_STAGE_LIMIT_MS - 1);
    client.quiet()?;
    stage.set_clock(asked + PASTE_STAGE_LIMIT_MS);
    client.closed()?;

    // The text: its time starts when the server has the `paste` line.
    let mut client = trusted(stage)?;
    client.say(b"paste 10\nabc")?;
    thread::sleep(SETTLE);
    let asked = stage.now();
    stage.set_clock(asked + PASTE_STAGE_LIMIT_MS - 1);
    client.quiet()?;
    stage.set_clock(asked + PASTE_STAGE_LIMIT_MS);
    client.closed()?;
    Ok(())
}

#[test]
fn paste_connections_have_deadlines_and_a_cap() -> Result<(), Box<dyn Error>> {
    let stage = Stage::new()?;
    stage.run(OWNER, &open())?;
    let takeover = taken(stage.take())?;

    // Two connections are served; one more is dropped as it is accepted.
    let mut served = Vec::new();
    for _ in 0..PASTE_CONNECTIONS {
        served.push(Client::open(&stage)?);
    }
    let mut extra = Client::raw(&stage)?;
    extra.closed()?;
    // A place is free again when a connection is over.
    served.pop();
    let mut next = None;
    eventually(|| {
        next = Client::raw(&stage)
            .ok()
            .filter(|_| true)
            .and_then(|mut client| match client.heard(SILENCE) {
                Ok(Heard::Line(_)) => Some(client),
                _ => None,
            });
        next.is_some()
    })?;
    let mut extra = Client::raw(&stage)?;
    extra.closed()?;
    // The connections that were served all along still work.
    let mut kept = served.pop().ok_or("no connection")?;
    kept.auth()?;
    kept.paste(b"kept")?;
    assert_eq!(kept.answer()?, "done");
    drop(next);
    assert_eq!(inserted(&stage), ["kept"]);
    eventually(|| Client::open(&stage).is_ok())?;

    // Twelve connections run out of time. None of them is a wrong answer:
    // the listener still takes nine of those and goes with the tenth.
    for _ in 0..4 {
        stalls(&stage)?;
    }
    assert_eq!(inserted(&stage), ["kept"]);
    for _ in 0..PASTE_AUTH_FAILURES - 1 {
        wrong(&stage, zeros)?;
    }
    assert_eq!(pasted(&stage, b"still")?, "done");
    wrong(&stage, zeros)?;
    assert!(fs::symlink_metadata(socket(&stage)?).is_err());
    assert_eq!(stage.routes().as_deref(), Some(VIEW_ROUTE));
    assert!(stage.user());
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    assert_eq!(inserted(&stage), ["kept", "still"]);

    // The episode ends under a connection that authenticated and one that
    // did not: the first is told its text has nowhere to go.
    let takeover = taken(stage.take())?;
    let mut waiting = trusted(&stage)?;
    let mut stranger = Client::open(&stage)?;
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    assert_eq!(waiting.answer()?, "error not_user");
    waiting.closed()?;
    stranger.closed()?;

    // A text that is going in holds the browser; the return waits for it.
    let takeover = taken(stage.take())?;
    let mut client = trusted(&stage)?;
    let page = stage.page.lock().expect("page");
    client.paste(b"in flight")?;
    thread::sleep(SETTLE);
    let returning = {
        let host = stage.host.clone();
        let target = BrowserReturnTarget::Label(takeover.label.clone());
        thread::spawn(move || host.return_takeover(&target))
    };
    thread::sleep(SETTLE);
    assert!(!returning.is_finished());
    client.quiet()?;
    drop(page);
    assert_eq!(client.answer()?, "done");
    eventually(|| returning.is_finished())?;
    assert_eq!(
        returning.join().map_err(|_| "the return panicked")?,
        BrowserReturnOutcome::Returned
    );
    assert_eq!(inserted(&stage), ["kept", "still", "in flight"]);

    // A text that arrives while the return is switching the input off (the
    // route is gone, the listener not yet) is not inserted.
    let takeover = taken(stage.take())?;
    let mut client = trusted(&stage)?;
    let mut late = client.stream.try_clone()?;
    stage.when_proxy_stops(move || {
        let _ = late.write_all(b"paste 4\nlate");
        thread::sleep(SETTLE);
    });
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Returned);
    assert_eq!(client.answer()?, "error not_user");
    client.closed()?;
    assert_eq!(inserted(&stage).len(), 3);

    // The time limit ends the episode the same way.
    let takeover = taken(stage.take())?;
    stage.set_clock(stage.now() + TAKEOVER_MS - 1);
    stage.upkeep();
    assert!(stage.user());
    let mut client = trusted(&stage)?;
    stage.set_clock(stage.now() + 1);
    stage.upkeep();
    assert!(!stage.user());
    assert_eq!(client.answer()?, "error not_user");
    client.closed()?;
    assert_eq!(stage.give_back(&takeover), BrowserReturnOutcome::Over);

    // The browser closes under an authenticated connection.
    let _takeover = taken(stage.take())?;
    let mut client = trusted(&stage)?;
    let mut sending = trusted(&stage)?;
    sending.say(b"paste 10\nabc")?;
    thread::sleep(SETTLE);
    // A close that finds upkeep holding the browser is carried out by it.
    assert_ne!(stage.host.close_by_user(), BrowserCloseOutcome::NothingOpen);
    eventually(|| !stage.user())?;
    assert_eq!(client.answer()?, "error browser_closed");
    client.closed()?;
    assert_eq!(sending.answer()?, "error browser_closed");
    sending.closed()?;
    assert_eq!(inserted(&stage).len(), 3);
    Ok(())
}
