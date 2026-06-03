use rusqlite::{Connection, OptionalExtension, fallible_iterator::FallibleIterator};
use serde::{Deserialize, Serialize};

use crate::{
    wiki_title::WikiTitle,
    wikitext::{TextSegment, WikiLink},
};
use std::cmp::Ordering;
use std::fmt;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock, Mutex},
};

#[derive(Debug, Clone)]
pub struct LinkSuggestion {
    pub text_segment: TextSegment,
    /// The resolved link target. `None` until disambiguation finds a title for
    /// the label (label candidates start unresolved).
    pub title: Option<WikiTitle>,
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
            title: Some(title),
            label,
            frequency: Some(0),
            frequency_max: 0,
        }
    }
    /// Creates an unresolved label candidate: it passed the label bloom but its
    /// title is not yet known. `process()` resolves it via `find_title_for_label`.
    pub fn new_with_label(text_segment: TextSegment, label: String) -> Self {
        LinkSuggestion {
            text_segment,
            title: None,
            label,
            frequency: Some(0),
            frequency_max: 0,
        }
    }

    fn get_freq_max(
        &self,
        connection: &Connection,
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
        let query = "SELECT COUNT(link_title) as freq FROM links GROUP BY link_title ORDER BY freq DESC LIMIT 1";
        let mut stmt = connection.prepare_cached(query)?;
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
        let freq_min: f32 = 1.0;
        let freq_max = self.frequency_max as f32;

        if self.title.is_none() {
            // A title for the label could not be resolved.
            return 0.0;
        }

        let fc = self.frequency.unwrap() as f32;
        if fc <= freq_min {
            return 0.0;
        }

        ((fc.ln() - freq_min.ln()) / (freq_max.ln() - freq_min.ln())).clamp(0.0, 1.0)
    }

    pub fn process(&mut self, source_article: WikiTitle, conn: &Connection) {
        // Candidates live in the same wiki as the source article, so take the
        // language from it (label candidates have no title to read it from yet).
        let language = source_article.language().to_string();
        if let Some((resolved_title, freq)) =
            self.find_title_for_label(source_article, conn).unwrap()
        {
            self.title = Some(WikiTitle::new(&resolved_title, language.clone()));
            self.frequency = Some(freq);
        }
        // Unresolved label candidate, or a title whose label had no links:
        // frequency is non-zero only when the block above set a title.
        if self.frequency == Some(0) {
            return;
        }
        let Some(title) = self.title.clone() else {
            return;
        };
        // Check the title really exists (eliminating red links) and resolve any
        // redirect to its canonical target, so the suggestion links directly
        // rather than to the redirect page.
        let Some(resolved) = Self::resolve_valid_title(&title, conn) else {
            self.frequency = Some(0);
            return;
        };
        self.title = Some(resolved.clone());
        // F_c for the confidence score must be the title's total link frequency
        // (title-grain) so it shares the same scale as F_max. The pair count from
        // find_title_for_label above is only used for candidate ranking.
        self.frequency = Some(Self::get_title_frequency(&resolved, conn).unwrap());
        self.frequency_max = self.get_freq_max(conn, &language).unwrap();
    }

    fn get_title_frequency(
        title: &WikiTitle,
        connection: &Connection,
    ) -> Result<i64, rusqlite::Error> {
        let mut stmt = connection
            .prepare_cached("SELECT COUNT(*) AS freq FROM links WHERE link_title = ?1")?;
        stmt.query_row([title.normalized()], |row| row.get(0))
    }

    /// Validates a candidate title and resolves it to a canonical link target.
    /// Returns the title itself if it is a real article, the redirect's target if
    /// it is a redirect (resolved one hop so the suggestion links directly rather
    /// than to the redirect page), or `None` for a red link.
    fn resolve_valid_title(title: &WikiTitle, connection: &Connection) -> Option<WikiTitle> {
        if !title.is_valid() {
            return None;
        }
        let mut stmt = connection
            .prepare_cached("SELECT 1 FROM links WHERE article_title = ?1 LIMIT 1")
            .unwrap();
        if stmt.exists([title.normalized()]).unwrap_or(false) {
            return Some(title.clone());
        }
        let mut stmt = connection
            .prepare_cached("SELECT target_title FROM redirects WHERE article_title = ?1 LIMIT 1")
            .unwrap();
        let target: Option<String> = stmt
            .query_row([title.normalized()], |row| row.get(0))
            .optional()
            .unwrap();
        target.map(|t| WikiTitle::new(&t, title.language().to_string()))
    }

    /// The raw (label, title) pair-frequency distribution for this suggestion's
    /// label: every title linked via that anchor with its occurrence count, most
    /// frequent first. This is what disambiguation picks from.
    pub fn candidate_distribution(
        &self,
        connection: &Connection,
    ) -> Result<Vec<(String, i64)>, rusqlite::Error> {
        let query = "SELECT  link_title, count(link_title) as freq FROM links WHERE link_label = ?1 GROUP by link_title ORDER BY freq DESC LIMIT 20".to_string();
        let mut stmt = connection.prepare_cached(&query)?;
        let rows = stmt.query([&self.label.to_lowercase()])?;

        let link_record_items = rows.map(|r| Ok((r.get(0)?, r.get::<_, i64>(1)?))).unwrap();

        Ok(link_record_items.collect::<Vec<(String, i64)>>())
    }

    fn find_title_for_label(
        &self,
        source_article: WikiTitle,
        connection: &Connection,
    ) -> Result<Option<(String, i64)>, rusqlite::Error> {
        let records = self.candidate_distribution(connection)?;

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
            let mut reverse_stmt = connection.prepare_cached(reverse_relation_query).unwrap();
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
        let title = self.title.as_ref()?;
        let text = &self.text_segment.text;
        let label = &self.label;

        // Find the label within the text segment
        if let Some(label_start) = text.to_lowercase().find(label.to_lowercase().as_str()) {
            let label_end = label_start + label.len();

            // Calculate absolute byte positions
            let absolute_start = self.text_segment.range.start_byte + label_start;
            let absolute_end = self.text_segment.range.start_byte + label_end;

            // Create the wiki link replacement text
            let replacement: String = if title.normalized() == label {
                format!("[[{label}]]")
            } else {
                format!("[[{}|{}]]", title.normalized(), label)
            };

            Some((absolute_start, absolute_end, replacement))
        } else {
            Some((0, 0, String::new()))
        }
    }

    /// Returns the absolute byte offset of the label's start within the full wikitext.
    /// Returns `None` if the label cannot be found in the text segment.
    pub fn link_start_byte(&self) -> Option<usize> {
        let label_start = self
            .text_segment
            .text
            .to_lowercase()
            .find(self.label.to_lowercase().as_str())?;
        Some(self.text_segment.range.start_byte + label_start)
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
        // Resolved suggestions are equal when they share a title; an unresolved
        // candidate (no title yet) is compared by its label instead.
        match (&self.title, &other.title) {
            (None, _) => self.label == other.label,
            (Some(_), None) => false,
            (Some(a), Some(b)) => a.normalized() == b.normalized(),
        }
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
            self.title
                .as_ref()
                .map(|t| t.normalized())
                .unwrap_or("<unresolved>"),
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
            // Deduplicate based on label
            if !seen_titles.insert(candidate.label.to_string()) {
                return false;
            }
            // Unresolved label candidates have no title yet; only the label
            // dedup above applies to them.
            let Some(title) = &candidate.title else {
                return true;
            };
            let normalized = title.normalized();
            // Remove candidates that are already present in existing WikiLinks
            if existing_links
                .iter()
                .any(|link| link.title.normalized() == normalized)
            {
                return false;
            }
            if normalized == current_article_title {
                return false;
            }
            if title.raw() == current_article_title {
                return false;
            }

            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Range;

    fn make_suggestion(freq: i64, freq_max: i64) -> LinkSuggestion {
        let range = Range {
            start_byte: 0,
            end_byte: 4,
            start_point: tree_sitter::Point { row: 0, column: 0 },
            end_point: tree_sitter::Point { row: 0, column: 4 },
        };
        let segment = TextSegment {
            text: "test".to_string(),
            range,
        };
        let title = WikiTitle::new("Test", "en".to_string());
        let mut s = LinkSuggestion::new(segment, title);
        s.frequency = Some(freq);
        s.frequency_max = freq_max;
        s
    }

    #[test]
    fn confidence_score_at_freq_min_is_zero() {
        let s = make_suggestion(1, 10000);
        assert_eq!(s.confidence_score(), 0.0);
    }

    #[test]
    fn confidence_score_at_freq_max_is_one() {
        let freq_max = 23789;
        let s = make_suggestion(freq_max, freq_max);
        assert!((s.confidence_score() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn confidence_score_interior_is_between_zero_and_one() {
        let s = make_suggestion(42, 23789);
        let score = s.confidence_score();
        assert!(score > 0.0 && score < 1.0, "score was {score}");
    }

    #[test]
    fn confidence_score_is_monotone() {
        let freq_max = 23789;
        let low = make_suggestion(10, freq_max).confidence_score();
        let mid = make_suggestion(100, freq_max).confidence_score();
        let high = make_suggestion(10000, freq_max).confidence_score();
        assert!(low < mid && mid < high, "scores: {low} {mid} {high}");
    }
}
