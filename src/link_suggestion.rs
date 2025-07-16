use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{
    wiki_title::WikiTitle,
    wikitext::{TextSegment, WikiLink},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock, Mutex},
};
use std::{fmt, sync::MutexGuard};
use std::cmp::Ordering;

#[derive(Debug, Clone)]
pub struct LinkSuggestion {
    pub text_segment: TextSegment,
    pub title: WikiTitle,
    pub label: String,
    pub frequency: Option<usize>,
    pub frequency_max: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkSuggestionRecord {
    pub language: String,
    pub title: WikiTitle,
    pub link_text: String,
    pub frequency: usize,
    pub score: f32,
    // Add character indices
    pub wikitext_offset: usize,
}

// Global cache for freq_max values by language
static FREQ_MAX_CACHE: LazyLock<Arc<Mutex<HashMap<String, usize>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

impl fmt::Display for LinkSuggestionRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "LinkSuggestion: \"{}\" ({})",
            self.link_text,
            self.title.normalized()
        )?;
        writeln!(f, "  Char offsets: {}", self.wikitext_offset)?;
        writeln!(f, "  Frequency: {}", self.frequency)?;
        writeln!(f, "  Confidence score: {}", self.score)
    }
}

impl LinkSuggestion {
    pub fn new(text_segment: TextSegment, title: WikiTitle) -> Self {
        let label = title.raw().to_string();
        LinkSuggestion {
            text_segment,
            title,
            label,
            frequency: Some(0),
            frequency_max: 0,
        }
    }
    pub fn new_with_label(text_segment: TextSegment, title: WikiTitle, label: String) -> Self {
        LinkSuggestion {
            text_segment,
            title,
            label,
            frequency: Some(0),
            frequency_max: 0,
        }
    }

    fn get_freq_max(
        &self,
        connection: MutexGuard<'_, Connection>,
        language: &str,
    ) -> Result<usize, rusqlite::Error> {
        // Check cache first
        {
            let cache = FREQ_MAX_CACHE.lock().unwrap();
            if let Some(&cached_value) = cache.get(language) {
                return Ok(cached_value);
            }
        }

        // Query database if not cached
        let query = "SELECT COUNT(link_label) AS freq FROM Links GROUP BY link_label ORDER BY freq DESC LIMIT 1";
        let mut stmt = connection.prepare(query)?;
        let mut rows = stmt.query([])?;

        let freq_max = if let Some(first_row) = rows.next()? {
            first_row.get(0)?
        } else {
            0
        };

        // Cache the result
        {
            let mut cache = FREQ_MAX_CACHE.lock().unwrap();
            cache.insert(language.to_string(), freq_max);
        }

        Ok(freq_max)
    }

    pub fn confidence_score(&self) -> f32 {
        let freq_min = 2;
        let freq_max = self.frequency_max;

        // Extract the frequency value or default to 0
        if self.frequency.unwrap() <= freq_min {
            return 0.0;
        }

        if self.title.normalized() == "WE_WILL_FIGURE_OUT_LATER" {
            // Edge cases: A title for the lable could not be found.
            return 0.0;
        }

        ((self.frequency.unwrap() as f32).ln() - (freq_min as f32).ln())
            / ((freq_max as f32).ln() - (freq_min as f32).ln())
    }

    pub fn process(&mut self, conn: Arc<Mutex<Connection>>) {
        if !self.is_valid_title(conn.lock().unwrap()) {
            self.frequency = Some(0);
            return;
        }
        if self.title.raw() == "WE_WILL_FIGURE_OUT_LATER" {
            let res = self.get_link_title_frequency(conn.lock().unwrap()).unwrap();
            if let Some((title, frequency)) = res {
                self.title = WikiTitle::new(&title, self.title.language().to_string());
                self.frequency = Some(frequency);
            }
        } else {
            self.frequency = self.get_link_frequency(conn.lock().unwrap()).unwrap();
        }
        self.frequency_max = self
            .get_freq_max(conn.lock().unwrap(), self.title.language())
            .unwrap();
    }

    fn is_valid_title(&self, connection: MutexGuard<'_, Connection>) -> bool {
        if !self.title.is_valid() {
            return false;
        }
        let mut stmt = connection
            .prepare("SELECT 1 FROM links WHERE link_title = ?1 LIMIT 1")
            .unwrap();
        stmt.exists([self.title.normalized()]).unwrap_or(false)
    }

    fn get_link_title_frequency(
        &self,
        connection: MutexGuard<'_, Connection>,
    ) -> Result<Option<(String, usize)>, rusqlite::Error> {
        let query = "SELECT link_title, count(link_title) as freq FROM links WHERE link_label = ?1 ORDER BY freq DESC LIMIT 2".to_string();
        let mut stmt = connection.prepare(&query)?;
        let mut rows = stmt.query([&self.label])?;
        let mut resp = Ok(None);
        // Check if there are multiple rows
        if let Some(first_row) = rows.next()? {
            // Otherwise, return the data from the first row
            let title: String = first_row.get(0)?;
            let frequency: usize = first_row.get(1)?;
            resp = Ok(Some((title, frequency)));
        }
        if rows.next()?.is_some() {
            // more rows?
            return Ok(None);
        }

        resp
    }

    fn get_link_frequency(
        &self,
        connection: MutexGuard<'_, Connection>,
    ) -> Result<Option<usize>, rusqlite::Error> {
        let query = "SELECT link_title, count(link_title) as freq FROM links WHERE link_title = ?1 ORDER BY freq DESC LIMIT 1".to_string();
        let mut stmt = connection.prepare(&query)?;
        let mut rows = stmt.query([&self.title.normalized()])?;

        if let Some(row) = rows.next()? {
            let frequency: usize = row.get(1)?;
            Ok(Some(frequency))
        } else {
            Ok(Some(0))
        }
    }

    /// Calculates the byte positions required to convert the label to a wiki internal link.
    ///
    /// Returns a tuple of (start_byte, end_byte, replacement_text) where:
    /// - start_byte: The starting byte position within the text segment where the label begins
    /// - end_byte: The ending byte position within the text segment where the label ends
    /// - replacement_text: The wiki link format string to replace the label with
    ///
    /// The byte positions are calculated relative to the text segment's start_byte.
    pub fn calculate_link_edit_positions(&self) -> Option<(usize, usize, String)> {
        let text = &self.text_segment.text;
        let label = &self.label;

        // Find the label within the text segment
        if let Some(label_start) = text.to_lowercase().find(label.to_lowercase().as_str()) {
            let label_end = label_start + label.len();

            // Calculate absolute byte positions
            let absolute_start = self.text_segment.range.start_byte + label_start;
            let absolute_end = self.text_segment.range.start_byte + label_end;

            // Create the wiki link replacement text
            let replacement: String = if self.title.normalized() == label {
                format!("[[{label}]]")
            } else {
                format!("[[{}|{}]]", self.title.normalized(), label)
            };

            Some((absolute_start, absolute_end, replacement))
        } else {
            Some((0, 0, String::new()))
        }
    }

    /// Returns character indices along with byte indices
    pub fn calculate_link_positions_with_char_indices(&self, full_text: &String) -> Option<usize> {
        let text = &self.text_segment.text;
        let label = &self.label;

        // Find the label within the text segment
        if let Some(label_start_bytes) = text.to_lowercase().find(label.to_lowercase().as_str()) {
            // Calculate absolute byte positions
            let absolute_start_bytes = self.text_segment.range.start_byte + label_start_bytes;
            // Calculate the character offsets
            let char_count_before_label = full_text[..absolute_start_bytes].chars().count();
            let char_start = char_count_before_label;

            Some(char_start)
        } else {
            Some(0)
        }
    }
}

impl PartialEq for LinkSuggestion {
    fn eq(&self, other: &Self) -> bool {
        // Two LinkSuggestions are equal if they have the same title or label
        self.title.normalized() == other.title.normalized() || self.label == other.label
    }
}

impl Eq for LinkSuggestion {}

impl PartialOrd for LinkSuggestion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LinkSuggestion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.text_segment.range.start_byte.cmp(&other.text_segment.range.start_byte)
    }
}

impl fmt::Display for LinkSuggestion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Segment[{}:{}-{}:{}]: \"{}\"",
            self.text_segment.range.start_point.row,
            self.text_segment.range.start_point.column,
            self.text_segment.range.end_point.row,
            self.text_segment.range.end_point.column,
            self.text_segment.text
        )?;
        writeln!(
            f,
            "Suggestion: [[{}|{}]]",
            self.title.normalized(),
            self.label
        )?;

        // Print the edit positions for tree-sitter
        if let Some((start, end, replacement)) = self.calculate_link_edit_positions() {
            writeln!(f, "Edit: bytes {start}..{end} -> '{replacement}'",)?;
        }
        writeln!(f, "frequency: {:?}", self.frequency)?;
        writeln!(f, "confidence_score: {}\n----", self.confidence_score())
    }
}

/// Filters a list of WikiTitle candidates to remove unwanted suggestions.
///
/// This function removes:
/// - Titles that are purely numeric
/// - Titles that are common stopwords
///
/// # Arguments
/// * `candidates` - A vector of WikiTitle candidates to filter
///
/// # Returns
/// A filtered vector of WikiTitle suggestions
pub fn filter_suggestions(
    candidates: Vec<LinkSuggestion>,
    existing_links: Vec<WikiLink>,
    current_article_title: &String,
) -> Vec<LinkSuggestion> {
    let mut seen_titles = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| {
            let normalized = candidate.title.normalized();
            // Deduplicate based on normalized title
            if !seen_titles.insert(normalized.to_string()) {
                return false;
            }
            // Remove candidates that are already present in existing WikiLinks
            if existing_links
                .iter()
                .any(|link| link.title.normalized() == normalized)
            {
                return false;
            }
            if candidate.title.normalized() == current_article_title {
                return false;
            }
            if candidate.title.raw() == current_article_title {
                return false;
            }

            true
        })
        .collect()
}
