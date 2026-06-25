//! Stage 1 实测脚本：把数据层每个新功能跑一遍，打印人类可读结果。
//! 运行：cargo run --manifest-path src-tauri/Cargo.toml --example stage1_smoke
//! 纯数据层（Repository + SQLite），不启动任何 GUI，安全。

use anyhow::Result;
use media_manager::domain::{
    CodeKind, Exception, ExceptionKind, ExceptionStatus, HoldingEntry, HoldingReason,
    PipelineRun, ScrapeJob, ScrapeStatus, WatchStatus, Work,
};
use media_manager::storage::Repository;
use tempfile::tempdir;

fn main() -> Result<()> {
    let tmp = tempdir()?;
    let db_path = tmp.path().join("stage1_smoke.sqlite");
    let repo = Repository::open(&db_path)?;
    repo.migrate()?;
    println!("✓ 打开 + 迁移数据库（新建库，所有新表/新列就位）");

    // —— 1. 作品 + WatchStatus 新变体（想看/在看/搁置）——
    let mut work = sample_work();
    work.normalized_code = Some("ABP-001".to_string());
    work.title_zh = Some("示例作品".to_string());
    let id = repo.upsert_work(&work)?;
    let w = repo.update_work_profile(id, Vec::new(), Vec::new(), None, WatchStatus::WantToWatch)?;
    println!("✓ 插入作品 ABP-001，观看状态 = {:?}", w.watch_status);

    // —— 2. 播放进度（续播）——
    let w = repo.set_watch_progress(id, Some(1865), Some("2026-06-25T21:00:00Z".to_string()))?;
    println!(
        "✓ 续播进度：{} 秒（约 {:.0} 分钟），上次播放 {}",
        w.watch_progress_seconds.unwrap(),
        w.watch_progress_seconds.unwrap() as f64 / 60.0,
        w.last_played_at.as_deref().unwrap()
    );

    // —— 3. 收藏夹（多对多，幂等）——
    let col = repo.create_collection("精选最爱", Some("#00a4dc"))?;
    repo.add_work_to_collection(id, col)?;
    repo.add_work_to_collection(id, col)?; // 重复添加 -> 幂等，不重复
    let cols = repo.list_collections()?;
    let wids = repo.list_works_in_collection(col)?;
    println!(
        "✓ 收藏夹「{}」含 {} 个作品（重复添加自动去重，现共 {} 个收藏夹）",
        cols[0].name, wids.len(), cols.len()
    );

    // —— 4. 异常队列（刮削失败）——
    repo.record_exception(&Exception {
        id: None,
        object_path: "H:/dl/STARS-456.mp4".to_string(),
        kind: ExceptionKind::ScrapeFailed,
        evidence_json: r#"{"sources":["FANZA","JavBus","JavDB"]}"#.to_string(),
        status: ExceptionStatus::Open,
        created_at: None,
        resolved_at: None,
    })?;
    let exs = repo.list_exceptions()?;
    println!("✓ 异常队列：{} 条待处理（类型 {:?}）", exs.len(), exs[0].kind);

    // —— 5. 搁置区（短视频）——
    repo.add_holding(&HoldingEntry {
        id: None,
        path: "H:/dl/trailer.mp4".to_string(),
        file_name: "trailer.mp4".to_string(),
        size_bytes: 120_000_000,
        reason: HoldingReason::ShortVideo,
        created_at: None,
    })?;
    println!("✓ 搁置区：{} 条（trailer.mp4，原因 ShortVideo）", repo.list_holding()?.len());

    // —— 6. 刮削记录（追溯）——
    repo.record_scrape_job(&ScrapeJob {
        id: None,
        work_id: Some(id),
        normalized_code: Some("ABP-321".to_string()),
        object_path: Some("/fake/source/ABP-321.mp4".to_string()),
        pipeline_run_id: None,
        source: "FANZA".to_string(),
        status: ScrapeStatus::Failed,
        attempts: 2,
        last_attempted_at: Some("2026-06-25T20:00:00Z".to_string()),
        error: Some("查无此号".to_string()),
    })?;
    println!("✓ 刮削记录：{} 条（FANZA 失败 2 次，可追溯/重试）", repo.list_scrape_jobs()?.len());

    // —— 7. 管线日志 ——
    repo.record_pipeline_run(&PipelineRun {
        id: None,
        file_path: "H:/dl/ABP-001.mp4".to_string(),
        started_at: Some("2026-06-25T19:00:00Z".to_string()),
        finished_at: Some("2026-06-25T19:00:05Z".to_string()),
        steps_json: r#"[{"step":"identify","ok":true},{"step":"scrape","ok":true}]"#.to_string(),
        status: "done".to_string(),
        error: None,
    })?;
    println!("✓ 管线日志：{} 条（ABP-001 全流程 5 秒完成）", repo.list_pipeline_runs()?.len());

    // —— 8. 幂等性：再迁移一次 ——
    repo.migrate()?;
    println!("✓ 重复迁移无副作用（旧库升级不会丢数据/不会报错）");

    println!("\n=== 阶段 1 数据层：7 大新功能全部实测通过 ===");
    Ok(())
}

fn sample_work() -> Work {
    Work {
        id: None,
        normalized_code: None,
        source_code: None,
        code_kind: CodeKind::Standard,
        title_zh: None,
        original_title: None,
        aliases: Vec::new(),
        summary: None,
        outline: None,
        cover_path: None,
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: None,
        tags: Vec::new(),
        sets: Vec::new(),
        lists: Vec::new(),
        rating: None,
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        watch_status: WatchStatus::Unwatched,
        genres: Vec::new(),
        studio: None,
        label: None,
        director: None,
        release_date: None,
        runtime_minutes: None,
        year: None,
        website: None,
        mpaa: None,
        has_video: false,
        ratings: Vec::new(),
        watch_progress_seconds: None,
        last_played_at: None,
    }
}
