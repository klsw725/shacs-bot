use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) struct EvidenceWriter {
    root: Dir,
}

pub(crate) struct EvidenceTempFile {
    path: PathBuf,
    file: std::fs::File,
}

impl EvidenceWriter {
    pub(crate) fn open_new_run(root: &Path) -> io::Result<Self> {
        let writer = Self::open(root)?;
        writer.require_empty()?;
        Ok(writer)
    }

    pub(crate) fn open_existing(root: &Path) -> io::Result<Self> {
        Self::open(root)
    }

    pub(crate) fn create_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        self.open_dir_all(path.as_ref()).map(|_| ())
    }

    pub(crate) fn write_new(&self, path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
        let mut temp = self.create_temp_file(path.as_ref())?;
        temp.file.write_all(bytes)?;
        temp.file.sync_all()?;
        drop(temp.file);
        self.publish_temp(&temp.path, path.as_ref())
    }

    pub(crate) fn create_temp_file(&self, final_path: &Path) -> io::Result<EvidenceTempFile> {
        let (parent_path, final_name) = split_relative_file(final_path)?;
        let parent = self.open_dir_all(&parent_path)?;
        ensure_missing(&parent, &final_name)?;
        let temp_name = temp_name(&final_name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let file = parent.open_with(&temp_name, &options)?.into_std();
        Ok(EvidenceTempFile {
            path: parent_path.join(temp_name),
            file,
        })
    }

    pub(crate) fn publish_temp(&self, temp: &Path, final_path: &Path) -> io::Result<()> {
        let (parent_path, final_name) = split_relative_file(final_path)?;
        let parent = self.open_dir_all(&parent_path)?;
        ensure_missing(&parent, &final_name)?;
        self.root.rename(temp, &self.root, final_path)
    }

    pub(crate) fn read_to_string(&self, path: impl AsRef<Path>) -> io::Result<String> {
        let path = path.as_ref();
        reject_unsafe_relative(path)?;
        self.root.read_to_string(path)
    }

    fn open(root: &Path) -> io::Result<Self> {
        let parent_path = root.parent().unwrap_or_else(|| Path::new("."));
        verify_existing_dir_components(parent_path)?;
        let root_name = root
            .file_name()
            .ok_or_else(|| invalid_path("missing evidence root name"))?;
        let parent = Dir::open_ambient_dir(parent_path, ambient_authority())?;
        match parent.symlink_metadata(root_name) {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(invalid_path("symlink root")),
            Ok(metadata) if metadata.is_dir() => Ok(Self {
                root: parent.open_dir(root_name)?,
            }),
            Ok(_) => Err(invalid_path("non-directory root")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                parent.create_dir(root_name)?;
                Ok(Self {
                    root: parent.open_dir(root_name)?,
                })
            }
            Err(error) => Err(error),
        }
    }

    fn require_empty(&self) -> io::Result<()> {
        if self.root.read_dir(".")?.next().is_some() {
            Err(invalid_path("nonempty evidence root"))
        } else {
            Ok(())
        }
    }

    fn open_dir_all(&self, path: &Path) -> io::Result<Dir> {
        let mut current = self.root.open_dir(".")?;
        for component in normal_components(path)? {
            match current.symlink_metadata(&component) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(invalid_path("symlink directory component"));
                }
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Err(invalid_path("non-directory component")),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    current.create_dir(&component)?;
                }
                Err(error) => return Err(error),
            }
            current = current.open_dir(&component)?;
        }
        Ok(current)
    }
}

impl EvidenceTempFile {
    pub(crate) fn into_std(self) -> std::fs::File {
        self.file
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

fn split_relative_file(path: &Path) -> io::Result<(PathBuf, OsString)> {
    reject_unsafe_relative(path)?;
    let file_name = path
        .file_name()
        .ok_or_else(|| invalid_path("missing file name"))?
        .to_os_string();
    let parent = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    Ok((parent, file_name))
}

fn reject_unsafe_relative(path: &Path) -> io::Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(invalid_path("unsafe relative path"));
    }
    normal_components(path).map(|_| ())
}

fn normal_components(path: &Path) -> io::Result<Vec<OsString>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(value.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid_path("unsafe path component"));
            }
        }
    }
    Ok(components)
}

fn verify_existing_dir_components(path: &Path) -> io::Result<()> {
    let mut cursor = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => cursor.push(prefix.as_os_str()),
            Component::RootDir => cursor.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => return Err(invalid_path("parent component")),
            Component::Normal(value) => {
                cursor.push(value);
                let metadata = std::fs::symlink_metadata(&cursor)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(invalid_path("unsafe parent component"));
                }
            }
        }
    }
    Ok(())
}

fn ensure_missing(parent: &Dir, name: &OsStr) -> io::Result<()> {
    match parent.symlink_metadata(name) {
        Ok(_) => Err(invalid_path("artifact already exists")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn temp_name(final_name: &OsStr) -> OsString {
    let mut name = OsString::from(".");
    name.push(final_name);
    name.push(format!(
        ".tmp.{}.{}",
        std::process::id(),
        NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    name
}

fn invalid_path(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
