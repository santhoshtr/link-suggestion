use database::with_connection;
use link_suggestion::{LinkSuggestion, LinkSuggestionRecord, filter_suggestions};
use linksuggestion_bloom::BloomFilterManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock};
use wiki_title::{WikiTitle, fetch_wikipedia_wikitext};
use wikitext::{TextSegment, WikiText};

mod database;
pub mod freq_distribution;
mod link_suggestion;
pub mod wiki_title;
pub mod wikitext;

/// Converts a list of byte offsets to character offsets in a single O(N) pass
/// over `text`, rather than calling `chars().count()` separately per offset.
fn byte_offsets_to_char_offsets(text: &str, byte_offsets: &[usize]) -> Vec<usize> {
    // Pair each offset with its original index, sort by byte position.
    let mut indexed: Vec<(usize, usize)> = byte_offsets.iter().copied().enumerate().collect();
    indexed.sort_unstable_by_key(|&(_, b)| b);

    let mut result = vec![0usize; byte_offsets.len()];
    let mut char_idx = 0usize;
    let mut byte_idx = 0usize;
    let mut cursor = indexed.into_iter().peekable();

    for ch in text.chars() {
        // Emit char offsets for all targets sitting at the current byte position.
        while let Some(&(orig, target_byte)) = cursor.peek() {
            if byte_idx == target_byte {
                result[orig] = char_idx;
                cursor.next();
            } else {
                break;
            }
        }
        byte_idx += ch.len_utf8();
        char_idx += 1;
    }
    // Handle any targets at the very end of the string.
    for (orig, _) in cursor {
        result[orig] = char_idx;
    }
    result
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LinkSuggestionsResult {
    pub language: String,
    pub title: String,
    pub confidence_score: f32,
    pub wikitext: String,
    pub suggestions: Vec<LinkSuggestionRecord>,
}

type BloomPair = Arc<(BloomFilterManager, BloomFilterManager)>;

static BLOOM_CACHE: LazyLock<RwLock<HashMap<String, BloomPair>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn load_bloom_filters(language: &str) -> BloomPair {
    {
        let cache = BLOOM_CACHE.read().unwrap();
        if let Some(pair) = cache.get(language) {
            return Arc::clone(pair);
        }
    }

    let data_dir = std::env::var("TOOL_DATA_DIR").unwrap_or_else(|_| "data".to_string());

    let title_file = PathBuf::from(format!("{data_dir}/bloom/{language}wiki.bloom"));
    let title_filter = BloomFilterManager::load_from_file(&title_file)
        .unwrap_or_else(|_| panic!("Error reading file {}", title_file.display()));

    let label_file = PathBuf::from(format!("{data_dir}/bloom/{language}wiki.labels.bloom"));
    let label_filter = BloomFilterManager::load_from_file(&label_file)
        .unwrap_or_else(|_| panic!("Error reading file {}", label_file.display()));

    let pair = Arc::new((title_filter, label_filter));
    BLOOM_CACHE
        .write()
        .unwrap()
        .insert(language.to_string(), Arc::clone(&pair));
    pair
}

fn process_title_candidates(
    segment: &TextSegment,
    candidates: Vec<String>,
    title_filter: &BloomFilterManager,
    language: &str,
) -> Vec<LinkSuggestion> {
    let mut suggestions = Vec::new();

    let filtered_candidates: Vec<String> = candidates
        .into_iter()
        .filter(|candidate| {
            let wiki_title = WikiTitle::new(candidate, language.to_owned());
            let normalized_title = wiki_title.normalized();
            // Everything in titles bloom filter is normalized titles - with underscores instead of
            // spaces.
            title_filter.exist(normalized_title)
        })
        .collect();

    for candidate in filtered_candidates {
        let wiki_title = WikiTitle::new(&candidate, language.to_owned());

        suggestions.push(LinkSuggestion::new(segment.clone(), wiki_title));
    }

    suggestions
}

fn process_label_candidates(
    segment: &TextSegment,
    candidates: Vec<String>,
    label_filter: &BloomFilterManager,
) -> Vec<LinkSuggestion> {
    let mut suggestions = Vec::new();

    let filtered_candidates: Vec<String> = candidates
        .into_iter()
        .filter(|candidate| label_filter.exist(candidate))
        .collect();

    if filtered_candidates.is_empty() {
        return suggestions;
    }
    for candidate in filtered_candidates {
        suggestions.push(LinkSuggestion::new_with_label(segment.clone(), candidate));
    }

    suggestions
}

fn process_text_segments(
    text_segments: Vec<TextSegment>,
    title_filter: &BloomFilterManager,
    label_filter: &BloomFilterManager,
    language: &str,
) -> Vec<LinkSuggestion> {
    let mut link_suggestions = Vec::new();

    for segment in text_segments {
        let mut link_candidates = segment.link_candidates();
        link_candidates.sort();
        link_candidates.dedup();

        // Process title candidates
        let title_suggestions =
            process_title_candidates(&segment, link_candidates.clone(), title_filter, language);
        link_suggestions.extend(title_suggestions);

        // Process label candidates
        let label_suggestions = process_label_candidates(&segment, link_candidates, label_filter);
        link_suggestions.extend(label_suggestions);
    }
    link_suggestions.sort();
    link_suggestions.dedup();
    link_suggestions
}

pub async fn process_links_command(
    language: &str,
    title: &str,
    confidence_threshold: f32,
) -> Result<LinkSuggestionsResult, Box<dyn Error>> {
    let mut parser = WikiText::new().unwrap();

    let source_article = WikiTitle::new(title, language.to_string());
    let mut wikitext = fetch_wikipedia_wikitext(language, title).await?;
    wikitext.push('\n');
    let existing_links = parser.extract_links(wikitext.as_str()).unwrap();
    let text_segments = parser.extract_text(wikitext.as_str()).unwrap();

    // Load bloom filters (cached after first load per language)
    let bloom_pair = load_bloom_filters(language);
    let (title_filter, label_filter) = (&bloom_pair.0, &bloom_pair.1);

    // Process all text segments
    let link_suggestions =
        process_text_segments(text_segments, &title_filter, &label_filter, language);

    //dbg!(&link_suggestions);
    let mut filtered_suggestions =
        filter_suggestions(link_suggestions, existing_links, &title.to_string());
    //    dbg!(&filtered_suggestions);
    // Process suggestions sequentially, reusing one pooled connection for the
    // whole batch. Each candidate is a handful of microsecond point lookups, so
    // the work does not justify a rayon thread pool.
    with_connection(language, |conn| {
        for suggestion in &mut filtered_suggestions {
            suggestion.process(source_article.clone(), conn);
        }
    });
    // Collect accepted suggestions above the confidence threshold.
    let accepted: Vec<&LinkSuggestion> = filtered_suggestions
        .iter()
        .filter(|s| s.confidence_score() >= confidence_threshold)
        .collect();

    // Gather byte offsets and convert to char offsets in a single O(N) pass
    // over the wikitext, rather than doing an O(N) chars().count() per suggestion.
    let byte_offsets: Vec<usize> = accepted
        .iter()
        .map(|s| s.link_start_byte().unwrap_or(0))
        .collect();
    let char_offsets = byte_offsets_to_char_offsets(&wikitext, &byte_offsets);

    let suggestions: Vec<LinkSuggestionRecord> = accepted
        .iter()
        .zip(char_offsets)
        .filter_map(|(suggestion, char_start)| {
            // Drop any unresolved candidate that slipped through (e.g. at a zero
            // threshold) — only suggestions with a resolved title are emitted.
            suggestion.title.as_ref().map(|title| LinkSuggestionRecord {
                language: language.to_string(),
                title: title.to_owned(),
                link_text: suggestion.label.to_owned(),
                frequency: suggestion.frequency.unwrap_or_default(),
                score: suggestion.confidence_score(),
                wikitext_offset: char_start,
            })
        })
        .collect();
    Ok(LinkSuggestionsResult {
        language: language.to_string(),
        title: title.to_string(),
        confidence_score: confidence_threshold,
        wikitext,
        suggestions,
    })
}
