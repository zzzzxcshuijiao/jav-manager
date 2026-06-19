use regex::Regex;

pub fn normalize_code(raw: &str) -> Option<String> {
    let re = Regex::new(r"(?i)\b([a-z]{2,10})[\s_-]*(\d{1,6})\b").ok()?;
    let captures = re.captures(raw.trim())?;
    let prefix = captures.get(1)?.as_str().to_ascii_uppercase();
    let number = captures.get(2)?.as_str();
    let parsed_number = number.parse::<u32>().ok()?;
    let width = number.len().max(3);
    Some(format!("{prefix}-{parsed_number:0width$}"))
}

pub fn extract_code_from_text(text: &str) -> Option<String> {
    normalize_code(text)
}
