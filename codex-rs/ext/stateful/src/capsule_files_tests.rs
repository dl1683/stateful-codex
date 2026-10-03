use tempfile::TempDir;

use super::FileObserver;
use super::Observation;

#[tokio::test]
async fn only_files_inside_the_project_roots_are_read_and_reads_are_bounded() {
    let root = TempDir::new().expect("root");
    let outside = TempDir::new().expect("outside");
    let inside = root.path().join("notes.md");
    std::fs::write(&inside, "hello").expect("write");
    let foreign = outside.path().join("notes.md");
    std::fs::write(&foreign, "elsewhere").expect("write");
    let mut observer = FileObserver::new(vec![root.path().to_path_buf()]);
    assert!(matches!(
        observer.observe(&inside.display().to_string()).await,
        Observation::Read(bytes) if bytes == b"hello"
    ));
    assert!(matches!(
        observer.observe(&foreign.display().to_string()).await,
        Observation::NotRead(reason) if reason.contains("outside the project roots")
    ));
    assert!(matches!(
        observer.observe("relative/notes.md").await,
        Observation::NotRead(_)
    ));
    assert!(matches!(
        observer
            .observe(&root.path().join("gone.md").display().to_string())
            .await,
        Observation::NotRead("missing at the boundary")
    ));
    let large = root.path().join("large.bin");
    std::fs::write(&large, vec![0u8; 4 * 1024 * 1024 + 1]).expect("write");
    assert!(matches!(
        observer.observe(&large.display().to_string()).await,
        Observation::NotRead("too large to read at the boundary")
    ));
}
