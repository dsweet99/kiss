use std::fs;
use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

pub(crate) fn write_demo_crate_source(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("src").join("lib.rs"),
        "pub fn value() -> u32 { 1 }\n",
    )
    .unwrap();
}

pub(crate) fn llvm_cov_json_for_file(file: &Path) -> String {
    format!(
        r#"{{"data":[{{"files":[{{"filename":"{}","segments":[[1,1,1,true,true,false]]}}]}}]}}"#,
        file.display()
    )
}

pub(crate) fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn wait_child(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "child exited with {status}");
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
