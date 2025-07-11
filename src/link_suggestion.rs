use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{
    stopwords::STOP_WORDS,
    wiki_title::WikiTitle,
    wikitext::{TextSegment, WikiLink},
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use std::{fmt, sync::MutexGuard};

#[derive(Debug, Clone)]
pub struct LinkSuggestion {
    pub text_segment: TextSegment,
    pub title: WikiTitle,
    pub label: String,
    pub frequency: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkSuggestionRecord {
    pub title: WikiTitle,
    pub label: String,
    pub frequency: usize,
    pub confidence_score: f32,
    pub byte_offset_start: usize,
    pub byte_offset_end: usize,
    pub link_text: String,
    // Add character indices
    pub char_offset_start: usize,
    pub char_offset_end: usize,
}

impl fmt::Display for LinkSuggestionRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "LinkSuggestion: \"{}\" ({})",
            self.link_text,
            self.title.normalized()
        )?;
        writeln!(
            f,
            "  Char offsets: {}..{}",
            self.char_offset_start, self.char_offset_end
        )?;
        writeln!(
            f,
            "  Byte offsets: {}..{}",
            self.byte_offset_start, self.byte_offset_end
        )?;
        writeln!(f, "  Frequency: {}", self.frequency)?;
        writeln!(f, "  Confidence score: {}", self.confidence_score)
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
        }
    }
    pub fn new_with_label(text_segment: TextSegment, title: WikiTitle, label: String) -> Self {
        LinkSuggestion {
            text_segment,
            title,
            label,
            frequency: Some(0),
        }
    }

    pub fn confidence_score(&self) -> f32 {
        let freq_threshold = 100;
        if self.title.normalized() == "WE_WILL_FIGURE_OUT_LATER" {
            return 0.0;
        }
        // Extract the frequency value or default to 0
        if self.frequency <= Some(freq_threshold) {
            return 0.1;
        }
        let freq = self.frequency.unwrap_or(0) as f32;
        0.4 + (freq / freq_threshold as f32) * 0.1
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
    pub fn calculate_link_positions_with_char_indices(
        &self,
        full_text: &String,
    ) -> Option<(usize, usize, usize, usize, String)> {
        let text = &self.text_segment.text;
        let label = &self.label;

        // Find the label within the text segment
        if let Some(label_start_bytes) = text.to_lowercase().find(label.to_lowercase().as_str()) {
            let label_end_bytes = label_start_bytes + label.len();

            // Calculate absolute byte positions
            let absolute_start_bytes = self.text_segment.range.start_byte + label_start_bytes;
            let absolute_end_bytes = self.text_segment.range.start_byte + label_end_bytes;
            // Calculate the character offsets
            let char_count_before_label = full_text[..absolute_start_bytes].chars().count();
            let label_char_count = label.chars().count();
            let char_start = char_count_before_label;
            let char_end = char_start + label_char_count;

            // Create the wiki link replacement text
            let replacement: String = if self.title.normalized() == label {
                format!("[[{label}]]")
            } else {
                format!("[[{}|{}]]", self.title.normalized(), label)
            };

            Some((
                absolute_start_bytes,
                absolute_end_bytes,
                char_start,
                char_end,
                replacement,
            ))
        } else {
            None
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
    current_article_tite: &String,
) -> Vec<LinkSuggestion> {
    let mut seen_titles = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| {
            let normalized = candidate.title.normalized();
            let label = candidate.label.as_str();
            // Deduplicate based on normalized title
            if !seen_titles.insert(normalized.to_string()) {
                return false;
            }

            // Remove titles that are numbers
            if normalized.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
            if label.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }

            // Remove if title is single letter
            if normalized.len() == 1 {
                return false;
            }
            if label.len() == 1 {
                return false;
            }

            // Remove candidates that are already present in existing WikiLinks
            if existing_links
                .iter()
                .any(|link| link.title.normalized() == normalized)
            {
                return false;
            }
            if candidate.title.normalized() == current_article_tite {
                return false;
            }
            if candidate.title.raw() == current_article_tite {
                return false;
            }

            // Remove titles that are stopwords
            let stopwords = STOP_WORDS;
            let lower_title = normalized.to_lowercase();
            if stopwords.contains(&lower_title.as_str()) {
                return false;
            }
            if stopwords.contains(&candidate.label.as_str()) {
                return false;
            }

            true
        })
        .collect()
}
