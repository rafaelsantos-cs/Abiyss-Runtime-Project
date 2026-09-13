#[cfg(target_os = "linux")]
mod linux_security {
    use abiyss_system_plane::read_confined_file;
    use std::ffi::CString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn fifo_read_is_nonblocking_and_rejected() {
        let base = unique_temp_dir("abiyss-fifo-test");
        fs::create_dir_all(&base).unwrap();
        let fifo = base.join("trap");
        let c_fifo = CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        let rc = unsafe { libc::mkfifo(c_fifo.as_ptr(), 0o600) };
        assert_eq!(rc, 0);

        let (tx, rx) = mpsc::channel();
        let root = base.clone();
        thread::spawn(move || {
            let result = read_confined_file(&root, "trap", 4096);
            let _ = tx.send(result);
        });

        let result = rx
            .recv_timeout(Duration::from_millis(500))
            .expect("FIFO access blocked the system-plane read path");
        assert!(result.is_err());
        let _ = fs::remove_dir_all(base);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{suffix}", std::process::id()))
    }
}
