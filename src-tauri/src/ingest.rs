use crate::domain::{IngestDecision, IngestItem, ReviewReason};
use crate::provider::MetadataProvider;

pub struct IngestEngine<P: MetadataProvider> {
    provider: P,
}

impl<P: MetadataProvider> IngestEngine<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    pub fn decide(&self, mut item: IngestItem) -> IngestItem {
        if item.decision == IngestDecision::DuplicateCandidate {
            push_reason(&mut item, ReviewReason::DuplicateFile);
            return item;
        }

        if item.review_reasons.contains(&ReviewReason::CodeConflict) {
            item.decision = IngestDecision::NeedsReview;
            return item;
        }

        if item.normalized_code.is_none() {
            push_reason(&mut item, ReviewReason::MissingCode);
            item.decision = IngestDecision::NeedsReview;
            return item;
        }

        if let Some(metadata) = item.metadata.as_ref() {
            if metadata.provider == "local" && metadata.confidence >= 0.9 {
                item.confidence = metadata.confidence;
                item.decision = IngestDecision::AutoArchive;
                return item;
            }
        }

        if item.confidence < 0.9 {
            push_reason(&mut item, ReviewReason::LowConfidence);
            item.decision = IngestDecision::NeedsReview;
            return item;
        }

        let code = item.normalized_code.clone().expect("checked above");
        match self.provider.lookup(&code, &item.file_name) {
            Ok(Some(metadata)) => {
                if metadata.confidence >= 0.8 {
                    item.metadata = Some(metadata);
                    item.decision = IngestDecision::AutoArchive;
                } else {
                    item.metadata = Some(metadata);
                    push_reason(&mut item, ReviewReason::LowConfidence);
                    item.decision = IngestDecision::NeedsReview;
                }
            }
            Ok(None) | Err(_) => {
                push_reason(&mut item, ReviewReason::ProviderFailed);
                item.decision = IngestDecision::NeedsReview;
            }
        }

        item
    }
}

fn push_reason(item: &mut IngestItem, reason: ReviewReason) {
    if !item.review_reasons.contains(&reason) {
        item.review_reasons.push(reason);
    }
}
