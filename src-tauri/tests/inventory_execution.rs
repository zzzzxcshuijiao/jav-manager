use media_manager::inventory::{
    preview_inventory_roots, InventoryPreviewAction, InventoryResourceKind, InventoryReviewBucket,
};
use media_manager::inventory_execution::{
    execute_inventory_report, InventoryExecutionActionStatus, InventoryExecutionMode,
    InventoryExecutionRequest,
};
use std::fs;
use std::path::Path;

/// Write a small test file, creating parent directories as needed.
fn write_file(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

/// Build a copy-all inventory execution request for integration tests.
fn copy_all_request() -> InventoryExecutionRequest {
    InventoryExecutionRequest {
        mode: InventoryExecutionMode::Copy,
        selected_codes: Vec::new(),
    }
}

#[test]
fn inventory_copy_execution_copies_auto_ready_plan_actions_and_preserves_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let video = root.join("IPX-401.mp4");
    let nfo = root.join("IPX-401.nfo");
    let poster = root.join("IPX-401-cover.jpg");
    let duplicate_poster = root.join("IPX-401-poster.jpg");
    write_file(&video, b"video");
    write_file(
        &nfo,
        br#"<movie><num>IPX-401</num><title>Ready</title></movie>"#,
    );
    write_file(&poster, b"poster-a");
    write_file(&duplicate_poster, b"poster-b");

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-401")
        .unwrap();
    assert_eq!(work.resolution.bucket, InventoryReviewBucket::AutoReady);
    assert!(work.actions.iter().any(|action| action
        .conflict
        .as_deref()
        .unwrap_or_default()
        .contains("target_duplicate")));

    let execution = execute_inventory_report(&report, &copy_all_request()).unwrap();

    assert_eq!(execution.requested_works, 1);
    assert_eq!(execution.executed_works, 1);
    assert_eq!(execution.skipped_works, 0);
    assert_eq!(execution.planned_actions, 3);
    assert_eq!(execution.copied_actions, 3);
    assert_eq!(execution.failed_actions, 0);
    assert_eq!(execution.rolled_back_actions, 0);
    assert!(execution
        .logs
        .iter()
        .all(|log| log.status == InventoryExecutionActionStatus::Copied));
    assert!(video.exists(), "copy mode must keep the source video");
    assert!(nfo.exists(), "copy mode must keep the source NFO");
    assert!(
        poster.exists(),
        "copy mode must keep the selected poster source"
    );
    assert!(archive.join("IPX-401").join("IPX-401.mp4").exists());
    assert!(archive.join("IPX-401").join("IPX-401.nfo").exists());
    assert!(archive.join("IPX-401").join("poster.jpg").exists());
}

#[test]
fn inventory_copy_execution_all_run_skips_non_ready_works() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-402.mp4"), b"ready-video");
    write_file(
        &root.join("IPX-402.nfo"),
        br#"<movie><num>IPX-402</num><title>Ready</title></movie>"#,
    );
    write_file(&root.join("IPX-403.mp4"), b"missing-nfo");

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let execution = execute_inventory_report(&report, &copy_all_request()).unwrap();

    assert_eq!(execution.requested_works, 1);
    assert_eq!(execution.executed_works, 1);
    assert_eq!(execution.skipped_works, 1);
    assert!(archive.join("IPX-402").join("IPX-402.mp4").exists());
    assert!(!archive.join("IPX-403").exists());
}

#[test]
fn inventory_copy_execution_rejects_explicit_non_ready_selection_before_copying() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-404.mp4"), b"ready-video");
    write_file(
        &root.join("IPX-404.nfo"),
        br#"<movie><num>IPX-404</num><title>Ready</title></movie>"#,
    );
    write_file(&root.join("IPX-405.mp4"), b"missing-nfo");

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let request = InventoryExecutionRequest {
        mode: InventoryExecutionMode::Copy,
        selected_codes: vec!["IPX-405".to_string()],
    };

    let error = execute_inventory_report(&report, &request).unwrap_err();

    assert!(error.to_string().contains("不是可自动整理状态"));
    assert!(!archive.join("IPX-404").exists());
    assert!(!archive.join("IPX-405").exists());
}

#[test]
fn inventory_copy_execution_rejects_existing_target_before_copying_any_action() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-406.mp4"), b"first-video");
    write_file(
        &root.join("IPX-406.nfo"),
        br#"<movie><num>IPX-406</num><title>First</title></movie>"#,
    );
    write_file(&root.join("IPX-407.mp4"), b"second-video");
    write_file(
        &root.join("IPX-407.nfo"),
        br#"<movie><num>IPX-407</num><title>Second</title></movie>"#,
    );
    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    write_file(&archive.join("IPX-407").join("IPX-407.mp4"), b"existing");

    let error = execute_inventory_report(&report, &copy_all_request()).unwrap_err();

    assert!(error.to_string().contains("目标路径已存在"));
    assert!(
        !archive.join("IPX-406").join("IPX-406.mp4").exists(),
        "preflight must reject the batch before copying earlier actions"
    );
}

#[test]
fn inventory_copy_execution_rejects_targets_outside_archive_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let outside = tmp.path().join("outside");
    write_file(&root.join("IPX-408.mp4"), b"video");
    write_file(
        &root.join("IPX-408.nfo"),
        br#"<movie><num>IPX-408</num><title>Ready</title></movie>"#,
    );
    let mut report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter_mut()
        .find(|work| work.code == "IPX-408")
        .unwrap();
    work.resolution.execution_plan.actions[0].to_path = Some(outside.join("IPX-408.mp4"));

    let error = execute_inventory_report(&report, &copy_all_request()).unwrap_err();

    assert!(error.to_string().contains("整理目标目录之外"));
    assert!(!outside.join("IPX-408.mp4").exists());
}

#[test]
fn inventory_copy_execution_ignores_raw_candidate_actions() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let outside = tmp.path().join("outside");
    write_file(&root.join("IPX-409.mp4"), b"video");
    write_file(
        &root.join("IPX-409.nfo"),
        br#"<movie><num>IPX-409</num><title>Ready</title></movie>"#,
    );
    let mut report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter_mut()
        .find(|work| work.code == "IPX-409")
        .unwrap();
    work.actions.push(InventoryPreviewAction {
        from_path: work.resolution.primary_video.clone().unwrap(),
        to_path: Some(outside.join("raw-candidate.mp4")),
        kind: InventoryResourceKind::Video,
        conflict: None,
    });

    let execution = execute_inventory_report(&report, &copy_all_request()).unwrap();

    assert_eq!(execution.copied_actions, 2);
    assert!(archive.join("IPX-409").join("IPX-409.mp4").exists());
    assert!(!outside.join("raw-candidate.mp4").exists());
}

#[cfg(unix)]
#[test]
fn inventory_copy_execution_rejects_symlink_target_parent_outside_archive_root() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let outside = tmp.path().join("outside");
    write_file(&root.join("IPX-410.mp4"), b"video");
    write_file(
        &root.join("IPX-410.nfo"),
        br#"<movie><num>IPX-410</num><title>Ready</title></movie>"#,
    );
    fs::create_dir_all(&archive).unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, archive.join("link")).unwrap();
    let mut report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter_mut()
        .find(|work| work.code == "IPX-410")
        .unwrap();
    work.resolution.execution_plan.actions[0].to_path =
        Some(archive.join("link").join("IPX-410.mp4"));

    let error = execute_inventory_report(&report, &copy_all_request()).unwrap_err();

    assert!(error.to_string().contains("整理目标目录之外"));
    assert!(!outside.join("IPX-410.mp4").exists());
}

#[cfg(windows)]
#[test]
fn inventory_copy_execution_rejects_junction_target_parent_outside_archive_root() {
    use std::process::Command;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let outside = tmp.path().join("outside");
    write_file(&root.join("IPX-411.mp4"), b"video");
    write_file(
        &root.join("IPX-411.nfo"),
        br#"<movie><num>IPX-411</num><title>Ready</title></movie>"#,
    );
    fs::create_dir_all(&archive).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let link = archive.join("link");
    let status = Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            &link.to_string_lossy(),
            &outside.to_string_lossy(),
        ])
        .status()
        .unwrap();
    if !status.success() {
        return;
    }
    let mut report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter_mut()
        .find(|work| work.code == "IPX-411")
        .unwrap();
    work.resolution.execution_plan.actions[0].to_path =
        Some(archive.join("link").join("IPX-411.mp4"));

    let error = execute_inventory_report(&report, &copy_all_request()).unwrap_err();

    assert!(error.to_string().contains("整理目标目录之外"));
    assert!(!outside.join("IPX-411.mp4").exists());
}
