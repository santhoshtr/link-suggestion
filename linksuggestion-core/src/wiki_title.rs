use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Represents a Wikipedia page title with utilities for normalization and manipulation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WikiTitle {
    /// The raw title as it appears
    raw: String,
    /// The normalized title following Wikipedia conventions
    normalized: String,
    language: String,
}

impl WikiTitle {
    /// Creates a new WikiTitle from a raw string
    pub fn new(title: &str, language: String) -> Self {
        let normalized = Self::normalize_title(title);
        Self {
            raw: title.to_string(),
            normalized,
            language,
        }
    }

    /// Returns the raw (original) title
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn language(&self) -> &String {
        &self.language
    }

    /// Returns the normalized title
    pub fn normalized(&self) -> &str {
        &self.normalized
    }

    /// Normalizes a Wikipedia title according to Wikipedia conventions
    fn normalize_title(title: &str) -> String {
        let mut normalized = title.trim().to_string();

        // Replace underscores with spaces
        normalized = normalized.replace('_', " ");

        // Normalize whitespace (collapse multiple spaces to single space)
        normalized = normalized
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join("_");

        // Capitalize first letter of each word for title case
        if !normalized.is_empty() {
            let mut chars: Vec<char> = normalized.chars().collect();
            chars[0] = chars[0].to_uppercase().next().unwrap_or(chars[0]);
            normalized = chars.into_iter().collect();
        }

        normalized
    }

    /// Converts the title to URL-safe format (spaces to underscores)
    pub fn to_url_format(&self) -> String {
        self.normalized.replace(' ', "_")
    }

    /// Returns the title in display format (normalized with spaces)
    pub fn to_display_format(&self) -> String {
        self.normalized.clone()
    }

    /// Checks if this title represents a disambiguation page
    pub fn is_disambiguation(&self) -> bool {
        self.normalized.contains("(disambiguation)")
            || self.normalized.ends_with(" (disambiguation)")
    }

    /// Extracts the main title without parenthetical disambiguation
    pub fn main_title(&self) -> String {
        if let Some(paren_pos) = self.normalized.find('(') {
            self.normalized[..paren_pos].trim().to_string()
        } else {
            self.normalized.clone()
        }
    }

    /// Extracts the disambiguation part (content in parentheses)
    pub fn disambiguation_part(&self) -> Option<String> {
        if let Some(start) = self.normalized.find('(') {
            if let Some(end) = self.normalized.find(')') {
                if end > start {
                    return Some(self.normalized[start + 1..end].to_string());
                }
            }
        }
        None
    }

    /// Checks if the title is valid (not empty after normalization)
    pub fn is_valid(&self) -> bool {
        !self.normalized.is_empty()
    }

    /// Returns the namespace if present (e.g., "User:" from "User:Example")
    pub fn namespace(&self) -> Option<String> {
        if let Some(colon_pos) = self.normalized.find(':') {
            let potential_namespace = &self.normalized[..colon_pos];
            // Common Wikipedia namespaces
            match potential_namespace {
                "User" | "Wikipedia" | "File" | "MediaWiki" | "Template" | "Help" | "Category"
                | "Portal" | "Book" | "Draft" | "Education Program" | "TimedText" | "Module"
                | "Gadget" | "Gadget definition" | "Topic" => Some(potential_namespace.to_string()),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Returns the title without namespace prefix
    pub fn without_namespace(&self) -> String {
        if let Some(_namespace) = self.namespace() {
            if let Some(colon_pos) = self.normalized.find(':') {
                return self.normalized[colon_pos + 1..].to_string();
            }
        }
        self.normalized.clone()
    }

    /// Checks if this is a main namespace article (no namespace prefix)
    pub fn is_main_namespace(&self) -> bool {
        self.namespace().is_none()
    }

    /// Compares two titles for equality (using normalized forms)
    pub fn equals(&self, other: &WikiTitle) -> bool {
        self.normalized == other.normalized
    }

    /// Compares title with a string (normalizes the string first)
    pub fn equals_str(&self, other: &str) -> bool {
        self.normalized == Self::normalize_title(other)
    }
}

pub async fn fetch_wikipedia_wikitext(
    language: &str,
    title: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = format!(
        "https://{language}.wikipedia.org/w/api.php?action=query&prop=revisions&rvprop=content&format=json&titles={title}&formatversion=2&redirects=1"
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header(
            "User-Agent",
            "WikiLink-Suggester/1.0 (https://gitlab.wikimedia.org/toolforge-repos/linker)",
        )
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Failed to fetch article: HTTP {}", response.status()).into());
    }

    let json_text = response.text().await?;
    let json: Value = serde_json::from_str(&json_text)?;

    let wikitext = json["query"]["pages"][0]["revisions"][0]["content"]
        .as_str()
        .ok_or("Could not find wikitext content in API response")?
        .to_string();

    Ok(wikitext)
}

impl fmt::Display for WikiTitle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_normalization() {
        let title = WikiTitle::new("  hello_world  ", String::from("en"));
        assert_eq!(title.normalized(), "Hello world");
    }

    #[test]
    fn test_url_format() {
        let title = WikiTitle::new("Hello world", String::from("en"));
        assert_eq!(title.to_url_format(), "Hello_world");
    }

    #[test]
    fn test_disambiguation() {
        let title = WikiTitle::new("Apple (fruit)", String::from("en"));
        assert!(title.is_disambiguation());
        assert_eq!(title.main_title(), "Apple");
        assert_eq!(title.disambiguation_part(), Some("fruit".to_string()));
    }

    #[test]
    fn test_namespace() {
        let title = WikiTitle::new("User:Example", String::from("en"));
        assert_eq!(title.namespace(), Some("User".to_string()));
        assert_eq!(title.without_namespace(), "Example");
        assert!(!title.is_main_namespace());
    }

    #[test]
    fn test_main_namespace() {
        let title = WikiTitle::new("Regular Article", String::from("en"));
        assert!(title.is_main_namespace());
        assert_eq!(title.namespace(), None);
    }

    #[test]
    fn test_equality() {
        let title1 = WikiTitle::new("Hello_World", String::from("en"));
        let title2 = WikiTitle::new("Hello World", String::from("en"));
        assert!(title1.equals(&title2));
        assert!(title1.equals_str("hello world"));
    }
}
