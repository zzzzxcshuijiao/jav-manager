use media_manager::acceptance::{build_phase_f_dry_run_report, format_count_pairs};
use media_manager::archive::ArchivePlanner;
use media_manager::ingest::IngestEngine;
use media_manager::provider::{DisabledProvider, ExampleProvider};
use media_manager::scanner::Scanner;
use media_manager::storage::Repository;
use std::env;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let use_example_provider = args.iter().any(|arg| arg == "--provider-example");
    let emit_json = args.iter().any(|arg| arg == "--json");
    let positional = args
        .into_iter()
        .filter(|arg| arg != "--provider-example" && arg != "--json")
        .collect::<Vec<_>>();
    if positional.len() < 3 {
        return Err(anyhow::anyhow!(
            "usage: phase_f_dry_run <library.sqlite> <archive_root> <source_root> [source_root...] [--provider-example] [--json]"
        ));
    }

    let db_path = PathBuf::from(&positional[0]);
    let archive_root = PathBuf::from(&positional[1]);
    let source_roots = positional[2..]
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();

    let repo = Repository::open(&db_path)?;
    repo.migrate()?;
    repo.set_archive_root(&archive_root)?;
    repo.set_source_roots(&source_roots)?;
    repo.set_metadata_provider_enabled(use_example_provider)?;

    let scanned_items = Scanner::scan_sources(&source_roots)?;
    let decided_items = if use_example_provider {
        let engine = IngestEngine::new(ExampleProvider);
        scanned_items
            .into_iter()
            .map(|item| engine.decide(item))
            .collect::<Vec<_>>()
    } else {
        let engine = IngestEngine::new(DisabledProvider);
        scanned_items
            .into_iter()
            .map(|item| engine.decide(item))
            .collect::<Vec<_>>()
    };

    let job_id = repo.create_ingest_job(&source_roots, &decided_items)?;
    let job = repo
        .get_ingest_job(job_id)?
        .ok_or_else(|| anyhow::anyhow!("created ingest job {job_id} was not found"))?;
    let persisted_items = repo.list_ingest_items(job_id)?;
    let archive_candidates = repo.list_archive_candidate_items_for_job(job_id)?;
    let plan = ArchivePlanner::new(archive_root).preview(&archive_candidates)?;
    let provider_name = if use_example_provider {
        "example"
    } else {
        "disabled"
    };
    let report = build_phase_f_dry_run_report(
        db_path.clone(),
        provider_name,
        source_roots.len(),
        &job,
        &persisted_items,
        &plan,
    );

    if emit_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("phase_f_dry_run=completed");
    println!("database={}", db_path.display());
    println!("provider={provider_name}");
    println!("source_roots={}", report.source_root_count);
    println!("job_id={}", report.summary.job_id);
    println!(
        "job_counts=total:{},auto:{},review:{},failed:{}",
        report.summary.total_items,
        report.summary.auto_count,
        report.summary.review_count,
        report.summary.failed_count
    );
    println!(
        "item_counts=with_code:{},with_media:{},duplicates:{},ignored:{}",
        report.summary.items_with_code,
        report.summary.items_with_media_info,
        report.summary.duplicate_count,
        report.summary.ignored_count
    );
    println!("decisions={}", format_count_pairs(&report.summary.decisions));
    println!(
        "review_reasons={}",
        format_count_pairs(&report.summary.review_reasons)
    );
    println!(
        "archive_preview=actions:{},conflicts:{}",
        report.summary.archive_actions, report.summary.archive_conflicts
    );
    println!("no_files_moved={}", report.no_files_moved);

    Ok(())
}
