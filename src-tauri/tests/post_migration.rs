use media_manager::inventory::InventoryResourceKind;
use media_manager::post_migration::{
    execute_post_migration_report, preview_post_migration_roots, PostMigrationActionKind,
    PostMigrationExecutionRequest, PostMigrationExecutionStatus, PostMigrationGroupKind,
};
use std::path::Path;

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn post_migration_preview_marks_verified_quarantine_for_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("video");
    let archive = tmp.path().join("archive");
    let bytes = b"same-video-bytes";
    write_file(
        &source
            .join("ABW-297")
            .join(".ABW-297.mp4.mm-source-delete-4716-0"),
        bytes,
    );
    write_file(&archive.join("ABW-297").join("ABW-297.mp4"), bytes);

    let report = preview_post_migration_roots(&[source], &archive).unwrap();

    assert_eq!(report.summary.quarantine_files, 1);
    assert_eq!(report.summary.cleanup_candidates, 1);
    let group = report
        .groups
        .iter()
        .find(|group| group.kind == PostMigrationGroupKind::Quarantine)
        .unwrap();
    assert_eq!(group.code, "ABW-297");
    let action = group.actions.first().unwrap();
    assert_eq!(action.kind, PostMigrationActionKind::DeleteQuarantine);
    assert_eq!(action.resource_kind, InventoryResourceKind::Video);
    assert!(action.conflict.is_none());
}

#[test]
fn post_migration_preview_cleans_quarantine_when_archive_target_uses_normalized_name() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("video");
    let archive = tmp.path().join("archive");
    let bytes = b"same-video-bytes";
    write_file(
        &source
            .join("ABW-297")
            .join(".ABW-297-C.mp4.mm-source-delete-4716-0"),
        bytes,
    );
    write_file(&archive.join("ABW-297").join("ABW-297.mp4"), bytes);

    let report = preview_post_migration_roots(&[source], &archive).unwrap();

    assert_eq!(report.summary.cleanup_candidates, 1);
    let action = report
        .groups
        .iter()
        .flat_map(|group| group.actions.iter())
        .find(|action| action.kind == PostMigrationActionKind::DeleteQuarantine)
        .unwrap();
    assert_eq!(
        action.to_path.as_deref(),
        Some(archive.join("ABW-297").join("ABW-297.mp4").as_path())
    );
    assert!(action.conflict.is_none());
}

#[test]
fn post_migration_preview_blocks_same_size_quarantine_when_hash_differs() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("video");
    let archive = tmp.path().join("archive");
    write_file(
        &source
            .join("ABW-302")
            .join(".ABW-302.mp4.mm-source-delete-4716-0"),
        b"same-size-left",
    );
    write_file(
        &archive.join("ABW-302").join("ABW-302.mp4"),
        b"same-size-rigt",
    );

    let report = preview_post_migration_roots(&[source], &archive).unwrap();

    assert_eq!(report.summary.cleanup_candidates, 0);
    assert_eq!(report.summary.restore_candidates, 0);
    assert_eq!(report.summary.blocked_actions, 1);
    let action = report
        .groups
        .iter()
        .flat_map(|group| group.actions.iter())
        .next()
        .unwrap();
    assert_eq!(action.conflict.as_deref(), Some("target_unverified"));
}

#[test]
fn post_migration_preview_marks_missing_target_quarantine_for_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("video");
    let archive = tmp.path().join("archive");
    let quarantine = source
        .join("ABW-298")
        .join(".ABW-298.mp4.mm-source-delete-4716-0");
    write_file(&quarantine, b"retained-source");

    let report = preview_post_migration_roots(&[source.clone()], &archive).unwrap();

    assert_eq!(report.summary.restore_candidates, 1);
    let action = report
        .groups
        .iter()
        .flat_map(|group| group.actions.iter())
        .find(|action| action.kind == PostMigrationActionKind::RestoreQuarantine)
        .unwrap();
    assert_eq!(action.from_path, quarantine);
    assert_eq!(
        action.to_path.as_deref(),
        Some(source.join("ABW-298").join("ABW-298.mp4").as_path())
    );
    assert!(action.conflict.is_none());
}

#[test]
fn post_migration_preview_builds_multi_video_supplemental_move_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("video");
    let archive = tmp.path().join("archive");
    write_file(&source.join("SSIS-270").join("SSIS-270-A.mp4"), b"video-a");
    write_file(&source.join("SSIS-270").join("SSIS-270-B.mkv"), b"video-b");
    write_file(
        &source.join("SSIS-270").join("SSIS-270.nfo"),
        br#"<movie><num>SSIS-270</num></movie>"#,
    );

    let report = preview_post_migration_roots(&[source], &archive).unwrap();

    assert_eq!(report.summary.multi_video_groups, 1);
    let group = report
        .groups
        .iter()
        .find(|group| group.kind == PostMigrationGroupKind::MultiVideo)
        .unwrap();
    assert_eq!(group.code, "SSIS-270");
    let video_targets: Vec<_> = group
        .actions
        .iter()
        .filter(|action| action.resource_kind == InventoryResourceKind::Video)
        .map(|action| {
            action
                .to_path
                .as_ref()
                .unwrap()
                .strip_prefix(&archive)
                .unwrap()
                .to_path_buf()
        })
        .collect();
    assert_eq!(
        video_targets,
        vec![
            Path::new("SSIS-270").join("SSIS-270.mp4"),
            Path::new("SSIS-270").join("SSIS-270-v2.mkv")
        ]
    );
    assert!(group.actions.iter().all(|action| action.conflict.is_none()));
}

#[test]
fn post_migration_preview_moves_asset_only_leftovers_into_existing_archive_work() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("jvedio");
    let archive = tmp.path().join("archive");
    write_file(&archive.join("ABP-108").join("ABP-108.mp4"), b"video");
    write_file(&source.join("Poster").join("ABP-108.jpg"), b"poster");
    write_file(&source.join("Screen").join("ABP-108-shot.jpg"), b"shot");
    write_file(
        &source.join("Nfo").join("ABP-108.nfo"),
        br#"<movie><num>ABP-108</num></movie>"#,
    );

    let report = preview_post_migration_roots(&[source], &archive).unwrap();

    assert_eq!(report.summary.asset_only_groups, 1);
    let group = report
        .groups
        .iter()
        .find(|group| group.kind == PostMigrationGroupKind::AssetOnly)
        .unwrap();
    assert_eq!(group.code, "ABP-108");
    let targets: Vec<_> = group
        .actions
        .iter()
        .map(|action| {
            action
                .to_path
                .as_ref()
                .unwrap()
                .strip_prefix(&archive)
                .unwrap()
                .to_path_buf()
        })
        .collect();
    assert!(targets.contains(&Path::new("ABP-108").join("poster.jpg")));
    assert!(targets.contains(
        &Path::new("ABP-108")
            .join("screenshots")
            .join("ABP-108-shot.jpg")
    ));
    assert!(targets.contains(&Path::new("ABP-108").join("ABP-108.nfo")));
}

#[test]
fn post_migration_execution_deletes_verified_quarantine_and_moves_assets() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("video");
    let archive = tmp.path().join("archive");
    let quarantine = source
        .join("ABW-299")
        .join(".ABW-299.mp4.mm-source-delete-4716-0");
    let asset = source.join("ABW-299").join("ABW-299-cover.jpg");
    write_file(&quarantine, b"same-video-bytes");
    write_file(&asset, b"poster");
    write_file(
        &archive.join("ABW-299").join("ABW-299.mp4"),
        b"same-video-bytes",
    );

    let report = preview_post_migration_roots(&[source], &archive).unwrap();
    let execution = execute_post_migration_report(
        &report,
        &PostMigrationExecutionRequest {
            selected_action_ids: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(execution.failed_actions, 0);
    assert!(execution
        .logs
        .iter()
        .any(|log| log.status == PostMigrationExecutionStatus::Deleted));
    assert!(!quarantine.exists());
    assert!(!asset.exists());
    assert!(archive.join("ABW-299").join("poster.jpg").exists());
}

#[test]
fn post_migration_execution_rejects_tampered_move_target_outside_archive_root() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("video");
    let archive = tmp.path().join("archive");
    let outside = tmp.path().join("outside");
    let asset = source.join("ABW-300").join("ABW-300-cover.jpg");
    write_file(&asset, b"poster");
    write_file(&archive.join("ABW-300").join("ABW-300.mp4"), b"video");

    let mut report = preview_post_migration_roots(&[source], &archive).unwrap();
    let action = report
        .groups
        .iter_mut()
        .flat_map(|group| group.actions.iter_mut())
        .find(|action| action.kind == PostMigrationActionKind::Move)
        .unwrap();
    action.to_path = Some(outside.join("stolen.jpg"));

    let execution = execute_post_migration_report(
        &report,
        &PostMigrationExecutionRequest {
            selected_action_ids: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(execution.failed_actions, 1);
    assert!(asset.exists());
    assert!(!outside.join("stolen.jpg").exists());
}

#[test]
fn post_migration_execution_rejects_tampered_quarantine_target_outside_archive_root() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("video");
    let archive = tmp.path().join("archive");
    let outside = tmp.path().join("outside");
    let quarantine = source
        .join("ABW-301")
        .join(".ABW-301.mp4.mm-source-delete-4716-0");
    write_file(&quarantine, b"same-video-bytes");
    write_file(
        &archive.join("ABW-301").join("ABW-301.mp4"),
        b"same-video-bytes",
    );
    write_file(&outside.join("fake-target.mp4"), b"same-video-bytes");

    let mut report = preview_post_migration_roots(&[source], &archive).unwrap();
    let action = report
        .groups
        .iter_mut()
        .flat_map(|group| group.actions.iter_mut())
        .find(|action| action.kind == PostMigrationActionKind::DeleteQuarantine)
        .unwrap();
    action.to_path = Some(outside.join("fake-target.mp4"));

    let execution = execute_post_migration_report(
        &report,
        &PostMigrationExecutionRequest {
            selected_action_ids: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(execution.failed_actions, 1);
    assert!(quarantine.exists());
}

#[test]
fn post_migration_execution_skips_blocked_actions_without_counting_them_as_failures() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("video");
    let archive = tmp.path().join("archive");
    write_file(
        &source
            .join("ABW-303")
            .join(".ABW-303.mp4.mm-source-delete-4716-0"),
        b"same-size-left",
    );
    write_file(
        &archive.join("ABW-303").join("ABW-303.mp4"),
        b"same-size-rigt",
    );

    let report = preview_post_migration_roots(&[source], &archive).unwrap();
    let blocked_id = report.groups[0].actions[0].id.clone();
    let execution = execute_post_migration_report(
        &report,
        &PostMigrationExecutionRequest {
            selected_action_ids: vec![blocked_id],
        },
    )
    .unwrap();

    assert_eq!(execution.skipped_actions, 1);
    assert_eq!(execution.failed_actions, 0);
    assert_eq!(execution.executed_actions, 0);
}
