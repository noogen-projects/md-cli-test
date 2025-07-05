use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::error::{self, TestError};

pub fn split_command_parts(command_line: &str) -> Vec<&str> {
    static REGEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r##"r?#"(?:.|\n)*"#|r?"(?:[^"]+)"|\S+"##).expect("regex must be correct"));

    REGEX
        .find_iter(command_line)
        .map(|found| {
            found
                .as_str()
                .trim_start_matches("r#\"")
                .trim_matches('#')
                .trim_matches('"')
        })
        .collect()
}

#[derive(Debug)]
pub enum Cmd {
    Cd(PathBuf),
    Ls(PathBuf),
    Mkdir(Vec<PathBuf>),
    Rm(Vec<PathBuf>),
    Echo(String, Option<PathBuf>),
    Cat(PathBuf, Option<PathBuf>),
}

pub enum CmdResponse {
    Success,
    ChangeDirTo(PathBuf),
    Output(String),
}

impl Cmd {
    pub fn parse(root_dir: impl AsRef<Path>, source: &str) -> Result<Self, Vec<&str>> {
        let root_dir = root_dir.as_ref();
        let parts = split_command_parts(source);

        let cmd = match &parts[..] {
            ["cd", path] => Self::Cd(checked_join(root_dir, path)),
            ["ls", path] => Self::Ls(checked_join(root_dir, path)),
            ["mkdir", pathes @ ..] => Self::Mkdir(pathes.iter().map(|path| checked_join(root_dir, path)).collect()),
            ["rm", pathes @ ..] => Self::Rm(pathes.iter().map(|path| checked_join(root_dir, path)).collect()),
            ["echo", text @ .., ">", path] => Self::Echo(text.to_vec().join(" "), Some(checked_join(root_dir, path))),
            ["echo", text @ ..] => Self::Echo(text.to_vec().join(" "), None),
            ["cat", from_path, ">", to_path] => {
                Self::Cat(checked_join(root_dir, from_path), Some(checked_join(root_dir, to_path)))
            },
            ["cat", path] => Self::Cat(checked_join(root_dir, path), None),
            _ => return Err(parts),
        };
        Ok(cmd)
    }

    pub fn run(self) -> error::Result<CmdResponse> {
        match self {
            Self::Cd(path) => cd(path),
            Self::Ls(path) => ls(path),
            Self::Mkdir(pathes) => mkdir(pathes),
            Self::Rm(pathes) => rm(pathes),
            Self::Echo(text, path) => echo(text, path),
            Self::Cat(from, to) => cat(from, to),
        }
    }
}

fn checked_join(root: impl AsRef<Path>, subpath: impl AsRef<Path>) -> PathBuf {
    let root = root.as_ref();
    let path = normalize_path(root.join(subpath));

    if path.starts_with(root) {
        path
    } else {
        panic!("Path `{}` is not a subpath of `{}`", path.display(), root.display())
    }
}

/// Normalize a path, removing things like `.` and `..`. This does not resolve symlinks (unlike
/// `std::fs::canonicalize`) and does not checking that path exists.
///
/// Taken from:
/// https://github.com/rust-lang/cargo/blob/e4162389d67c603d25ba6e25b0e9423fcb8daa64/crates/cargo-util/src/paths.rs#L84
fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    let mut components = path.as_ref().components().peekable();
    let mut normalized = if let Some(comp @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        PathBuf::from(comp.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                normalized.push(Component::RootDir);
            },
            Component::CurDir => {},
            Component::ParentDir => {
                if normalized.ends_with(Component::ParentDir) {
                    normalized.push(Component::ParentDir);
                } else {
                    let popped = normalized.pop();
                    if !popped && !normalized.has_root() {
                        normalized.push(Component::ParentDir);
                    }
                }
            },
            Component::Normal(chunk) => {
                normalized.push(chunk);
            },
        }
    }
    normalized
}

fn cd(path: PathBuf) -> error::Result<CmdResponse> {
    if path.is_dir() {
        Ok(CmdResponse::ChangeDirTo(path))
    } else {
        Err(TestError::Command(format!("Path `{}` is not dir", path.display())))
    }
}

fn ls(path: PathBuf) -> error::Result<CmdResponse> {
    let mut entries = Vec::new();

    for entry in fs::read_dir(&path)? {
        let entry_path = entry?.path();
        let entry = entry_path
            .strip_prefix(&path)
            .map_err(|_| {
                TestError::Command(format!(
                    "Could not strip prefix {} for path: {}",
                    path.display(),
                    entry_path.display()
                ))
            })?
            .display()
            .to_string();
        entries.push(entry);
    }

    entries.sort();

    let mut output = entries.join(" ");
    output.push('\n');

    Ok(CmdResponse::Output(output))
}

fn mkdir(pathes: Vec<PathBuf>) -> error::Result<CmdResponse> {
    for path in pathes {
        fs::create_dir_all(&path)
            .map_err(|err| TestError::Command(format!("Failed to create directory `{}`: {err}", path.display())))?;
    }
    Ok(CmdResponse::Success)
}

fn rm(pathes: Vec<PathBuf>) -> error::Result<CmdResponse> {
    for path in pathes {
        if path.is_dir() {
            fs::remove_dir_all(&path)
                .map_err(|err| TestError::Command(format!("Failed to remove directory `{}`: {err}", path.display())))?;
        } else {
            fs::remove_file(&path)
                .map_err(|err| TestError::Command(format!("Failed to remove file `{}`: {err}", path.display())))?;
        }
    }
    Ok(CmdResponse::Success)
}

fn echo(text: String, path: Option<PathBuf>) -> error::Result<CmdResponse> {
    if let Some(path) = path {
        fs::write(&path, text)
            .map_err(|err| TestError::Command(format!("Failed to write file `{}`: {err}", path.display())))?;
        Ok(CmdResponse::Success)
    } else {
        Ok(CmdResponse::Output(text))
    }
}

fn cat(from_path: PathBuf, to_path: Option<PathBuf>) -> error::Result<CmdResponse> {
    let content = fs::read_to_string(&from_path)
        .map_err(|err| TestError::Command(format!("Failed to read file `{}`: {err}", from_path.display())))?;
    echo(content, to_path)
}
