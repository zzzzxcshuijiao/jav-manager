use crate::domain::{ArchivePlan, IngestItem, IngestJobSummary};
use serde::Serialize;
use std::path::PathBuf;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhaseFDryRunSummary {
    pub job_id: i64,
    pub total_items: usize,
    pub auto_count: usize,
    pub review_count: usize,
    pub failed_count: usize,
    pub ignored_count: usize,
    pub duplicate_count: usize,
    pub items_with_code: usize,
    pub items_with_media_info: usize,
    pub archive_actions: usize,
    pub archive_conflicts: usize,
    pub decisions: Vec<(String, usize)>,
    pub review_reasons: Vec<(String, usize)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseFDryRunReport {
    pub database: PathBuf,
    pub provider: String,
    pub source_root_count: usize,
    pub summary: PhaseFDryRunSummary,
    pub items: Vec<IngestItem>,
    pub archive_plan: ArchivePlan,
    pub no_files_moved: bool,
}

pub fn summarize_phase_f_dry_run(
    job: &IngestJobSummary,
    items: &[IngestItem],
    plan: &ArchivePlan,
) -> PhaseFDryRunSummary {
    let mut decisions = BTreeMap::new();
    let mut review_reasons = BTreeMap::new();
    let mut ignored_count = 0;
    let mut duplicate_count = 0;
    let mut items_with_code = 0;
    let mut items_with_media_info = 0;

    for item in items {
        let decision = format!("{:?}", item.decision);
        *decisions.entry(decision.clone()).or_insert(0) += 1;
        if decision == "Ignored" {
            ignored_count += 1;
        }
        if decision == "DuplicateCandidate" {
            duplicate_count += 1;
        }
        if item.normalized_code.is_some() {
            items_with_code += 1;
        }
        if item.duration_seconds.is_some()
            || item.width.is_some()
            || item.height.is_some()
            || item.codec.is_some()
        {
            items_with_media_info += 1;
        }
        for reason in &item.review_reasons {
            *review_reasons
                .entry(format!("{:?}", reason))
                .or_insert(0) += 1;
        }
    }

    PhaseFDryRunSummary {
        job_id: job.id,
        total_items: job.total_items,
        auto_count: job.auto_count,
        review_count: job.review_count,
        failed_count: job.failed_count,
        ignored_count,
        duplicate_count,
        items_with_code,
        items_with_media_info,
        archive_actions: plan.actions.len(),
        archive_conflicts: plan.conflicts.len(),
        decisions: decisions.into_iter().collect(),
        review_reasons: review_reasons.into_iter().collect(),
    }
}

pub fn build_phase_f_dry_run_report(
    database: PathBuf,
    provider: &str,
    source_root_count: usize,
    job: &IngestJobSummary,
    items: &[IngestItem],
    plan: &ArchivePlan,
) -> PhaseFDryRunReport {
    PhaseFDryRunReport {
        database,
        provider: provider.to_string(),
        source_root_count,
        summary: summarize_phase_f_dry_run(job, items, plan),
        items: items.to_vec(),
        archive_plan: plan.clone(),
        no_files_moved: true,
    }
}

pub fn format_count_pairs(pairs: &[(String, usize)]) -> String {
    if pairs.is_empty() {
        return "none".to_string();
    }
    pairs
        .iter()
        .map(|(name, count)| format!("{name}:{count}"))
        .collect::<Vec<_>>()
        .join(",")
}
