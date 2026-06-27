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
