use crate::domain::ProviderMetadata;

pub trait MetadataProvider: Send + Sync {
    fn name(&self) -> &str;
    fn lookup(&self, normalized_code: &str, original_file_name: &str) -> anyhow::Result<Option<ProviderMetadata>>;
}

pub struct ExampleProvider;
pub struct DisabledProvider;

impl MetadataProvider for ExampleProvider {
    fn name(&self) -> &str {
        "example"
    }

    fn lookup(&self, normalized_code: &str, _original_file_name: &str) -> anyhow::Result<Option<ProviderMetadata>> {
        Ok(Some(ProviderMetadata {
            provider: self.name().to_string(),
            title_zh: Some(format!("{normalized_code} 本地示例标题")),
            original_title: Some(format!("{normalized_code} Example Title")),
            aliases: vec![normalized_code.to_string()],
            summary: None,
            cover_url: None,
            release_date: None,
            confidence: 0.85,
            actors: vec![],
            genres: vec![],
            studio: None,
            director: None,
        }))
    }
}

impl MetadataProvider for DisabledProvider {
    fn name(&self) -> &str {
        "disabled"
    }

    fn lookup(&self, _normalized_code: &str, _original_file_name: &str) -> anyhow::Result<Option<ProviderMetadata>> {
        Ok(None)
    }
}
