
/// Checks whether `text` starts with `prefix` and there is word boundary
/// right after prefix, i.e. either `text` ends there or next character
/// is not alphanumberic.
pub fn starts_with_word(text: &str, prefix: &str) -> bool {
    text.starts_with(prefix) &&
        text[prefix.len()..].chars().next().map(|c| !c.is_ascii_alphanumeric()).unwrap_or(true)
}
