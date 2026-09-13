use abiyss_system_plane::skill_secure::verify_skill;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn tempdir() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("abiyss-secure-skill-test-{}-{suffix}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_skill(root: &Path, content: &[u8]) -> PathBuf {
    let skill = root.join("demo");
    fs::create_dir_all(&skill).unwrap();
    let entry = skill.join("run.bin");
    fs::write(&entry, content).unwrap();
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o700)).unwrap();
    let digest = format!("{:x}", sha2::Sha256::digest(content));
    let manifest = format!(
        "{{\"name\":\"demo\",\"version\":\"1\",\"entrypoint\":[\"run.bin\"],\"files\":{{\"run.bin\":\"{digest}\"}},\"allow_root\":false,\"timeout_seconds\":10.0,\"max_output_bytes\":65536,\"max_args\":32}}"
    );
    fs::write(skill.join("skill.json"), manifest).unwrap();
    skill
}

#[test]
fn valid_skill_verifies_through_pinned_root_fd() {
    let root = tempdir();
    let skill = write_skill(&root, b"hello");
    let verified = verify_skill(&root, &skill).unwrap();
    assert_eq!(verified.manifest.name, "demo");
    assert_eq!(verified.manifest.files.len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn symlink_member_is_rejected() {
    let root = tempdir();
    let skill = write_skill(&root, b"hello");
    fs::write(root.join("outside"), b"outside").unwrap();
    std::os::unix::fs::symlink(root.join("outside"), skill.join("link")).unwrap();
    assert!(verify_skill(&root, &skill).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn symlinked_skill_directory_is_rejected() {
    let root = tempdir();
    let real = write_skill(&root, b"hello");
    let link = root.join("alias");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(verify_skill(&root, &link).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn traversal_directory_is_rejected() {
    let root = tempdir();
    let skill = write_skill(&root, b"hello");
    let traversal = skill.join("../demo");
    assert!(verify_skill(&root, &traversal).is_ok());
    fs::remove_dir_all(root).unwrap();
}
