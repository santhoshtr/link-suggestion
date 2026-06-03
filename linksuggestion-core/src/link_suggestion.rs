use rusqlite::{Connection, fallible_iterator::FallibleIterator};
use serde::{Deserialize, Serialize};

use crate::{
    wiki_title::WikiTitle,
    wikitext::{TextSegment, WikiLink},
};
use std::cmp::Ordering;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock, Mutex},
};
use std::{fmt, sync::MutexGuard};

#[derive(Debug, Clone)]
pub struct LinkSuggestion {
    pub text_segment: TextSegment,
    pub title: WikiTitle,
    pub label: String,
    pub frequency: Option<i64>,
    pub frequency_max: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkSuggestionRecord {
    pub language: String,
    pub title: WikiTitle,
    pub link_text: String,
    pub frequency: i64,
    pub score: f32,
    // Add character indices
    pub wikitext_offset: usize,
}

#[derive(Debug, Clone)]
struct LinkRecord {
    pub article_title: String,
    pub link_title: String,
    pub link_label: String,
    pub frequency: usize,
}

// Helper function to remove punctuation
fn strip_punctuation(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_ascii_punctuation() && !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
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
    ) -> Result<i64, rusqlite::Error> {
        // Check cache first
        {
            let cache = FREQ_MAX_CACHE.lock().unwrap();
            if let Some(&cached_value) = cache.get(language) {
                return Ok(cached_value as i64);
            }
        }

        // Query database if not cached
        let query = "SELECT COUNT(link_title) as freq FROM Links GROUP BY link_title ORDER BY freq DESC LIMIT 1";
        let mut stmt = connection.prepare(query)?;
        let mut rows = stmt.query([])?;

        let freq_max = if let Some(first_row) = rows.next()? {
            first_row.get::<_, i64>(0)?
        } else {
            0
        };

        // Cache the result
        {
            let mut cache = FREQ_MAX_CACHE.lock().unwrap();
            cache.insert(language.to_string(), freq_max.try_into().unwrap());
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
            // Edge cases: A title for the label could not be found.
            return 0.0;
        }

        ((self.frequency.unwrap() as f32).ln() - (freq_min as f32).log10())
            / ((freq_max as f32).ln() - (freq_min as f32).log10())
    }

    pub fn process(&mut self, source_article: WikiTitle, conn: Arc<Mutex<Connection>>) {
        let res = self
            .find_title_for_label(source_article, conn.lock().unwrap())
            .unwrap();
        if let Some(link_record) = res {
            self.title = WikiTitle::new(&link_record.0, self.title.language().to_string());
            self.frequency = Some(link_record.1);
        }
        if self.frequency == Some(0) {
            return;
        }
        // Now we want to see if the tille really exist. This is where we eliminate red links as
        // well.
        if !self.is_valid_title(conn.lock().unwrap()) {
            self.frequency = Some(0);
            return;
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
            .prepare("SELECT 1 FROM links WHERE article_title = ?1 LIMIT 1")
            .unwrap();
        if stmt.exists([self.title.normalized()]).unwrap_or(false) {
            return true;
        }
        let mut stmt = connection
            .prepare("SELECT 1 FROM redirects WHERE article_title = ?1 LIMIT 1")
            .unwrap();
        if stmt.exists([self.title.normalized()]).unwrap_or(false) {
            return true;
        }

        false
    }

    fn find_title_for_label(
        &self,
        source_article: WikiTitle,
        connection: MutexGuard<'_, Connection>,
    ) -> Result<Option<(String, i64)>, rusqlite::Error> {
        let query = "SELECT  link_title, count(link_title) as freq FROM links WHERE link_label = ?1 GROUP by link_title ORDER BY freq DESC LIMIT 4".to_string();
        let mut stmt = connection.prepare(&query)?;
        let rows = stmt.query([&self.label.to_lowercase()])?;

        let link_record_items = rows.map(|r| Ok((r.get(0)?, r.get::<_, i64>(1)?))).unwrap();

        let records: Vec<(String, i64)> = link_record_items.collect::<Vec<(String, i64)>>();

        if records.is_empty() {
            return Ok(None);
        }

        let first_record = &records[0];
        if records.len() == 1 {
            // Only one record. No ambiguity
            return Ok(Some(first_record.clone()));
        }

        let mut winning_freq: i32 = first_record.1 as i32;
        for record in &records {
            // Let us see if the target article is linked to the current source article.
            // If so, we can assume source and target articles are related.
            // The suggestion will make them mutually linked.
            let reverse_relation_query =
                "SELECT 1 FROM links WHERE article_title = ?1 AND link_title = ?2 LIMIT 1";
            let mut reverse_stmt = connection.prepare(reverse_relation_query).unwrap();
            if reverse_stmt.exists([&record.0, source_article.normalized()])? {
                // Target article links back to the source_article. Accept the candidate.
                return Ok(Some(record.clone()));
            }
            // The winning_freq is the frequency of first candidate. It loses to other records.
            if winning_freq != record.1 as i32 {
                winning_freq -= record.1 as i32;
            }
        }
        // See if winning_freq is > 0
        // Then another check to make sure we are not linking random articles - link label and link
        // title should match in case insensitive way.
        if winning_freq > 0
            && (strip_punctuation(&self.label) == strip_punctuation(&first_record.0))
        {
            Ok(Some(first_record.clone()))
        } else {
            Ok(None)
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
        if self.title.normalized() == "WE_WILL_FIGURE_OUT_LATER" {
            return self.label == other.label;
        }
        self.title.normalized() == other.title.normalized()
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
        // First compare by start_byte position
        match self
            .text_segment
            .range
            .start_byte
            .cmp(&other.text_segment.range.start_byte)
        {
            Ordering::Equal => {
                // If start_byte is the same, compare by title as tiebreaker
                self.label.cmp(&other.label)
            }
            other_ordering => other_ordering,
        }
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
            if !seen_titles.insert(candidate.label.to_string()) {
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
