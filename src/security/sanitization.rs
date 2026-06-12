//! Input sanitization functions for security-critical operations

/// Sanitize HTML-like content by removing potentially dangerous elements
pub fn sanitize_html_basic(input: &str) -> String {
    // Remove script tags and their contents
    let script_regex = regex::Regex::new(r"(?i)<script[^>]*>.*?</script>").unwrap();
    let mut sanitized = script_regex.replace_all(input, "").to_string();

    // Remove potentially dangerous attributes
    let dangerous_attrs = [
        "onclick",
        "onload",
        "onerror",
        "onmouseover",
        "onmouseout",
        "onfocus",
        "onblur",
        "onchange",
        "onsubmit",
        "onreset",
        "javascript:",
        "vbscript:",
        "data:",
        "file:",
    ];

    for attr in &dangerous_attrs {
        let attr_regex = regex::Regex::new(&format!(r"(?i){}[^>]*", regex::escape(attr))).unwrap();
        sanitized = attr_regex.replace_all(&sanitized, "").to_string();
    }

    // Remove HTML comments that might contain code
    let comment_regex = regex::Regex::new(r"<!--.*?-->").unwrap();
    sanitized = comment_regex.replace_all(&sanitized, "").to_string();

    sanitized
}

/// Sanitize user input for database queries (basic protection)
pub fn sanitize_db_input(input: &str) -> String {
    // Remove SQL injection attempts
    let sql_keywords = [
        "DROP", "DELETE", "INSERT", "UPDATE", "ALTER", "CREATE", "TRUNCATE", "EXEC", "UNION",
        "SELECT", "--", "/*", "*/",
    ];

    let mut sanitized = input.to_string();
    for keyword in &sql_keywords {
        sanitized = sanitized.replace(keyword, "");
        sanitized = sanitized.replace(&keyword.to_lowercase(), "");
    }

    // Remove single quotes that aren't properly escaped
    sanitized = sanitized.replace("'", "''");

    sanitized
}

/// Sanitize file paths to prevent directory traversal
pub fn sanitize_file_path(path: &str) -> Result<String, String> {
    // Remove dangerous path components
    if path.contains("..") || path.contains("~") {
        return Err("Path contains potentially dangerous components".to_string());
    }

    // Remove or replace dangerous characters
    let sanitized = path
        .replace("\\", "/") // Normalize path separators
        .replace("//", "/") // Remove double slashes
        .trim_start_matches('/') // Remove leading slash
        .to_string();

    // Check for absolute paths on Windows
    if sanitized.len() >= 2 && sanitized.chars().nth(1) == Some(':') {
        return Err("Absolute paths not allowed".to_string());
    }

    Ok(sanitized)
}

/// Sanitize trust report content
pub fn sanitize_trust_report(content: &str) -> String {
    let mut sanitized = sanitize_html_basic(content);

    // Limit length
    if sanitized.len() > 1024 {
        sanitized.truncate(1024);
        sanitized.push_str("...[truncated]");
    }

    // Remove control characters except newlines and tabs
    sanitized = sanitized
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect();

    sanitized
}

/// Sanitize log messages to prevent log injection
pub fn sanitize_log_message(message: &str) -> String {
    // Replace newlines and carriage returns to prevent log injection
    message
        .replace(['\n', '\r', '\0'], " ") // Remove null bytes
        .chars()
        .filter(|&c| !c.is_control() || c == ' ') // Remove control chars except space
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_html_basic() {
        let input = "<script>alert('xss')</script><p>Safe content</p>";
        let result = sanitize_html_basic(input);
        assert!(!result.contains("script"));
        assert!(result.contains("Safe content"));
    }

    #[test]
    fn test_sanitize_db_input() {
        let input = "'; DROP TABLE users; --";
        let result = sanitize_db_input(input);
        assert!(!result.contains("DROP"));
        assert!(result.contains("''")); // Single quotes should be escaped
    }

    #[test]
    fn test_sanitize_file_path() {
        assert!(sanitize_file_path("../../../etc/passwd").is_err());
        assert!(sanitize_file_path("safe/path/file.txt").is_ok());
        assert!(sanitize_file_path("C:\\Windows\\System32").is_err());
    }

    #[test]
    fn test_sanitize_log_message() {
        let input = "Normal log\nFake log entry: ERROR";
        let result = sanitize_log_message(input);
        assert!(!result.contains('\n'));
        assert!(result.contains("Normal log Fake log entry"));
    }
}
