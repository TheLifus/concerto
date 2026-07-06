use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn reads_local_file_urls() {
    let path = temp_file("local-url");

    std::fs::write(&path, "fixture").unwrap();

    let url = format!("file://{}", path.display());

    assert_eq!(get_text(&url).unwrap(), "fixture");
    assert_eq!(download_bytes(&url).unwrap(), b"fixture");
}

fn temp_file(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    std::env::temp_dir().join(format!("concerto-{name}-{nanos}"))
}
