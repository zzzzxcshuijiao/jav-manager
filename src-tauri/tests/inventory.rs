use media_manager::inventory::{
    preview_inventory_roots, InventoryResourceKind, InventoryResourceRoleKind,
    InventoryReviewBucket, InventoryStatus,
};
use std::path::{Path, PathBuf};

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

// Build unique alphabetic file suffixes that avoid accidental normalized code matches.
fn alphabetic_suffix(mut index: usize) -> String {
    let mut chars = Vec::new();
    while index > 0 {
        index -= 1;
        chars.push((b'a' + (index % 26) as u8) as char);
        index /= 26;
    }
    chars.into_iter().rev().collect()
}

#[test]
fn inventory_preview_groups_scattered_resources_by_code_without_writing_files() {
    let tmp = tempfile::tempdir().unwrap();
    let videos = tmp.path().join("videos");
    let metadata = tmp.path().join("metadata");
    let images = tmp.path().join("images");
    let archive = tmp.path().join("archive");
    write_file(&videos.join("IPX-159.mp4"), b"video");
    write_file(
        &metadata.join("renamed-info.nfo"),
        br#"<movie><num>IPX-159</num><title>Inventory</title></movie>"#,
    );
    write_file(&images.join("IPX-159-poster.jpg"), b"poster");
    write_file(&images.join("IPX-159-fanart.webp"), b"fanart");
    write_file(&images.join("IPX-159-shot-01.png"), b"shot");
    write_file(&images.join("IPX-159.gif"), b"gif");

    let report = preview_inventory_roots(
        &[videos.clone(), metadata.clone(), images.clone()],
        Some(&archive),
    )
    .unwrap();

    assert_eq!(report.summary.total_files, 6);
    assert_eq!(report.summary.works, 1);
    assert_eq!(report.summary.ready, 1);
    assert!(report.orphans.is_empty());
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-159")
        .unwrap();
    assert!(work.statuses.contains(&InventoryStatus::Ready));
    assert_eq!(
        work.resources
            .iter()
            .filter(|r| r.kind == InventoryResourceKind::Video)
            .count(),
        1
    );
    assert_eq!(
        work.resources
            .iter()
            .filter(|r| r.kind == InventoryResourceKind::Nfo)
            .count(),
        1
    );
    assert_eq!(
        work.resources
            .iter()
            .filter(|r| r.kind == InventoryResourceKind::Poster)
            .count(),
        1
    );
    assert_eq!(
        work.resources
            .iter()
            .filter(|r| r.kind == InventoryResourceKind::Fanart)
            .count(),
        1
    );
    assert_eq!(
        work.resources
            .iter()
            .filter(|r| r.kind == InventoryResourceKind::Screenshot)
            .count(),
        1
    );
    assert_eq!(
        work.resources
            .iter()
            .filter(|r| r.kind == InventoryResourceKind::Gif)
            .count(),
        1
    );
    assert!(work
        .target_dir
        .as_ref()
        .unwrap()
        .ends_with(PathBuf::from("IPX-159")));
    assert!(
        !archive.exists(),
        "stage 7A preview must not create archive directories"
    );
}

#[test]
fn inventory_preview_preserves_full_summary_when_work_details_are_truncated() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("videos");
    for index in 1..=1001 {
        write_file(&root.join(format!("TST-{index:04}.mp4")), b"video");
    }

    let report = preview_inventory_roots(&[root], None).unwrap();

    assert_eq!(report.summary.total_files, 1001);
    assert_eq!(report.summary.works, 1001);
    assert_eq!(report.works.len(), 1000);
    assert!(report.truncated);
}

#[test]
fn inventory_preview_preserves_full_summary_when_orphan_details_are_truncated() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("loose");
    for index in 1..=1001 {
        write_file(
            &root.join(format!("note-{}.txt", alphabetic_suffix(index))),
            b"note",
        );
    }

    let report = preview_inventory_roots(&[root], None).unwrap();

    assert_eq!(report.summary.total_files, 1001);
    assert_eq!(report.summary.orphans, 1001);
    assert_eq!(report.orphans.len(), 1000);
    assert!(report.truncated);
}

#[test]
fn inventory_preview_reports_missing_roots_and_uncoded_orphans() {
    let tmp = tempfile::tempdir().unwrap();
    let missing_root = tmp.path().join("missing");
    let loose_root = tmp.path().join("loose");
    write_file(&loose_root.join("readme.txt"), b"notes");

    let report = preview_inventory_roots(&[missing_root.clone(), loose_root], None).unwrap();

    assert_eq!(report.summary.total_files, 1);
    assert_eq!(report.summary.works, 0);
    assert_eq!(report.summary.orphans, 1);
    assert_eq!(report.orphans.len(), 1);
    assert_eq!(report.orphans[0].file_name, "readme.txt");
    assert!(report.warnings.iter().any(|warning| {
        warning.contains("扫描根目录不存在")
            && warning.contains(&missing_root.to_string_lossy().to_string())
    }));
}

#[test]
fn inventory_preview_treats_whole_unpadded_code_image_stems_as_posters() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("images");
    write_file(&root.join("ABP-1.jpg"), b"poster");
    write_file(&root.join("ABP-01.jpg"), b"poster");

    let report = preview_inventory_roots(&[root], None).unwrap();

    let work = report
        .asset_candidates
        .iter()
        .find(|work| work.code == "ABP-001")
        .unwrap();
    assert!(work.statuses.contains(&InventoryStatus::AssetOnly));
    assert_eq!(
        work.resources
            .iter()
            .filter(|resource| resource.kind == InventoryResourceKind::Poster)
            .count(),
        2
    );
}

#[test]
fn inventory_preview_keeps_asset_only_groups_out_of_missing_video_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-301-cover.jpg"), b"poster");
    write_file(&root.join("IPX-301-01.jpg"), b"shot");
    write_file(&root.join("IPX-301.gif"), b"gif");
    write_file(
        &root.join("IPX-302.nfo"),
        br#"<movie><num>IPX-302</num></movie>"#,
    );

    let report = preview_inventory_roots(&[root], None).unwrap();

    assert_eq!(report.summary.total_files, 4);
    assert_eq!(report.summary.works, 1);
    assert_eq!(report.summary.asset_candidates, 1);
    assert_eq!(report.summary.missing_video, 1);
    assert!(report.works.iter().any(|work| work.code == "IPX-302"));
    assert!(report.works.iter().all(|work| work.code != "IPX-301"));
    let asset_candidate = report
        .asset_candidates
        .iter()
        .find(|work| work.code == "IPX-301")
        .unwrap();
    assert!(asset_candidate
        .statuses
        .contains(&InventoryStatus::AssetOnly));
    assert!(!asset_candidate
        .statuses
        .contains(&InventoryStatus::MissingVideo));
}

#[test]
fn inventory_preview_marks_missing_and_conflict_states() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("ABP-001.mp4"), b"video");
    write_file(
        &root.join("SSIS-777.nfo"),
        br#"<movie><num>SSIS-777</num></movie>"#,
    );
    write_file(&root.join("IPX-159.mp4"), b"v1");
    write_file(&root.join("IPX-159-CD2.mkv"), b"v2");
    write_file(
        &root.join("IPX-159.nfo"),
        br#"<movie><num>IPX-160</num></movie>"#,
    );

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();

    let abp = report
        .works
        .iter()
        .find(|work| work.code == "ABP-001")
        .unwrap();
    assert!(abp.statuses.contains(&InventoryStatus::MissingNfo));
    let ssis = report
        .works
        .iter()
        .find(|work| work.code == "SSIS-777")
        .unwrap();
    assert!(ssis.statuses.contains(&InventoryStatus::MissingVideo));
    let ipx159 = report
        .works
        .iter()
        .find(|work| work.code == "IPX-159")
        .unwrap();
    assert!(ipx159.statuses.contains(&InventoryStatus::MultiVideo));
    let ipx160 = report
        .works
        .iter()
        .find(|work| work.code == "IPX-160")
        .unwrap();
    assert!(ipx160.statuses.contains(&InventoryStatus::CodeConflict));
}

#[test]
fn inventory_preview_marks_same_size_multi_videos_as_duplicate_candidates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-206.mp4"), b"same-size-a");
    write_file(&root.join("IPX-206-copy.mkv"), b"same-size-b");

    let report = preview_inventory_roots(&[root.clone()], None).unwrap();

    assert_eq!(report.summary.duplicate_candidate, 1);
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-206")
        .unwrap();
    assert!(work.statuses.contains(&InventoryStatus::DuplicateCandidate));
}

#[test]
fn inventory_preview_deduplicates_repeated_or_overlapping_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let child = root.join("child");
    write_file(&child.join("IPX-207.mp4"), b"video");

    let report = preview_inventory_roots(&[root.clone(), child.clone()], None).unwrap();

    assert_eq!(report.summary.total_files, 1);
    assert_eq!(report.summary.multi_video, 0);
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-207")
        .unwrap();
    assert_eq!(work.resources.len(), 1);
    assert_eq!(work.actions.len(), 1);
    assert_eq!(work.actions[0].conflict, None);
}

#[test]
fn inventory_preview_marks_multiple_nfo_files_for_one_work() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(
        &root.join("IPX-203.nfo"),
        br#"<movie><num>IPX-203</num></movie>"#,
    );
    write_file(
        &root.join("renamed-metadata.nfo"),
        br#"<movie><num>IPX-203</num></movie>"#,
    );

    let report = preview_inventory_roots(&[root.clone()], None).unwrap();

    assert_eq!(report.summary.multi_nfo, 1);
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-203")
        .unwrap();
    assert!(work.statuses.contains(&InventoryStatus::MultiNfo));
}

#[test]
fn inventory_preview_marks_nfo_parse_errors_when_file_stem_provides_code() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-204.nfo"), &[0xff, 0xfe, 0xfd]);

    let report = preview_inventory_roots(&[root.clone()], None).unwrap();

    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-204")
        .unwrap();
    assert!(work.statuses.contains(&InventoryStatus::NfoParseError));
    let nfo = work
        .resources
        .iter()
        .find(|resource| resource.kind == InventoryResourceKind::Nfo)
        .unwrap();
    assert!(nfo
        .warnings
        .iter()
        .any(|warning| warning.contains("NFO 解析失败")));
}

#[test]
fn inventory_preview_builds_target_actions_and_marks_existing_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-200.mp4"), b"video");
    write_file(
        &root.join("IPX-200.nfo"),
        br#"<movie><num>IPX-200</num></movie>"#,
    );
    write_file(&root.join("IPX-200-cover.jpg"), b"poster");
    write_file(&archive.join("IPX-200").join("IPX-200.mp4"), b"existing");

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();

    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-200")
        .unwrap();
    let video_action = work
        .actions
        .iter()
        .find(|action| action.kind == InventoryResourceKind::Video)
        .unwrap();
    assert!(video_action
        .to_path
        .as_ref()
        .unwrap()
        .ends_with(PathBuf::from("IPX-200/IPX-200.mp4")));
    assert_eq!(video_action.conflict.as_deref(), Some("target_exists"));
    let nfo_action = work
        .actions
        .iter()
        .find(|action| action.kind == InventoryResourceKind::Nfo)
        .unwrap();
    assert!(nfo_action
        .to_path
        .as_ref()
        .unwrap()
        .ends_with(PathBuf::from("IPX-200/IPX-200.nfo")));
    let poster_action = work
        .actions
        .iter()
        .find(|action| action.kind == InventoryResourceKind::Poster)
        .unwrap();
    assert!(poster_action
        .to_path
        .as_ref()
        .unwrap()
        .ends_with(PathBuf::from("IPX-200/poster.jpg")));
}

#[test]
fn inventory_preview_uses_bare_video_as_primary_action_target() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-159-CD2.mkv"), b"part2");
    write_file(&root.join("IPX-159.mp4"), b"main");

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();

    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-159")
        .unwrap();
    let main_action = work
        .actions
        .iter()
        .find(|action| action.from_path.file_name().unwrap() == "IPX-159.mp4")
        .unwrap();
    assert!(main_action
        .to_path
        .as_ref()
        .unwrap()
        .ends_with(PathBuf::from("IPX-159/IPX-159.mp4")));
    let main_position = work
        .actions
        .iter()
        .position(|action| action.from_path.file_name().unwrap() == "IPX-159.mp4")
        .unwrap();
    let part_action = work
        .actions
        .iter()
        .find(|action| action.from_path.file_name().unwrap() == "IPX-159-CD2.mkv")
        .unwrap();
    assert!(part_action
        .to_path
        .as_ref()
        .unwrap()
        .ends_with(PathBuf::from("IPX-159/IPX-159-v2.mkv")));
    let part_position = work
        .actions
        .iter()
        .position(|action| action.from_path.file_name().unwrap() == "IPX-159-CD2.mkv")
        .unwrap();
    assert!(main_position < part_position);
}

#[test]
fn inventory_preview_marks_duplicate_generated_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-201-cover.jpg"), b"cover");
    write_file(&root.join("IPX-201-poster.jpg"), b"poster");

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();

    let work = report
        .asset_candidates
        .iter()
        .find(|work| work.code == "IPX-201")
        .unwrap();
    let duplicate_posters: Vec<_> = work
        .actions
        .iter()
        .filter(|action| action.kind == InventoryResourceKind::Poster)
        .collect();
    assert_eq!(duplicate_posters.len(), 2);
    assert!(duplicate_posters.iter().all(|action| action
        .to_path
        .as_ref()
        .unwrap()
        .ends_with(PathBuf::from("IPX-201/poster.jpg"))));
    assert!(duplicate_posters
        .iter()
        .all(|action| action.conflict.as_deref() == Some("target_duplicate")));
}

#[cfg(windows)]
#[test]
fn inventory_preview_marks_case_insensitive_duplicate_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-205-cover.JPG"), b"cover");
    write_file(&root.join("IPX-205-poster.jpg"), b"poster");

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();

    let work = report
        .asset_candidates
        .iter()
        .find(|work| work.code == "IPX-205")
        .unwrap();
    let duplicate_posters: Vec<_> = work
        .actions
        .iter()
        .filter(|action| action.kind == InventoryResourceKind::Poster)
        .collect();
    assert_eq!(duplicate_posters.len(), 2);
    assert!(duplicate_posters.iter().all(|action| {
        action
            .conflict
            .as_deref()
            .unwrap_or_default()
            .contains("target_duplicate")
    }));
}

#[test]
fn inventory_preview_combines_existing_and_duplicate_target_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-202-cover.jpg"), b"cover");
    write_file(&root.join("IPX-202-poster.jpg"), b"poster");
    write_file(&archive.join("IPX-202").join("poster.jpg"), b"existing");

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();

    let work = report
        .asset_candidates
        .iter()
        .find(|work| work.code == "IPX-202")
        .unwrap();
    let duplicate_posters: Vec<_> = work
        .actions
        .iter()
        .filter(|action| action.kind == InventoryResourceKind::Poster)
        .collect();
    assert_eq!(duplicate_posters.len(), 2);
    assert!(duplicate_posters.iter().all(|action| {
        let conflict = action.conflict.as_deref().unwrap_or_default();
        conflict.contains("target_exists") && conflict.contains("target_duplicate")
    }));
}

#[test]
fn inventory_preview_keeps_missing_roots_as_warnings() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing");

    let report = preview_inventory_roots(&[missing.clone()], None).unwrap();

    assert_eq!(report.summary.total_files, 0);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("扫描根目录不存在")));
}

#[test]
fn inventory_resolution_buckets_clean_work_as_auto_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-170.mp4"), b"video");
    write_file(
        &root.join("IPX-170.nfo"),
        br#"<movie><num>IPX-170</num><title>Ready</title></movie>"#,
    );

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();

    assert_eq!(report.summary.auto_ready, 1);
    assert_eq!(report.summary.needs_review, 0);
    assert_eq!(report.summary.blocked, 0);
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-170")
        .unwrap();
    assert_eq!(work.resolution.bucket, InventoryReviewBucket::AutoReady);
    assert_eq!(work.resolution.recommended, "可自动整理");
}

#[test]
fn execution_plan_selects_unique_safe_actions_from_duplicate_candidates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-290.mp4"), b"video");
    write_file(
        &root.join("IPX-290.nfo"),
        br#"<movie><num>IPX-290</num><title>Main</title></movie>"#,
    );
    write_file(
        &root.join("metadata-copy.nfo"),
        br#"<movie><num>IPX-290</num><title>Copy</title></movie>"#,
    );
    write_file(&root.join("IPX-290-cover.jpg"), b"cover-a");
    write_file(&root.join("IPX-290-poster.jpg"), b"cover-b");

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-290")
        .unwrap();

    assert!(work.actions.iter().any(|action| action
        .conflict
        .as_deref()
        .unwrap_or_default()
        .contains("target_duplicate")));
    assert_eq!(work.resolution.bucket, InventoryReviewBucket::AutoReady);
    assert!(work.resolution.execution_plan.ready);
    assert!(work.resolution.execution_plan.conflicts.is_empty());
    assert!(work
        .resolution
        .execution_plan
        .actions
        .iter()
        .all(|action| !action
            .conflict
            .as_deref()
            .unwrap_or_default()
            .contains("target_duplicate")));
    assert_eq!(
        work.resolution
            .execution_plan
            .actions
            .iter()
            .filter(|action| action.kind == InventoryResourceKind::Nfo)
            .count(),
        1
    );
    assert_eq!(
        work.resolution
            .execution_plan
            .actions
            .iter()
            .filter(|action| action.kind == InventoryResourceKind::Poster)
            .count(),
        1
    );
    assert!(work
        .resolution
        .execution_plan
        .notes
        .iter()
        .any(|note| note.contains("重复目标")));
}

#[test]
fn execution_plan_requires_review_for_multi_video_even_when_primary_is_selected() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-291.mp4"), b"main-video");
    write_file(&root.join("IPX-291-CD2.mkv"), b"second-part");
    write_file(
        &root.join("IPX-291.nfo"),
        br#"<movie><num>IPX-291</num><title>Multi video</title></movie>"#,
    );

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-291")
        .unwrap();

    assert_eq!(work.resolution.bucket, InventoryReviewBucket::NeedsReview);
    assert!(!work.resolution.execution_plan.ready);
    assert!(work
        .resolution
        .execution_plan
        .conflicts
        .iter()
        .any(|conflict| conflict.contains("多视频")));
}

#[test]
fn execution_plan_blocks_existing_selected_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-292.mp4"), b"video");
    write_file(
        &root.join("IPX-292.nfo"),
        br#"<movie><num>IPX-292</num><title>Existing target</title></movie>"#,
    );
    write_file(&archive.join("IPX-292").join("IPX-292.mp4"), b"existing");

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-292")
        .unwrap();

    assert_eq!(work.resolution.bucket, InventoryReviewBucket::Blocked);
    assert!(!work.resolution.execution_plan.ready);
    assert!(work
        .resolution
        .execution_plan
        .conflicts
        .iter()
        .any(|conflict| conflict.contains("目标路径已存在")));
    assert!(work
        .resolution
        .execution_plan
        .actions
        .iter()
        .any(|action| action
            .conflict
            .as_deref()
            .unwrap_or_default()
            .contains("target_exists")));
}

#[test]
fn execution_plan_blocks_existing_candidate_targets_outside_selected_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-293.mp4"), b"primary-video");
    write_file(&root.join("IPX-293-CD2.mkv"), b"second-part");
    write_file(
        &root.join("IPX-293.nfo"),
        br#"<movie><num>IPX-293</num><title>Existing secondary target</title></movie>"#,
    );
    write_file(
        &archive.join("IPX-293").join("IPX-293-v2.mkv"),
        b"existing-secondary",
    );

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-293")
        .unwrap();

    assert!(work.actions.iter().any(|action| action
        .from_path
        .ends_with("IPX-293-CD2.mkv")
        && action
            .conflict
            .as_deref()
            .unwrap_or_default()
            .contains("target_exists")));
    assert_eq!(work.resolution.bucket, InventoryReviewBucket::Blocked);
    assert!(!work.resolution.execution_plan.ready);
    assert!(work
        .resolution
        .execution_plan
        .conflicts
        .iter()
        .any(|conflict| conflict.contains("目标路径已存在")));
    assert!(work
        .resolution
        .blockers
        .iter()
        .any(|blocker| blocker.contains("目标路径已存在")));
}

#[test]
fn inventory_resolution_blocks_when_target_path_already_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-171.mp4"), b"video");
    write_file(
        &root.join("IPX-171.nfo"),
        br#"<movie><num>IPX-171</num><title>Existing target</title></movie>"#,
    );
    write_file(&archive.join("IPX-171").join("IPX-171.mp4"), b"existing");

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();

    assert_eq!(report.summary.blocked, 1);
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-171")
        .unwrap();
    assert_eq!(work.resolution.bucket, InventoryReviewBucket::Blocked);
    assert!(work
        .resolution
        .blockers
        .iter()
        .any(|blocker| blocker.contains("目标路径已存在")));
}

#[test]
fn inventory_resolution_blocks_code_conflict_from_nfo_num_precedence() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-172.mp4"), b"video");
    write_file(
        &root.join("IPX-172.nfo"),
        br#"<movie><num>IPX-173</num><title>Conflicting num</title></movie>"#,
    );

    let report = preview_inventory_roots(&[root], None).unwrap();

    assert_eq!(report.summary.blocked, 1);
    let work = report
        .works
        .iter()
        .find(|work| work.statuses.contains(&InventoryStatus::CodeConflict))
        .unwrap();
    assert_eq!(work.code, "IPX-173");
    assert_eq!(work.resolution.bucket, InventoryReviewBucket::Blocked);
    assert!(work
        .resolution
        .blockers
        .iter()
        .any(|blocker| blocker.contains("番号证据冲突")));
}

#[test]
fn inventory_resolution_keeps_asset_only_group_as_asset_candidate() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-174-cover.jpg"), b"cover");

    let report = preview_inventory_roots(&[root], None).unwrap();

    assert_eq!(report.summary.asset_candidates, 1);
    assert_eq!(report.summary.auto_ready, 0);
    assert_eq!(report.summary.needs_review, 0);
    assert_eq!(report.summary.blocked, 0);
    let asset_candidate = report
        .asset_candidates
        .iter()
        .find(|work| work.code == "IPX-174")
        .unwrap();
    assert_eq!(
        asset_candidate.resolution.bucket,
        InventoryReviewBucket::AssetCandidate
    );
}

#[test]
fn inventory_resolution_marks_same_size_non_primary_video_as_duplicate_role() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-175.mp4"), b"same-size");
    write_file(&root.join("IPX-175-copy.mkv"), b"same-size");

    let report = preview_inventory_roots(&[root], None).unwrap();

    assert_eq!(report.summary.needs_review, 1);
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-175")
        .unwrap();
    let duplicate_role = work
        .resource_roles
        .iter()
        .find(|role| role.role == InventoryResourceRoleKind::DuplicateVideo)
        .unwrap();
    assert!(duplicate_role.needs_review);
    assert!(duplicate_role.reason.contains("疑似重复视频"));
}

#[test]
fn inventory_resolution_selects_bare_video_and_matching_nfo_as_primary() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-159-CD2.mkv"), b"part2");
    write_file(&root.join("IPX-159.mp4"), b"main-video");
    write_file(
        &root.join("metadata.nfo"),
        br#"<movie><num>IPX-159</num><title>Main</title></movie>"#,
    );

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-159")
        .unwrap();

    assert_eq!(
        work.resolution.primary_video.as_deref(),
        Some(
            work.resources
                .iter()
                .find(|resource| resource.file_name == "IPX-159.mp4")
                .unwrap()
                .path
                .as_path()
        )
    );
    assert_eq!(
        work.resolution.primary_nfo.as_deref(),
        Some(
            work.resources
                .iter()
                .find(|resource| resource.file_name == "metadata.nfo")
                .unwrap()
                .path
                .as_path()
        )
    );
    assert_eq!(
        work.resolution.confidence,
        media_manager::inventory::InventoryConfidence::Medium
    );
    assert_eq!(work.resolution.bucket, InventoryReviewBucket::NeedsReview);
    assert!(work
        .resolution
        .reasons
        .iter()
        .any(|reason| reason.contains("裸番号视频")));
    assert!(work
        .resource_roles
        .iter()
        .any(|role| role.path.ends_with("IPX-159.mp4")
            && role.role == media_manager::inventory::InventoryResourceRoleKind::PrimaryVideo
            && role.selected));
    assert!(work
        .resource_roles
        .iter()
        .any(|role| role.path.ends_with("metadata.nfo")
            && role.role == media_manager::inventory::InventoryResourceRoleKind::PrimaryNfo
            && role.selected));
}

#[test]
fn inventory_resolution_uses_largest_video_when_no_bare_or_first_part_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-160-a-small.mp4"), b"small");
    write_file(&root.join("IPX-160-z-large.mkv"), b"larger-video");
    write_file(
        &root.join("IPX-160.nfo"),
        br#"<movie><num>IPX-160</num><title>Main</title></movie>"#,
    );

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report
        .works
        .iter()
        .find(|work| work.code == "IPX-160")
        .unwrap();

    assert!(work
        .resolution
        .primary_video
        .as_ref()
        .unwrap()
        .ends_with("IPX-160-z-large.mkv"));
    assert!(work
        .resolution
        .reasons
        .iter()
        .any(|reason| reason.contains("体积最大视频")));
    let primary_action = work
        .actions
        .iter()
        .find(|action| action.from_path.ends_with("IPX-160-z-large.mkv"))
        .unwrap();
    assert!(primary_action
        .to_path
        .as_ref()
        .unwrap()
        .ends_with(PathBuf::from("IPX-160/IPX-160.mkv")));
}
