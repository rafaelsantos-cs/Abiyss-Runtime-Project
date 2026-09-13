//! Skill manifest and artifact verification for the ABIYSS system plane.
//!
//! Verification lives close to the filesystem boundary. The high-level
//! SkillLE lifecycle remains in Python, but filesystem trust decisions are
//! made here when the Rust system plane is enabled.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
pub const MAX_FILES: usize = 256;
pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_TOKEN_BYTES: usize = 4096;
pub const MAX_VERSION_BYTES: usize = 128;
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_TREE_DEPTH: usize = 32;

const SYSTEM_PREFIXES: &[&str] = &["/bin", "/usr/bin", "/usr/local/bin"];
const FORBIDDEN_LAUNCHERS: &[&str] = &[
    "env", "bash", "sh", "dash", "zsh", "fish", "python", "python3",
    "python3.11", "python3.12", "python3.13", "python3.14", "node", "nodejs",
    "ruby", "perl", "php", "lua", "luajit", "java", "tclsh", "awk",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    pub name: String,
    pub version: String,
    pub entrypoint: Vec<String>,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub allow_root: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: f64,
    #[serde(default = "default_output_limit")]
    pub max_output_bytes: u64,
    #[serde(default = "default_max_args")]
    pub max_args: usize,
}

fn default_timeout() -> f64 {
    10.0
}

fn default_output_limit() -> u64 {
    64 * 1024
}

fn default_max_args() -> usize {
    32
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifiedSkill {
    pub directory: String,
    #[serde(flatten)]
    pub manifest: SkillManifest,
}

#[derive(Debug, Clone)]
pub enum VerifyError {
    UnsafePath(String),
    Manifest(String),
    Io(String),
    HashMismatch(String),
    FileSetMismatch {
        missing: Vec<String>,
        unexpected: Vec<String>,
    },
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsafePath(message) => write!(f, "unsafe skill path: {message}"),
            Self::Manifest(message) => write!(f, "invalid skill manifest: {message}"),
            Self::Io(message) => write!(f, "skill I/O failure: {message}"),
            Self::HashMismatch(path) => write!(f, "skill hash mismatch: {path}"),
            Self::FileSetMismatch { missing, unexpected } => write!(
                f,
                "skill file set mismatch; missing={:?}, unexpected={:?}",
                &missing[..missing.len().min(8)],
                &unexpected[..unexpected.len().min(8)]
            ),
        }
    }
}

impl From<io::Error> for VerifyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = digest.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn validate_text(value: &str, max_bytes: usize, label: &str) -> Result<(), VerifyError> {
    if value.is_empty() || value.len() > max_bytes || value.as_bytes().contains(&0) {
        return Err(VerifyError::Manifest(format!("invalid {label}")));
    }
    Ok(())
}

fn validate_relative_member(value: &str) -> Result<(), VerifyError> {
    validate_text(value, MAX_TOKEN_BYTES, "skill file path")?;
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(VerifyError::UnsafePath(format!("absolute member path: {value}")));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(VerifyError::UnsafePath(format!(
                "non-canonical member path: {value}"
            )));
        }
    }
    if value == "skill.json" {
        return Err(VerifyError::Manifest(
            "skill.json must not appear in files".to_string(),
        ));
    }
    Ok(())
}

fn validate_manifest(manifest: &SkillManifest) -> Result<(), VerifyError> {
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
        validate_relative_member(path)?;
        if digest.len() != 64
            || digest
                .bytes()
                .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
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
    Ok(())
}

fn read_manifest(path: &Path) -> Result<SkillManifest, VerifyError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VerifyError::UnsafePath(
            "skill.json must be a real regular file".to_string(),
        ));
    }
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(VerifyError::Manifest("manifest too large".to_string()));
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() {
        return Err(VerifyError::UnsafePath(
            "skill.json is not regular".to_string(),
        ));
    }
    let mut raw = Vec::with_capacity(opened.len() as usize);
    (&mut file)
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut raw)?;
    if raw.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(VerifyError::Manifest("manifest too large".to_string()));
    }
    let manifest: SkillManifest = serde_json::from_slice(&raw)
        .map_err(|error| VerifyError::Manifest(format!("invalid JSON: {error}")))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn hash_regular_file(path: &Path, metadata: &fs::Metadata) -> Result<String, VerifyError> {
    if !metadata.is_file() {
        return Err(VerifyError::UnsafePath(format!(
            "not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(VerifyError::Io(format!(
            "file exceeds {} byte cap: {}",
            MAX_FILE_BYTES,
            path.display()
        )));
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() {
        return Err(VerifyError::UnsafePath(format!(
            "file changed type: {}",
            path.display()
        )));
    }
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
            return Err(VerifyError::Io(format!(
                "file grew beyond cap: {}",
                path.display()
            )));
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex_digest(hasher.finalize()))
}

fn check_privileged_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), VerifyError> {
    if unsafe { libc::geteuid() } != 0 {
        return Ok(());
    }
    if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(VerifyError::UnsafePath(format!(
            "privileged skill path must be root-owned and not group/world writable: {}",
            path.display()
        )));
    }
    Ok(())
}

fn walk_tree(
    current: &Path,
    relative: &Path,
    depth: usize,
    require_private: bool,
    files: &mut BTreeMap<String, String>,
    total_bytes: &mut u64,
) -> Result<(), VerifyError> {
    if depth > MAX_TREE_DEPTH {
        return Err(VerifyError::UnsafePath(
            "skill tree nesting exceeds limit".to_string(),
        ));
    }
    let metadata = fs::symlink_metadata(current)?;
    if metadata.file_type().is_symlink() {
        return Err(VerifyError::UnsafePath(format!(
            "symlink in skill tree: {}",
            current.display()
        )));
    }
    if require_private {
        check_privileged_metadata(current, &metadata)?;
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let next = entry.path();
            let name = entry.file_name();
            let next_relative = if relative.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                relative.join(name)
            };
            walk_tree(
                &next,
                &next_relative,
                depth + 1,
                require_private,
                files,
                total_bytes,
            )?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(VerifyError::UnsafePath(format!(
            "special file in skill tree: {}",
            current.display()
        )));
    }
    if relative == Path::new("skill.json") {
        return Ok(());
    }
    if files.len() >= MAX_FILES {
        return Err(VerifyError::Manifest(
            "skill contains too many files".to_string(),
        ));
    }
    let relative_text = relative.to_str().ok_or_else(|| {
        VerifyError::UnsafePath(format!("non-UTF-8 skill path: {}", current.display()))
    })?;
    validate_relative_member(relative_text)?;
    *total_bytes = total_bytes
        .checked_add(metadata.len())
        .ok_or_else(|| VerifyError::Io("skill total size overflow".to_string()))?;
    if *total_bytes > MAX_TOTAL_BYTES {
        return Err(VerifyError::Io(format!(
            "skill tree exceeds {} byte cap",
            MAX_TOTAL_BYTES
        )));
    }
    let digest = hash_regular_file(current, &metadata)?;
    files.insert(relative_text.to_string(), digest);
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), VerifyError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(VerifyError::UnsafePath(format!(
                "symlink in skill directory path: {}",
                current.display()
            )));
        }
    }
    Ok(())
}

fn canonical_within(root: &Path, directory: &Path) -> Result<PathBuf, VerifyError> {
    if !directory.is_absolute() {
        return Err(VerifyError::UnsafePath(
            "skill directory must be absolute".to_string(),
        ));
    }
    reject_symlink_components(directory)?;
    let canonical_root = fs::canonicalize(root)?;
    let canonical_directory = fs::canonicalize(directory)?;
    if !canonical_directory.starts_with(&canonical_root) {
        return Err(VerifyError::UnsafePath(
            "skill directory escapes configured system root".to_string(),
        ));
    }
    Ok(canonical_directory)
}

/// Verify the skill tree and return the manifest that passed verification.
pub fn verify_skill(system_root: &Path, directory: &Path) -> Result<VerifiedSkill, VerifyError> {
    let directory = canonical_within(system_root, directory)?;
    let metadata = fs::symlink_metadata(&directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(VerifyError::UnsafePath(
            "skill directory is unavailable or a symlink".to_string(),
        ));
    }

    let manifest = read_manifest(&directory.join("skill.json"))?;
    let mut actual = BTreeMap::new();
    let mut total_bytes = 0u64;
    walk_tree(
        &directory,
        Path::new(""),
        0,
        manifest.allow_root,
        &mut actual,
        &mut total_bytes,
    )?;

    let expected = manifest.files.clone();
    let missing: Vec<String> = expected
        .keys()
        .filter(|key| !actual.contains_key(*key))
        .cloned()
        .collect();
    let unexpected: Vec<String> = actual
        .keys()
        .filter(|key| !expected.contains_key(*key))
        .cloned()
        .collect();
    if !missing.is_empty() || !unexpected.is_empty() {
        return Err(VerifyError::FileSetMismatch { missing, unexpected });
    }
    for (path, expected_digest) in &expected {
        if actual.get(path) != Some(expected_digest) {
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
        validate_relative_entrypoint(&directory, entrypoint, &actual)?;
    }

    Ok(VerifiedSkill {
        directory: directory.display().to_string(),
        manifest,
    })
}

fn validate_absolute_entrypoint(path: &Path) -> Result<(), VerifyError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VerifyError::UnsafePath(
            "absolute skill executable is unavailable or a symlink".to_string(),
        ));
    }
    if !SYSTEM_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return Err(VerifyError::UnsafePath(
            "absolute skill executable is outside trusted system prefixes".to_string(),
        ));
    }
    if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
        if FORBIDDEN_LAUNCHERS.contains(&name) {
            return Err(VerifyError::UnsafePath(
                "generic command launcher is not an allowed skill entrypoint".to_string(),
            ));
        }
    }
    check_privileged_metadata(path, &metadata)
}

fn validate_relative_entrypoint(
    directory: &Path,
    entrypoint: &str,
    files: &BTreeMap<String, String>,
) -> Result<(), VerifyError> {
    validate_relative_member(entrypoint)?;
    let candidate = directory.join(entrypoint);
    let metadata = fs::symlink_metadata(&candidate)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VerifyError::UnsafePath(
            "relative skill entrypoint is unavailable or a symlink".to_string(),
        ));
    }
    if !files.contains_key(entrypoint) {
        return Err(VerifyError::Manifest(
            "relative skill entrypoint must be hashed in manifest".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tempdir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "abiyss-skill-test-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_skill(root: &Path, content: &[u8]) -> (PathBuf, String) {
        let skill = root.join("demo");
        fs::create_dir_all(&skill).unwrap();
        let entry = skill.join("run.bin");
        fs::write(&entry, content).unwrap();
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o700)).unwrap();
        let digest = hex_digest(Sha256::digest(content));
        let manifest = format!(
            "{{\"name\":\"demo\",\"version\":\"1\",\"entrypoint\":[\"run.bin\"],\"files\":{{\"run.bin\":\"{}\"}},\"allow_root\":false,\"timeout_seconds\":10.0,\"max_output_bytes\":65536,\"max_args\":32}}",
            digest
        );
        fs::write(skill.join("skill.json"), manifest).unwrap();
        (skill, digest)
    }

    #[test]
    fn valid_skill_is_verified() {
        let root = tempdir();
        let (skill, digest) = write_skill(&root, b"hello");
        let verified = verify_skill(&root, &skill).unwrap();
        assert_eq!(verified.manifest.name, "demo");
        assert_eq!(verified.manifest.files["run.bin"], digest);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tampered_file_reports_hash_mismatch() {
        let root = tempdir();
        let (skill, _) = write_skill(&root, b"hello");
        fs::write(skill.join("run.bin"), b"tampered").unwrap();
        assert!(matches!(
            verify_skill(&root, &skill),
            Err(VerifyError::HashMismatch(path)) if path == "run.bin"
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn extra_file_is_rejected() {
        let root = tempdir();
        let (skill, _) = write_skill(&root, b"hello");
        fs::write(skill.join("extra.bin"), b"extra").unwrap();
        assert!(matches!(
            verify_skill(&root, &skill),
            Err(VerifyError::FileSetMismatch { .. })
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn symlink_is_rejected() {
        let root = tempdir();
        let (skill, _) = write_skill(&root, b"hello");
        let target = root.join("outside");
        fs::write(&target, b"outside").unwrap();
        std::os::unix::fs::symlink(&target, skill.join("link")).unwrap();
        assert!(matches!(
            verify_skill(&root, &skill),
            Err(VerifyError::UnsafePath(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn traversal_entrypoint_is_rejected() {
        let root = tempdir();
        let skill = root.join("demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("skill.json"),
            r#"{"name":"demo","version":"1","entrypoint":["../outside"],"files":{}}"#,
        )
        .unwrap();
        fs::write(root.join("outside"), b"bad").unwrap();
        assert!(matches!(
            verify_skill(&root, &skill),
            Err(VerifyError::Manifest(_)) | Err(VerifyError::UnsafePath(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_manifest_field_is_rejected() {
        let root = tempdir();
        let skill = root.join("demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("skill.json"),
            r#"{"name":"demo","version":"1","entrypoint":["run"],"files":{},"future":true}"#,
        )
        .unwrap();
        assert!(matches!(
            verify_skill(&root, &skill),
            Err(VerifyError::Manifest(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
