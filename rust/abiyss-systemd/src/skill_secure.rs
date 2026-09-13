//! Hardened SkillLE verification for Linux.
//!
//! All reads of untrusted skill members are anchored at a pinned system-root
//! file descriptor and resolved through openat2(2) with traversal and symlink
//! restrictions. This closes the path-resolution side of the verification
//! race; execution is still a separate boundary and is not authorized by a
//! stale verification result.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_FILES: usize = 256;
const MAX_NAME_BYTES: usize = 128;
const MAX_TOKEN_BYTES: usize = 4096;
const MAX_VERSION_BYTES: usize = 128;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TREE_DEPTH: usize = 32;

const SYSTEM_PREFIXES: &[&str] = &["/bin", "/usr/bin", "/usr/local/bin"];
const FORBIDDEN_LAUNCHERS: &[&str] = &[
    "env", "bash", "sh", "dash", "zsh", "fish", "python", "python3",
    "python3.11", "python3.12", "python3.13", "python3.14", "node", "nodejs",
    "ruby", "perl", "php", "lua", "luajit", "java", "tclsh", "awk",
];

const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;

#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    pub name: String,
    pub version: String,
    pub entrypoint: Vec<String>,
    pub files: BTreeMap<String, String>,
    pub allow_root: bool,
    pub timeout_seconds: f64,
    pub max_output_bytes: u64,
    pub max_args: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifiedSkill {
    pub directory: String,
    #[serde(flatten)]
    pub manifest: SkillManifest,
}

#[derive(Debug, Error, Clone)]
pub enum VerifyError {
    #[error("unsafe skill path: {0}")]
    UnsafePath(String),
    #[error("invalid skill manifest: {0}")]
    Manifest(String),
    #[error("skill I/O failure: {0}")]
    Io(String),
    #[error("skill hash mismatch: {0}")]
    HashMismatch(String),
    #[error("skill file set mismatch; missing={missing:?}, unexpected={unexpected:?}")]
    FileSetMismatch {
        missing: Vec<String>,
        unexpected: Vec<String>,
    },
}

impl From<io::Error> for VerifyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

fn io_error(prefix: &str) -> VerifyError {
    VerifyError::Io(format!("{prefix}: {}", io::Error::last_os_error()))
}

fn openat2_fd(dirfd: RawFd, path: &Path, flags: u64, resolve: u64) -> Result<OwnedFd, VerifyError> {
    let bytes = path.as_os_str().as_bytes();
    let c_path = CString::new(bytes)
        .map_err(|_| VerifyError::UnsafePath("path contains NUL".to_string()))?;
    let how = OpenHow { flags, mode: 0, resolve };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            dirfd,
            c_path.as_ptr(),
            &how,
            std::mem::size_of::<OpenHow>(),
        ) as libc::c_int
    };
    if fd < 0 {
        return Err(io_error("openat2 failed"));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn stat_fd(fd: &OwnedFd) -> Result<(u32, u32, u32, u64), VerifyError> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    let rc = unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) };
    if rc != 0 {
        return Err(io_error("fstat failed"));
    }
    let stat = unsafe { stat.assume_init() };
    Ok((
        stat.st_mode as u32,
        stat.st_uid as u32,
        stat.st_gid as u32,
        stat.st_size.max(0) as u64,
    ))
}

fn is_regular(mode: u32) -> bool {
    (mode as libc::mode_t & libc::S_IFMT) == libc::S_IFREG
}

fn is_directory(mode: u32) -> bool {
    (mode as libc::mode_t & libc::S_IFMT) == libc::S_IFDIR
}

fn secure_open(root_fd: RawFd, relative: &Path, flags: u64) -> Result<OwnedFd, VerifyError> {
    if relative.is_absolute() || relative.as_os_str().is_empty() {
        return Err(VerifyError::UnsafePath("secure path must be non-empty and relative".to_string()));
    }
    openat2_fd(
        root_fd,
        relative,
        flags,
        RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS,
    )
}

fn secure_open_absolute(path: &Path, flags: u64) -> Result<OwnedFd, VerifyError> {
    openat2_fd(
        libc::AT_FDCWD,
        path,
        flags,
        RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS,
    )
}

fn open_root(root: &Path) -> Result<OwnedFd, VerifyError> {
    let canonical = fs::canonicalize(root)?;
    let fd = secure_open_absolute(
        &canonical,
        libc::O_PATH as u64 | libc::O_DIRECTORY as u64 | libc::O_CLOEXEC as u64,
    )?;
    let (mode, _, _, _) = stat_fd(&fd)?;
    if !is_directory(mode) {
        return Err(VerifyError::UnsafePath("system root is not a directory".to_string()));
    }
    Ok(fd)
}

fn validate_text(value: &str, max_bytes: usize, label: &str) -> Result<(), VerifyError> {
    if value.is_empty() || value.len() > max_bytes || value.as_bytes().contains(&0) {
        return Err(VerifyError::Manifest(format!("invalid {label}")));
    }
    Ok(())
}

fn validate_member(value: &str) -> Result<(), VerifyError> {
    validate_text(value, MAX_TOKEN_BYTES, "skill file path")?;
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(VerifyError::UnsafePath(format!("absolute member path: {value}")));
    }
    if value == "skill.json" {
        return Err(VerifyError::Manifest("skill.json must not appear in files".to_string()));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(VerifyError::UnsafePath(format!("non-canonical member path: {value}")));
        }
    }
    Ok(())
}

fn validate_manifest(manifest: SkillManifest) -> Result<SkillManifest, VerifyError> {
    validate_text(&manifest.name, MAX_NAME_BYTES, "skill name")?;
    validate_text(&manifest.version, MAX_VERSION_BYTES, "skill version")?;
    if manifest.entrypoint.is_empty() || manifest.entrypoint.len() > 32 {
        return Err(VerifyError::Manifest("invalid entrypoint".to_string()));
    }
    for token in &manifest.entrypoint {
        validate_text(token, MAX_TOKEN_BYTES, "entrypoint token")?;
    }
    if manifest.files.len() > MAX_FILES {
        return Err(VerifyError::Manifest("too many manifest files".to_string()));
    }
    for (path, digest) in &manifest.files {
        validate_member(path)?;
        if digest.len() != 64
            || digest.bytes().any(|byte| !matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(VerifyError::Manifest(format!("invalid SHA-256 for {path}")));
        }
    }
    if !manifest.timeout_seconds.is_finite() || !(0.1..=300.0).contains(&manifest.timeout_seconds) {
        return Err(VerifyError::Manifest("timeout outside policy".to_string()));
    }
    if !(1024..=4 * 1024 * 1024).contains(&manifest.max_output_bytes) {
        return Err(VerifyError::Manifest("output limit outside policy".to_string()));
    }
    if !(1..=32).contains(&manifest.max_args) {
        return Err(VerifyError::Manifest("max_args outside policy".to_string()));
    }
    Ok(manifest)
}

fn read_manifest(root_fd: RawFd, skill_relative: &Path) -> Result<SkillManifest, VerifyError> {
    let path = skill_relative.join("skill.json");
    let fd = secure_open(
        root_fd,
        &path,
        libc::O_RDONLY as u64 | libc::O_CLOEXEC as u64 | libc::O_NONBLOCK as u64,
    )?;
    let (_, _, _, size) = stat_fd(&fd)?;
    if size > MAX_MANIFEST_BYTES {
        return Err(VerifyError::Manifest("manifest too large".to_string()));
    }
    let mut file = fs::File::from(fd);
    let mut raw = Vec::with_capacity(size as usize);
    file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut raw)?;
    if raw.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(VerifyError::Manifest("manifest too large".to_string()));
    }
    let manifest: SkillManifest = serde_json::from_slice(&raw)
        .map_err(|error| VerifyError::Manifest(format!("invalid JSON: {error}")))?;
    validate_manifest(manifest)
}

fn check_private(uid: u32, mode: u32, label: &str, required: bool) -> Result<(), VerifyError> {
    if !required || unsafe { libc::geteuid() } != 0 {
        return Ok(());
    }
    if uid != 0 || mode & 0o022 != 0 {
        return Err(VerifyError::UnsafePath(format!(
            "privileged skill {label} must be root-owned and not group/world writable"
        )));
    }
    Ok(())
}

fn hash_file(root_fd: RawFd, relative: &Path, expected_size: u64) -> Result<(String, (u32, u32, u32, u64)), VerifyError> {
    let fd = secure_open(
        root_fd,
        relative,
        libc::O_RDONLY as u64 | libc::O_CLOEXEC as u64 | libc::O_NONBLOCK as u64,
    )?;
    let stat = stat_fd(&fd)?;
    if !is_regular(stat.0) {
        return Err(VerifyError::UnsafePath(format!("not a regular file: {}", relative.display())));
    }
    if stat.3 != expected_size {
        return Err(VerifyError::Io(format!("file changed size during verification: {}", relative.display())));
    }
    if stat.3 > MAX_FILE_BYTES {
        return Err(VerifyError::Io(format!("file exceeds {} byte cap: {}", MAX_FILE_BYTES, relative.display())));
    }
    let mut file = fs::File::from(fd);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| VerifyError::Io("skill file size overflow".to_string()))?;
        if total > MAX_FILE_BYTES {
            return Err(VerifyError::Io(format!("file grew beyond cap: {}", relative.display())));
        }
        hasher.update(&buffer[..count]);
    }
    if total != expected_size {
        return Err(VerifyError::Io(format!("file changed size during read: {}", relative.display())));
    }
    let digest = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok((text, stat))
}

fn walk_tree(
    current: &Path,
    relative: &Path,
    skill_relative: &Path,
    root_fd: RawFd,
    depth: usize,
    require_private: bool,
    files: &mut BTreeMap<String, String>,
    total_bytes: &mut u64,
) -> Result<(), VerifyError> {
    if depth > MAX_TREE_DEPTH {
        return Err(VerifyError::UnsafePath("skill tree nesting exceeds limit".to_string()));
    }
    let path_stat = fs::symlink_metadata(current)?;
    if path_stat.file_type().is_symlink() {
        return Err(VerifyError::UnsafePath(format!("symlink in skill tree: {}", current.display())));
    }
    if path_stat.is_dir() {
        let dir_relative = skill_relative.join(relative);
        let dir_fd = secure_open(
            root_fd,
            &dir_relative,
            libc::O_PATH as u64 | libc::O_DIRECTORY as u64 | libc::O_CLOEXEC as u64,
        )?;
        let dir_stat = stat_fd(&dir_fd)?;
        if !is_directory(dir_stat.0) {
            return Err(VerifyError::UnsafePath(format!("skill directory changed type: {}", current.display())));
        }
        check_private(dir_stat.1, "directory", require_private)?;
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let name = entry.file_name();
            let next_relative = if relative.as_os_str().is_empty() {
                PathBuf::from(&name)
            } else {
                relative.join(&name)
            };
            walk_tree(
                &entry.path(),
                &next_relative,
                skill_relative,
                root_fd,
                depth + 1,
                require_private,
                files,
                total_bytes,
            )?;
        }
        return Ok(());
    }
    if !path_stat.is_file() {
        return Err(VerifyError::UnsafePath(format!("special file in skill tree: {}", current.display())));
    }
    if relative == Path::new("skill.json") {
        return Ok(());
    }
    if files.len() >= MAX_FILES {
        return Err(VerifyError::Manifest("skill contains too many files".to_string()));
    }
    let relative_text = relative
        .to_str()
        .ok_or_else(|| VerifyError::UnsafePath(format!("non-UTF-8 skill path: {}", current.display())))?;
    validate_member(relative_text)?;
    *total_bytes = total_bytes
        .checked_add(path_stat.len())
        .ok_or_else(|| VerifyError::Io("skill total size overflow".to_string()))?;
    if *total_bytes > MAX_TOTAL_BYTES {
        return Err(VerifyError::Io(format!("skill tree exceeds {} byte cap", MAX_TOTAL_BYTES)));
    }
    let root_relative = skill_relative.join(relative);
    let (digest, opened_stat) = hash_file(root_fd, &root_relative, path_stat.len())?;
    if opened_stat.1 != path_stat.uid()
        || opened_stat.2 != path_stat.gid()
        || opened_stat.0 != path_stat.mode()
    {
        return Err(VerifyError::Io(format!("file metadata changed during verification: {relative_text}")));
    }
    check_private(opened_stat.1, opened_stat.0, "file", require_private)?;
    files.insert(relative_text.to_string(), digest);
    Ok(())
}

fn validate_absolute_entrypoint(path: &Path) -> Result<(), VerifyError> {
    let fd = secure_open_absolute(path, libc::O_PATH as u64 | libc::O_CLOEXEC as u64)?;
    let stat = stat_fd(&fd)?;
    if !is_regular(stat.0) {
        return Err(VerifyError::UnsafePath("absolute skill executable is not a regular file".to_string()));
    }
    if !SYSTEM_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return Err(VerifyError::UnsafePath("absolute skill executable is outside trusted system prefixes".to_string()));
    }
    if let Some(name) = path.file_name().and_then(|value| value.to_str())
        && FORBIDDEN_LAUNCHERS.contains(&name)
    {
        return Err(VerifyError::UnsafePath("generic command launcher is not an allowed skill entrypoint".to_string()));
    }
    check_private(stat.1, "executable", true)
}

fn validate_relative_entrypoint(
    root_fd: RawFd,
    skill_relative: &Path,
    entrypoint: &str,
    files: &BTreeMap<String, String>,
) -> Result<(), VerifyError> {
    validate_member(entrypoint)?;
    if !files.contains_key(entrypoint) {
        return Err(VerifyError::Manifest("relative skill entrypoint must be hashed in manifest".to_string()));
    }
    let relative = skill_relative.join(entrypoint);
    let fd = secure_open(root_fd, &relative, libc::O_PATH as u64 | libc::O_CLOEXEC as u64)?;
    let stat = stat_fd(&fd)?;
    if !is_regular(stat.0) {
        return Err(VerifyError::UnsafePath("relative skill entrypoint is not a regular file".to_string()));
    }
    Ok(())
}

/// Verify a skill using FD-anchored Linux path resolution.
pub fn verify_skill(system_root: &Path, directory: &Path) -> Result<VerifiedSkill, VerifyError> {
    if !directory.is_absolute() {
        return Err(VerifyError::UnsafePath("skill directory must be absolute".to_string()));
    }
    let canonical_root = fs::canonicalize(system_root)?;
    let root_relative = directory
        .strip_prefix(&canonical_root)
        .map_err(|_| VerifyError::UnsafePath("skill directory escapes configured system root".to_string()))?;
    if root_relative.as_os_str().is_empty() {
        return Err(VerifyError::UnsafePath("skill directory must not be the system root itself".to_string()));
    }
    let root = open_root(&canonical_root)?;
    let skill_dir = secure_open(
        root.as_raw_fd(),
        root_relative,
        libc::O_PATH as u64 | libc::O_DIRECTORY as u64 | libc::O_CLOEXEC as u64,
    )?;
    let skill_stat = stat_fd(&skill_dir)?;
    if !is_directory(skill_stat.0) {
        return Err(VerifyError::UnsafePath("skill directory is not a directory".to_string()));
    }

    let manifest = read_manifest(root.as_raw_fd(), root_relative)?;
    let mut actual = BTreeMap::new();
    let mut total_bytes = 0u64;
    walk_tree(
        &directory,
        Path::new(""),
        root_relative,
        root.as_raw_fd(),
        0,
        manifest.allow_root,
        &mut actual,
        &mut total_bytes,
    )?;

    let missing: Vec<String> = manifest
        .files
        .keys()
        .filter(|key| !actual.contains_key(*key))
        .cloned()
        .collect();
    let unexpected: Vec<String> = actual
        .keys()
        .filter(|key| !manifest.files.contains_key(*key))
        .cloned()
        .collect();
    if !missing.is_empty() || !unexpected.is_empty() {
        return Err(VerifyError::FileSetMismatch { missing, unexpected });
    }
    for (path, expected) in &manifest.files {
        if actual.get(path) != Some(expected) {
            return Err(VerifyError::HashMismatch(path.clone()));
        }
    }

    let entrypoint = manifest
        .entrypoint
        .first()
        .ok_or_else(|| VerifyError::Manifest("missing entrypoint".to_string()))?;
    if Path::new(entrypoint).is_absolute() {
        validate_absolute_entrypoint(Path::new(entrypoint))?;
    } else {
        validate_relative_entrypoint(root.as_raw_fd(), root_relative, entrypoint, &actual)?;
    }

    let display_directory = fs::canonicalize(directory)
        .map_err(|error| VerifyError::Io(format!("canonicalize verified skill directory: {error}")))?;
    Ok(VerifiedSkill {
        directory: display_directory.display().to_string(),
        manifest,
    })
}
