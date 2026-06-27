use media_manager::inventory::{preview_inventory_roots, InventoryResourceKind, InventoryStatus};
use std::path::{Path, PathBuf};

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
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
        .works
        .iter()
        .find(|work| work.code == "ABP-001")
        .unwrap();
    assert_eq!(
        work.resources
            .iter()
            .filter(|resource| resource.kind == InventoryResourceKind::Poster)
            .count(),
        2
    );
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
