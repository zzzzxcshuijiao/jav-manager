use media_manager::inventory::{
    preview_inventory_roots, InventoryPreviewAction, InventoryResourceKind, InventoryReviewBucket,
};
use media_manager::inventory_execution::{
    execute_inventory_report, InventoryExecutionActionStatus, InventoryExecutionMode,
    InventoryExecutionReport, InventoryExecutionRequest,
};
use std::fs;
use std::path::{Path, PathBuf};

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

/// Build a low-space inventory execution request for integration tests.
fn low_space_all_request() -> InventoryExecutionRequest {
    InventoryExecutionRequest {
        mode: InventoryExecutionMode::LowSpace,
        selected_codes: Vec::new(),
    }
}

#[test]
fn inventory_execution_report_serializes_move_contract() {
    let report = InventoryExecutionReport {
        mode: InventoryExecutionMode::Move,
        started_at: "2026-06-28T00:00:00Z".to_string(),
        finished_at: "2026-06-28T00:00:01Z".to_string(),
        requested_works: 2,
        executed_works: 1,
        skipped_works: 0,
        planned_actions: 3,
        linked_actions: 0,
        copied_actions: 0,
        moved_actions: 2,
        failed_actions: 0,
        rolled_back_actions: 0,
        rollback_failed_actions: 1,
        same_volume_actions: 1,
        cross_volume_actions: 1,
        space_blocked_actions: 1,
        bytes_linked: 0,
        bytes_copied: 0,
        bytes_moved: 123,
        logs: vec![
            media_manager::inventory_execution::InventoryExecutionActionLog {
                code: "IPX-601".to_string(),
                kind: InventoryResourceKind::Video,
                from_path: PathBuf::from("from-video.mp4"),
                to_path: PathBuf::from("archive/IPX-601.mp4"),
                status: InventoryExecutionActionStatus::Moved,
                message: None,
                bytes: 100,
            },
            media_manager::inventory_execution::InventoryExecutionActionLog {
                code: "IPX-602".to_string(),
                kind: InventoryResourceKind::Poster,
                from_path: PathBuf::from("from-poster.jpg"),
                to_path: PathBuf::from("archive/poster.jpg"),
                status: InventoryExecutionActionStatus::RollbackFailed,
                message: Some("rollback failed".to_string()),
                bytes: 23,
            },
        ],
    };

    let json = serde_json::to_value(report).unwrap();

    assert_eq!(json["mode"], "move");
    assert_eq!(json["logs"][0]["status"], "moved");
    assert_eq!(json["logs"][1]["status"], "rollback_failed");
    assert_eq!(json["moved_actions"], 2);
    assert_eq!(json["rollback_failed_actions"], 1);
    assert_eq!(json["same_volume_actions"], 1);
    assert_eq!(json["cross_volume_actions"], 1);
    assert_eq!(json["space_blocked_actions"], 1);
    assert_eq!(json["bytes_moved"], 123);
}

#[test]
fn inventory_move_execution_is_not_enabled_before_move_task() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let video = root.join("IPX-701.mp4");
    let nfo = root.join("IPX-701.nfo");
    write_file(&video, b"video");
    write_file(
        &nfo,
        br#"<movie><num>IPX-701</num><title>Ready</title></movie>"#,
    );

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();
    let request = InventoryExecutionRequest {
        mode: InventoryExecutionMode::Move,
        selected_codes: Vec::new(),
    };

    let error = execute_inventory_report(&report, &request).unwrap_err();

    assert!(error.to_string().contains("集中迁移执行尚未启用"));
    assert!(video.exists(), "disabled move mode must keep the source video");
    assert!(nfo.exists(), "disabled move mode must keep the source NFO");
    assert!(!archive.join("IPX-701").join("IPX-701.mp4").exists());
    assert!(!archive.join("IPX-701").join("IPX-701.nfo").exists());
}

#[test]
fn inventory_low_space_execution_hardlinks_video_and_copies_small_assets() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let video = root.join("IPX-501.mp4");
    let nfo = root.join("IPX-501.nfo");
    let poster = root.join("IPX-501-cover.jpg");
    let nfo_contents = br#"<movie><num>IPX-501</num><title>Ready</title></movie>"#;
    write_file(&video, b"video");
    write_file(&nfo, nfo_contents);
    write_file(&poster, b"poster");

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();
    let execution = execute_inventory_report(&report, &low_space_all_request()).unwrap();
    let archive_video = archive.join("IPX-501").join("IPX-501.mp4");
    let archive_nfo = archive.join("IPX-501").join("IPX-501.nfo");
    let archive_poster = archive.join("IPX-501").join("poster.jpg");

    assert_eq!(execution.mode, InventoryExecutionMode::LowSpace);
    assert_eq!(execution.requested_works, 1);
    assert_eq!(execution.executed_works, 1);
    assert_eq!(execution.planned_actions, 3);
    assert_eq!(execution.linked_actions, 1);
    assert_eq!(execution.copied_actions, 2);
    assert_eq!(execution.moved_actions, 0);
    assert_eq!(execution.failed_actions, 0);
    assert_eq!(execution.rollback_failed_actions, 0);
    assert_eq!(execution.same_volume_actions, 0);
    assert_eq!(execution.cross_volume_actions, 0);
    assert_eq!(execution.space_blocked_actions, 0);
    assert_eq!(execution.bytes_linked, 5);
    assert_eq!(execution.bytes_copied, nfo_contents.len() as u64 + 6);
    assert_eq!(execution.bytes_moved, 0);
    assert!(execution
        .logs
        .iter()
        .any(|log| log.status == InventoryExecutionActionStatus::Linked));
    assert!(archive_video.exists());
    assert!(archive_nfo.exists());
    assert!(archive_poster.exists());

    write_file(&video, b"changed-video");
    assert_eq!(fs::read(&archive_video).unwrap(), b"changed-video");

    write_file(
        &nfo,
        br#"<movie><num>IPX-501</num><title>Changed</title></movie>"#,
    );
    assert_eq!(fs::read(&archive_nfo).unwrap(), nfo_contents);
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
    assert_eq!(execution.moved_actions, 0);
    assert_eq!(execution.failed_actions, 0);
    assert_eq!(execution.rolled_back_actions, 0);
    assert_eq!(execution.rollback_failed_actions, 0);
    assert_eq!(execution.same_volume_actions, 0);
    assert_eq!(execution.cross_volume_actions, 0);
    assert_eq!(execution.space_blocked_actions, 0);
    assert_eq!(execution.bytes_moved, 0);
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
