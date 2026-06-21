//! Rebuild orchestration: turn scanned NFO+video pairs into source_code
//! groups, pick a representative NFO per group, and summarize the outcome as
//! a `RebuildReport`. This module is pure data shuffling; every SQLite write
//! lives in `storage::Repository::rebuild_library`, so preview and the real
//! rebuild can share `summarize_grouped_inputs` and always agree on counts.

use crate::nfo::ParsedNfoDocument;
use crate::scanner::ScannedLibrary;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A recoverable per-NFO problem recorded in the report instead of aborting
/// the whole rebuild (e.g. an NFO with no parseable `<num>`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildError {
    pub nfo_path: String,
    pub message: String,
}

/// Aggregate outcome of a preview or real rebuild. Counts are derived from the
/// scanned groups so the two modes report identical numbers; `errors` lists
/// the NFOs that were scanned but could not become a work.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildReport {
    pub nfos_scanned: usize,
    pub works_created: usize,
    pub works_merged: usize,
    pub tags_extracted: usize,
    pub sets_extracted: usize,
    pub actors_extracted: usize,
    pub file_versions_created: usize,
    pub errors: Vec<RebuildError>,
}

/// One NFO that survived scanning, plus the video paired to it (if any) and
/// the source root it was found under. Members are grouped by `<num>`
/// (source_code) into a single logical work.
#[derive(Debug, Clone)]
pub struct GroupedWorkMember {
    pub nfo_path: PathBuf,
    pub nfo_stem: String,
    pub nfo_file_name: String,
    pub source_root: PathBuf,
    pub document: ParsedNfoDocument,
    pub paired_video: Option<PathBuf>,
    pub paired_video_size: u64,
}

/// A logical work assembled from one or more NFO members that share a
/// source_code. Multi-CD releases collapse into one group here.
#[derive(Debug, Clone)]
pub struct GroupedWorkInput {
    pub source_code: String,
    pub members: Vec<GroupedWorkMember>,
}

/// Output of grouping scanned NFOs: the groups plus per-NFO errors (NFOs with
/// no parseable `<num>` land here instead of in a group).
#[derive(Debug, Clone, Default)]
pub struct GroupedScan {
    pub groups: Vec<GroupedWorkInput>,
    pub errors: Vec<RebuildError>,
}

/// Group scanned NFO documents by their `<num>` (source_code). NFOs whose
/// document has no source_code become errors, since a work without an id can
/// neither merge nor be queried. Group order is first-sighting order; member
/// order within a group is scan order.
pub fn group_scanned_nfos(scanned: &ScannedLibrary) -> GroupedScan {
    let mut groups: Vec<GroupedWorkInput> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut errors: Vec<RebuildError> = Vec::new();

    for doc in &scanned.nfo_documents {
        let nfo_path = doc.nfo_path.clone();
        let Some(source_code) = doc
            .document
            .source_code
            .as_ref()
            .map(|code| code.trim())
            .filter(|code| !code.is_empty())
        else {
            errors.push(RebuildError {
                nfo_path: nfo_path.to_string_lossy().to_string(),
                message: "NFO has no <num> source code".to_string(),
            });
            continue;
        };

        let member = GroupedWorkMember {
            nfo_stem: doc
                .nfo_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("")
                .to_string(),
            nfo_file_name: doc
                .nfo_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string(),
            nfo_path,
            source_root: doc.source_root.clone(),
            document: doc.document.clone(),
            paired_video: doc.paired_video.clone(),
            paired_video_size: doc.paired_video_size,
        };

        if let Some(&idx) = index.get(source_code) {
            groups[idx].members.push(member);
        } else {
            index.insert(source_code.to_string(), groups.len());
            groups.push(GroupedWorkInput {
                source_code: source_code.to_string(),
                members: vec![member],
            });
        }
    }

    GroupedScan { groups, errors }
}

/// Pick the representative NFO for a group: shortest stem wins (so the base
/// `IPX-607.nfo` beats `IPX-607-cd2.nfo`), ties broken by smallest file name
/// for determinism. Caller guarantees a non-empty member slice.
pub fn select_main_nfo(members: &[GroupedWorkMember]) -> &GroupedWorkMember {
    members
        .iter()
        .min_by(|a, b| {
            a.nfo_stem
                .len()
                .cmp(&b.nfo_stem.len())
                .then_with(|| a.nfo_file_name.cmp(&b.nfo_file_name))
        })
        .expect("select_main_nfo called on an empty group")
}

/// Build a count-only report from grouped inputs (no writes). Preview and the
/// real rebuild share this so their reported counts always match.
pub fn summarize_grouped_inputs(
    groups: &[GroupedWorkInput],
    errors: &[RebuildError],
) -> RebuildReport {
    let member_total: usize = groups.iter().map(|group| group.members.len()).sum();
    let works_created = groups.len();
    let works_merged = groups
        .iter()
        .filter(|group| group.members.len() > 1)
        .count();

    // Distinct tags/sets/actors across all representative NFOs. tags union
    // genres mirrors the unified work_tags bag the persist step writes.
    let mut tags: Vec<String> = Vec::new();
    let mut sets: Vec<String> = Vec::new();
    let mut actors: Vec<String> = Vec::new();
    for group in groups {
        let main = select_main_nfo(&group.members);
        for tag in main.document.tags.iter().chain(main.document.genres.iter()) {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
        for set in &main.document.sets {
            if !sets.contains(set) {
                sets.push(set.clone());
            }
        }
        for actor in &main.document.actors {
            if !actors.contains(actor) {
                actors.push(actor.clone());
            }
        }
    }

    RebuildReport {
        nfos_scanned: member_total + errors.len(),
        works_created,
        works_merged,
        tags_extracted: tags.len(),
        sets_extracted: sets.len(),
        actors_extracted: actors.len(),
        file_versions_created: member_total,
        errors: errors.to_vec(),
    }
}
