use sha2::{Digest, Sha256};
use shacs_core::memory_search::{
    self, Embedder, EmbedderLoader, EmbeddingError, FakeEmbedder, FakeEmbedding, FakeLoader,
    HuggingFaceSource, IndexModel, InstallError, InstalledFile, LazyEmbedder, LoadError,
    MemorySearchPaths, ModelArchitecture, ModelFile, ModelFileError, ModelFileSource,
    ModelManifest, Pooling, VerifiedModelLoader,
};
use shacs_core::tools::NetworkGuard;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;
use std::{fs, thread};

const FILES: [(&str, &[u8]); 6] = [
    ("config.json", b"{\"hidden_size\": 4}"),
    ("tokenizer.json", b"{\"model\": \"tokenizer\"}"),
    ("model.safetensors", b"weights weights weights"),
    ("1_Pooling/config.json", b"{\"pooling\": \"mean\"}"),
    ("modules.json", b"[]"),
    ("sentence_bert_config.json", b"{}"),
];
const WEIGHTS: usize = 2;

fn manifest() -> &'static ModelManifest {
    Box::leak(Box::new(ModelManifest {
        id: "test/model",
        revision: "rev1",
        architecture: ModelArchitecture::Bert,
        dimension: 4,
        query_prefix: "",
        passage_prefix: "",
        pooling: Pooling::Mean,
        max_tokens: 8,
        files: FILES.map(|(name, bytes)| ModelFile {
            name,
            size: bytes.len() as u64,
            sha256: Box::leak(format!("{:x}", Sha256::digest(bytes)).into_boxed_str()),
        }),
    }))
}

struct TestSource {
    bodies: BTreeMap<String, Vec<u8>>,
    cut_after: Option<(&'static str, usize)>,
    fetched: RefCell<Vec<(String, u64)>>,
}

impl TestSource {
    fn new(manifest: &ModelManifest) -> Self {
        let bodies = manifest
            .files
            .iter()
            .zip(FILES)
            .map(|(file, (_, bytes))| (manifest.download_url(file), bytes.to_vec()))
            .collect();
        Self {
            bodies,
            cut_after: None,
            fetched: RefCell::new(Vec::new()),
        }
    }

    fn replace(mut self, manifest: &ModelManifest, name: &str, body: &[u8]) -> Self {
        if let Some(file) = manifest.file(name) {
            self.bodies
                .insert(manifest.download_url(file), body.to_vec());
        }
        self
    }
}

impl ModelFileSource for TestSource {
    fn fetch(&self, url: &str, limit: u64, sink: &mut dyn Write) -> Result<(), String> {
        self.fetched.borrow_mut().push((url.to_owned(), limit));
        let body = self.bodies.get(url).ok_or("404 not found")?;
        let body = &body[..body.len().min(usize::try_from(limit).unwrap_or(usize::MAX))];
        if let Some((name, after)) = self.cut_after {
            if url.ends_with(name) {
                sink.write_all(&body[..after.min(body.len())])
                    .map_err(|error| error.to_string())?;
                return Err("connection reset".to_owned());
            }
        }
        sink.write_all(body).map_err(|error| error.to_string())
    }
}

fn paths(root: &Path) -> Result<MemorySearchPaths, Box<dyn Error>> {
    Ok(MemorySearchPaths::new(root)?)
}

fn files_below(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if fs::symlink_metadata(&path)?.is_dir() {
                pending.push(path);
            } else {
                found.push(path.strip_prefix(dir)?.to_path_buf());
            }
        }
    }
    found.sort();
    Ok(found)
}

fn verified_files() -> [PathBuf; 2] {
    [
        PathBuf::from("config.json"),
        PathBuf::from("tokenizer.json"),
    ]
}

#[cfg(unix)]
fn mode(path: &Path) -> Result<u32, Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::symlink_metadata(path)?.permissions().mode() & 0o777)
}

#[test]
fn install_downloads_each_file_once_and_keeps_verified_files_on_the_next_run(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    let source = TestSource::new(manifest);

    // When
    let first = memory_search::install_model(&paths, manifest, &source)?;
    let fetched = source.fetched.borrow().clone();
    let second = memory_search::install_model(&paths, manifest, &source)?;

    // Then
    assert_eq!(first.model_dir, paths.model_dir(manifest));
    assert!(first
        .files
        .iter()
        .all(|(_, state)| *state == InstalledFile::Downloaded));
    assert!(second
        .files
        .iter()
        .all(|(_, state)| *state == InstalledFile::AlreadyVerified));
    assert_eq!(*source.fetched.borrow(), fetched);
    let expected: Vec<(String, u64)> = FILES
        .iter()
        .map(|(name, bytes)| {
            (
                format!("https://huggingface.co/test/model/resolve/rev1/{name}"),
                bytes.len() as u64 + 1,
            )
        })
        .collect();
    assert_eq!(fetched, expected);
    let model = memory_search::read_verified_model(&paths, manifest)?;
    assert_eq!(model.config, FILES[0].1);
    assert_eq!(model.tokenizer, FILES[1].1);
    assert_eq!(model.weights, FILES[WEIGHTS].1);
    Ok(())
}

#[test]
fn an_interrupted_or_wrong_download_never_takes_a_final_name() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    let dir = paths.model_dir(manifest);
    let mut interrupted = TestSource::new(manifest);
    interrupted.cut_after = Some(("model.safetensors", 5));

    // When
    let cut = memory_search::install_model(&paths, manifest, &interrupted);

    // Then
    assert!(matches!(
        cut,
        Err(InstallError::Download {
            file: "model.safetensors",
            ..
        })
    ));
    assert_eq!(files_below(&dir)?, verified_files());

    // Given
    let left_behind = dir.join(".model.safetensors.partial-999-0");
    fs::write(&left_behind, b"half")?;
    let mut same_size = FILES[WEIGHTS].1.to_vec();
    same_size[0] = b'W';
    let longer = [FILES[WEIGHTS].1, b"!"].concat();

    // When
    let digest_differs = memory_search::install_model(
        &paths,
        manifest,
        &TestSource::new(manifest).replace(manifest, "model.safetensors", &same_size),
    );
    let too_long = memory_search::install_model(
        &paths,
        manifest,
        &TestSource::new(manifest).replace(manifest, "model.safetensors", &longer),
    );

    // Then
    for refused in [digest_differs, too_long] {
        assert!(matches!(
            refused,
            Err(InstallError::Verification {
                file: "model.safetensors",
                ..
            })
        ));
    }
    assert_eq!(files_below(&dir)?, verified_files());

    // When
    let report = memory_search::install_model(&paths, manifest, &TestSource::new(manifest))?;

    // Then
    let states: BTreeMap<&str, InstalledFile> = report.files.iter().copied().collect();
    assert_eq!(states["config.json"], InstalledFile::AlreadyVerified);
    assert_eq!(states["tokenizer.json"], InstalledFile::AlreadyVerified);
    assert_eq!(states["model.safetensors"], InstalledFile::Downloaded);
    assert!(memory_search::read_verified_model(&paths, manifest).is_ok());
    Ok(())
}

#[test]
fn a_model_file_is_used_only_when_its_size_and_digest_match() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    memory_search::install_model(&paths, manifest, &TestSource::new(manifest))?;
    let config = paths.model_dir(manifest).join("config.json");

    // When / Then
    let mut same_size = FILES[0].1.to_vec();
    same_size[0] = b'[';
    fs::write(&config, &same_size)?;
    assert!(matches!(
        memory_search::read_verified_model(&paths, manifest),
        Err(ModelFileError::Unverified { file: "config.json", reason }) if reason.contains("SHA-256")
    ));
    fs::write(&config, b"{}")?;
    assert!(matches!(
        memory_search::read_verified_model(&paths, manifest),
        Err(ModelFileError::Unverified { file: "config.json", reason }) if reason.contains("size")
    ));
    fs::remove_file(&config)?;
    assert!(matches!(
        memory_search::read_verified_model(&paths, manifest),
        Err(ModelFileError::Missing {
            file: "config.json"
        })
    ));
    fs::write(&config, b"{}")?;
    let report = memory_search::install_model(&paths, manifest, &TestSource::new(manifest))?;
    assert!(report
        .files
        .contains(&("config.json", InstalledFile::Downloaded)));

    // When: the right bytes are put in place by hand.
    fs::remove_file(&config)?;
    fs::write(&config, FILES[0].1)?;
    let source = TestSource::new(manifest);
    let report = memory_search::install_model(&paths, manifest, &source)?;

    // Then
    assert!(report
        .files
        .iter()
        .all(|(_, state)| *state == InstalledFile::AlreadyVerified));
    assert!(source.fetched.borrow().is_empty());
    assert!(memory_search::read_verified_model(&paths, manifest).is_ok());
    #[cfg(unix)]
    assert_eq!(mode(&config)?, 0o600);
    Ok(())
}

#[test]
fn one_install_runs_at_a_time_and_keeps_the_temporary_files_of_another(
) -> Result<(), Box<dyn Error>> {
    use fs2::FileExt;

    struct LockProbe {
        inner: TestSource,
        lock: PathBuf,
        held: RefCell<Vec<bool>>,
    }

    impl ModelFileSource for LockProbe {
        fn fetch(&self, url: &str, limit: u64, sink: &mut dyn Write) -> Result<(), String> {
            let probe = fs::OpenOptions::new()
                .write(true)
                .open(&self.lock)
                .map_err(|error| error.to_string())?;
            self.held
                .borrow_mut()
                .push(probe.try_lock_exclusive().is_err());
            self.inner.fetch(url, limit, sink)
        }
    }

    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    let probe = LockProbe {
        inner: TestSource::new(manifest),
        lock: paths.models_dir().join(".install.lock"),
        held: RefCell::new(Vec::new()),
    };

    // When
    memory_search::install_model(&paths, manifest, &probe)?;

    // Then
    assert_eq!(*probe.held.borrow(), [true; FILES.len()]);

    // Given: another install is downloading.
    let downloading = paths
        .model_dir(manifest)
        .join(".model.safetensors.partial-1-0");
    fs::write(&downloading, b"half")?;
    let running = fs::OpenOptions::new()
        .write(true)
        .open(paths.models_dir().join(".install.lock"))?;
    running.try_lock_exclusive()?;
    let source = TestSource::new(manifest);

    // When
    let refused = memory_search::install_model(&paths, manifest, &source);
    let kept = downloading.exists();
    drop(running);
    let after = memory_search::install_model(&paths, manifest, &source);

    // Then
    assert!(matches!(refused, Err(InstallError::Busy)));
    assert!(kept);
    assert!(after.is_ok());
    assert!(!downloading.exists());
    assert!(source.fetched.borrow().is_empty());
    Ok(())
}

#[cfg(unix)]
#[test]
fn model_folders_are_0700_and_model_and_temporary_files_0600() -> Result<(), Box<dyn Error>> {
    struct Watching {
        inner: TestSource,
        dir: PathBuf,
        temporary_modes: RefCell<Vec<u32>>,
    }

    impl ModelFileSource for Watching {
        fn fetch(&self, url: &str, limit: u64, sink: &mut dyn Write) -> Result<(), String> {
            let files = files_below(&self.dir).map_err(|error| error.to_string())?;
            for file in files
                .iter()
                .filter(|file| file.to_string_lossy().contains(".partial-"))
            {
                let mode = mode(&self.dir.join(file)).map_err(|error| error.to_string())?;
                self.temporary_modes.borrow_mut().push(mode);
            }
            self.inner.fetch(url, limit, sink)
        }
    }

    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    let dir = paths.model_dir(manifest);
    let source = Watching {
        inner: TestSource::new(manifest),
        dir: dir.clone(),
        temporary_modes: RefCell::new(Vec::new()),
    };

    // When
    memory_search::install_model(&paths, manifest, &source)?;

    // Then
    assert_eq!(*source.temporary_modes.borrow(), [0o600; FILES.len()]);
    let memory_search_dir = paths
        .models_dir()
        .parent()
        .ok_or("no parent")?
        .to_path_buf();
    for folder in [
        memory_search_dir,
        paths.models_dir(),
        dir.clone(),
        dir.join("1_Pooling"),
    ] {
        assert_eq!(mode(&folder)?, 0o700, "{}", folder.display());
    }
    for (name, _) in FILES {
        assert_eq!(mode(&dir.join(name))?, 0o600, "{name}");
    }
    Ok(())
}

#[test]
fn a_wrong_file_under_a_final_name_is_removed_even_when_its_download_fails(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    memory_search::install_model(&paths, manifest, &TestSource::new(manifest))?;
    let dir = paths.model_dir(manifest);
    let mut same_size = FILES[WEIGHTS].1.to_vec();
    same_size[0] = b'W';
    fs::write(dir.join("model.safetensors"), &same_size)?;
    let mut interrupted = TestSource::new(manifest);
    interrupted.cut_after = Some(("model.safetensors", 5));

    // When
    let cut = memory_search::install_model(&paths, manifest, &interrupted);

    // Then
    assert!(matches!(
        cut,
        Err(InstallError::Download {
            file: "model.safetensors",
            ..
        })
    ));
    let kept: Vec<PathBuf> = [
        "1_Pooling/config.json",
        "config.json",
        "modules.json",
        "sentence_bert_config.json",
        "tokenizer.json",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    assert_eq!(files_below(&dir)?, kept);

    // Given: a folder where a model file belongs.
    fs::remove_file(dir.join("modules.json"))?;
    fs::create_dir(dir.join("modules.json"))?;

    // When
    let refused = memory_search::install_model(&paths, manifest, &TestSource::new(manifest));

    // Then
    assert!(matches!(
        refused,
        Err(InstallError::Verification { file: "modules.json", reason })
            if reason.contains("not a regular file")
    ));
    assert!(dir.join("modules.json").is_dir());
    assert!(dir.join("model.safetensors").is_file());
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_model_placed_by_hand_is_made_private_when_read_and_a_missing_one_creates_nothing(
) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    let dir = paths.model_dir(manifest);

    // When
    let missing = memory_search::read_verified_model(&paths, manifest);

    // Then
    assert!(matches!(
        missing,
        Err(ModelFileError::Missing {
            file: "config.json"
        })
    ));
    assert!(!root.path().join("runtime").exists());

    // Given
    fs::create_dir_all(dir.join("1_Pooling"))?;
    for (name, bytes) in FILES {
        fs::write(dir.join(name), bytes)?;
        fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o644))?;
    }
    let folders = [
        paths
            .models_dir()
            .parent()
            .ok_or("no parent")?
            .to_path_buf(),
        paths.models_dir(),
        dir.clone(),
        dir.join("1_Pooling"),
    ];
    for folder in &folders {
        fs::set_permissions(folder, fs::Permissions::from_mode(0o755))?;
    }

    // When
    let model = memory_search::read_verified_model(&paths, manifest);

    // Then
    assert!(model.is_ok());
    for folder in &folders {
        assert_eq!(mode(folder)?, 0o700, "{}", folder.display());
    }
    for (name, _) in FILES {
        assert_eq!(mode(&dir.join(name))?, 0o600, "{name}");
    }
    Ok(())
}

#[cfg(unix)]
fn make_fifo(path: &Path) -> Result<(), Box<dyn Error>> {
    let status = std::process::Command::new("mkfifo").arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("mkfifo failed: {status}").into())
    }
}

fn within_ten_seconds<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Box<dyn Error>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(work());
    });
    Ok(receiver.recv_timeout(Duration::from_secs(10))?)
}

#[cfg(unix)]
#[test]
fn a_fifo_in_place_of_a_model_file_or_the_install_lock_is_refused_without_waiting(
) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::FileTypeExt;

    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    memory_search::install_model(&paths, manifest, &TestSource::new(manifest))?;
    let weights = paths.model_dir(manifest).join("model.safetensors");
    fs::remove_file(&weights)?;
    make_fifo(&weights)?;

    // When
    let read = within_ten_seconds({
        let paths = paths.clone();
        move || {
            memory_search::read_verified_model(&paths, manifest)
                .err()
                .map(|error| error.to_string())
        }
    })?;
    let installed = within_ten_seconds({
        let paths = paths.clone();
        move || {
            memory_search::install_model(&paths, manifest, &TestSource::new(manifest))
                .err()
                .map(|error| error.to_string())
        }
    })?;

    // Then
    assert!(read.is_some_and(|reason| reason.contains("not a regular file")));
    assert!(installed.is_some_and(|reason| reason.contains("not a regular file")));
    assert!(fs::symlink_metadata(&weights)?.file_type().is_fifo());

    // Given: the install lock is a FIFO.
    let lock = paths.models_dir().join(".install.lock");
    fs::remove_file(&lock)?;
    make_fifo(&lock)?;

    // When
    let locked = within_ten_seconds({
        let paths = paths.clone();
        move || {
            memory_search::install_model(&paths, manifest, &TestSource::new(manifest))
                .err()
                .map(|error| error.to_string())
        }
    })?;

    // Then
    assert!(locked.is_some_and(|reason| reason.contains("not a file")));
    Ok(())
}

struct LocalServer {
    port: u16,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl LocalServer {
    fn start() -> Result<Self, Box<dyn Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let requests = requests.clone();
            let stop = stop.clone();
            move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        requests.fetch_add(1, Ordering::SeqCst);
                        let _ = answer(stream, port);
                    }
                }
            }
        });
        Ok(Self {
            port,
            requests,
            stop,
            thread: Some(thread),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn answer(mut stream: TcpStream, port: u16) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    let mut header = String::new();
    while reader.read_line(&mut header)? > 2 {
        header.clear();
    }
    let path = request.split_whitespace().nth(1).unwrap_or_default();
    let chain = path
        .strip_prefix("/chain/")
        .and_then(|step| step.parse::<u32>().ok());
    let (status, location, body) = match (path, chain) {
        (_, Some(step)) if step < 10 => (
            "302 Found",
            format!("http://127.0.0.1:{port}/chain/{}", step + 1),
            "",
        ),
        (_, Some(_)) => (
            "302 Found",
            "http://169.254.169.254/latest/meta-data".to_owned(),
            "",
        ),
        ("/file", _) => ("200 OK", String::new(), "0123456789"),
        ("/to-file", _) => ("302 Found", format!("http://127.0.0.1:{port}/file"), ""),
        ("/to-metadata", _) => (
            "302 Found",
            "http://169.254.169.254/latest/meta-data".to_owned(),
            "",
        ),
        ("/loop", _) => ("302 Found", "/loop".to_owned(), ""),
        _ => ("404 Not Found", String::new(), ""),
    };
    let location = if location.is_empty() {
        String::new()
    } else {
        format!("Location: {location}\r\n")
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn hugging_face_source_checks_every_request_and_redirect_with_the_network_guard(
) -> Result<(), Box<dyn Error>> {
    // Given
    let server = LocalServer::start()?;
    let guarded = HuggingFaceSource::new(NetworkGuard::default());
    let local = HuggingFaceSource::new(NetworkGuard::with_ssrf_whitelist(["127.0.0.1/32"]));
    let mut body = Vec::new();

    // When / Then: the default guard refuses a local address before connecting.
    let refused = guarded.fetch(&server.url("/file"), 11, &mut body);
    assert!(refused.is_err_and(|reason| reason.contains("Blocked")));
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);

    // When / Then: an allowed redirect is followed and the body read.
    local.fetch(&server.url("/to-file"), 11, &mut body)?;
    assert_eq!(body, b"0123456789");
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);

    // When / Then: a body over the limit, a redirect to a blocked address and a loop fail.
    assert!(local
        .fetch(&server.url("/file"), 4, &mut Vec::new())
        .is_err());
    let blocked = local.fetch(&server.url("/to-metadata"), 11, &mut Vec::new());
    assert!(blocked.is_err_and(|reason| reason.contains("Blocked")));
    let looping = local.fetch(&server.url("/loop"), 11, &mut Vec::new());
    assert!(looping.is_err_and(|reason| reason.contains("redirects")));

    // When / Then: the target of the last redirect allowed is checked too.
    let requests = server.requests.load(Ordering::SeqCst);
    let last = local.fetch(&server.url("/chain/0"), 11, &mut Vec::new());
    assert!(last.is_err_and(|reason| reason.contains("Blocked")));
    assert_eq!(server.requests.load(Ordering::SeqCst) - requests, 11);
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_unreadable_model_file_is_made_private_and_checked_or_replaced() -> Result<(), Box<dyn Error>>
{
    use std::os::unix::fs::PermissionsExt;

    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    memory_search::install_model(&paths, manifest, &TestSource::new(manifest))?;
    let weights = paths.model_dir(manifest).join("model.safetensors");
    fs::set_permissions(&weights, fs::Permissions::from_mode(0o000))?;

    // When
    let read = memory_search::read_verified_model(&paths, manifest);

    // Then
    assert!(read.is_ok());
    assert_eq!(mode(&weights)?, 0o600);

    // Given: a wrong file nobody can read.
    fs::write(&weights, b"wrong weights")?;
    fs::set_permissions(&weights, fs::Permissions::from_mode(0o000))?;

    // When
    let report = memory_search::install_model(&paths, manifest, &TestSource::new(manifest))?;

    // Then
    assert!(report
        .files
        .contains(&("model.safetensors", InstalledFile::Downloaded)));
    assert_eq!(mode(&weights)?, 0o600);
    assert!(memory_search::read_verified_model(&paths, manifest).is_ok());
    Ok(())
}

#[cfg(unix)]
#[test]
fn model_paths_that_are_symbolic_links_are_refused() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    memory_search::install_model(&paths, manifest, &TestSource::new(manifest))?;
    let dir = paths.model_dir(manifest);
    let outside = root.path().join("outside.safetensors");
    fs::write(&outside, FILES[WEIGHTS].1)?;
    fs::remove_file(dir.join("model.safetensors"))?;
    symlink(&outside, dir.join("model.safetensors"))?;

    // When / Then
    assert!(matches!(
        memory_search::read_verified_model(&paths, manifest),
        Err(ModelFileError::Unsafe { .. })
    ));
    assert!(matches!(
        memory_search::install_model(&paths, manifest, &TestSource::new(manifest)),
        Err(InstallError::Unsafe(_))
    ));

    // Given: the models folder itself is a link.
    let other = tempfile::tempdir()?;
    let other_paths = MemorySearchPaths::new(other.path())?;
    let elsewhere = other.path().join("elsewhere");
    fs::create_dir_all(&elsewhere)?;
    fs::create_dir_all(other.path().join("runtime").join("memory-search"))?;
    symlink(&elsewhere, other_paths.models_dir())?;

    // When / Then
    assert!(matches!(
        memory_search::install_model(&other_paths, manifest, &TestSource::new(manifest)),
        Err(InstallError::Unsafe(_))
    ));
    assert!(matches!(
        memory_search::read_verified_model(&other_paths, manifest),
        Err(ModelFileError::Unsafe { .. })
    ));
    assert_eq!(fs::read_dir(&elsewhere)?.count(), 0);
    Ok(())
}

#[test]
fn a_model_is_built_only_from_verified_files_and_each_failure_says_which_step_failed(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let paths = paths(root.path())?;
    let manifest = manifest();
    let loader = VerifiedModelLoader::new(paths.clone(), manifest);

    // When
    let missing = loader.load();
    memory_search::install_model(&paths, manifest, &TestSource::new(manifest))?;
    let not_a_model = loader.load();

    // Then
    assert_eq!(loader.model(), IndexModel::from_manifest(manifest));
    assert!(matches!(
        missing,
        Err(LoadError::File(ModelFileError::Missing {
            file: "config.json"
        }))
    ));
    assert!(matches!(
        not_a_model,
        Err(LoadError::Model(EmbeddingError::Load(_)))
    ));
    Ok(())
}

fn fake() -> FakeEmbedder {
    FakeEmbedder::new("test/fake", "r1", 4, FakeEmbedding::Hashed)
}

struct CountingLoader {
    loads: AtomicUsize,
    failures: usize,
}

impl EmbedderLoader for CountingLoader {
    fn model(&self) -> IndexModel {
        FakeLoader::new(fake()).model()
    }

    fn load(&self) -> Result<Arc<dyn Embedder>, LoadError> {
        let load = self.loads.fetch_add(1, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(20));
        if load < self.failures {
            FakeLoader::failing(fake()).load()
        } else {
            FakeLoader::new(fake()).load()
        }
    }
}

#[test]
fn a_model_is_loaded_on_first_use_shared_afterwards_and_loaded_again_after_a_failure(
) -> Result<(), Box<dyn Error>> {
    // Given
    let loader = Arc::new(CountingLoader {
        loads: AtomicUsize::new(0),
        failures: 1,
    });
    let lazy = LazyEmbedder::new(loader.clone());

    // When
    let failed = lazy.get();
    let first = lazy.get()?;
    let again = lazy.get()?;

    // Then
    assert!(matches!(
        failed,
        Err(LoadError::Model(EmbeddingError::Load(_)))
    ));
    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!(loader.loads.load(Ordering::SeqCst), 2);
    assert_eq!(lazy.model(), IndexModel::of(first.as_ref()));
    Ok(())
}

#[test]
fn callers_that_arrive_while_the_model_loads_share_that_one_load() -> Result<(), Box<dyn Error>> {
    // Given
    let loader = Arc::new(CountingLoader {
        loads: AtomicUsize::new(0),
        failures: 0,
    });
    let lazy = LazyEmbedder::new(loader.clone());
    let callers = 8;
    let start = Barrier::new(callers);

    // When
    let embedders = thread::scope(|scope| {
        let handles: Vec<_> = (0..callers)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    lazy.get().map_err(|error| error.to_string())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().map_err(|_| "a caller panicked".to_owned())?)
            .collect::<Result<Vec<_>, String>>()
    })?;

    // Then
    assert_eq!(loader.loads.load(Ordering::SeqCst), 1);
    let first = embedders.first().ok_or("no caller returned")?;
    assert!(embedders
        .iter()
        .all(|embedder| Arc::ptr_eq(embedder, first)));
    Ok(())
}
