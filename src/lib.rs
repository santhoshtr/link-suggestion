use link_suggestion::{LinkSuggestion, LinkSuggestionRecord, filter_suggestions};
use rayon::prelude::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wiki_title::{WikiTitle, fetch_wikipedia_wikitext};
use wikitext::{TextSegment, WikiText};

mod bloom_filter;
mod link_suggestion;
mod stopwords;
mod wiki_title;
mod wikitext;
use bloom_filter::BloomFilterManager;

#[derive(Debug, Deserialize, Serialize)]
pub struct LinkSuggestionsResult {
    pub language: String,
    pub title: String,
    pub confidence_score: f32,
    pub original_wikitext: String,
    pub new_wikitext: String,
    pub suggestions: Vec<LinkSuggestionRecord>,
}

fn load_bloom_filters(language: &str) -> (BloomFilterManager, BloomFilterManager) {
    let link_title_bloom_filter_file = PathBuf::from(format!("bloom/{language}wiki.bloom"));
    let link_title_filter_manager =
        BloomFilterManager::load_from_file(&link_title_bloom_filter_file)
            .unwrap_or_else(|_| panic!("Error reading file bloom/{language}wiki.bloom"));

    let link_label_bloom_filter_file = PathBuf::from(format!("bloom/{language}wiki.labels.bloom"));
    let link_label_filter_manager =
        BloomFilterManager::load_from_file(&link_label_bloom_filter_file)
            .unwrap_or_else(|_| panic!(" Error reading file bloom/{language}wiki.labels.bloom"));

    (link_title_filter_manager, link_label_filter_manager)
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
    language: &str,
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
        let wiki_title = WikiTitle::new("WE_WILL_FIGURE_OUT_LATER", language.to_owned());
        suggestions.push(LinkSuggestion::new_with_label(
            segment.clone(),
            wiki_title,
            candidate,
        ));
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
        let label_suggestions =
            process_label_candidates(&segment, link_candidates, label_filter, language);
        link_suggestions.extend(label_suggestions);
    }
    link_suggestions.dedup();

    link_suggestions
}

pub async fn process_links_command(
    language: &str,
    title: &str,
    confidence_threshold: f32,
) -> io::Result<LinkSuggestionsResult> {
    let mut parser = WikiText::new().unwrap();

    let mut wikitext = fetch_wikipedia_wikitext(language, title).await.unwrap();
    wikitext.push('\n');
    let existing_links = parser.extract_links(wikitext.as_str()).unwrap();
    let text_segments = parser.extract_text(wikitext.as_str()).unwrap();

    // Load bloom filters
    let (title_filter, label_filter) = load_bloom_filters(language);

    // Process all text segments
    let link_suggestions =
        process_text_segments(text_segments, &title_filter, &label_filter, language);

    // Create a shared connection wrapped in Arc<Mutex<>>
    let connection = get_db_connection(language);
    let shared_conn = Arc::new(Mutex::new(connection));
    // Print filtered link suggestions
    println!("Link suggestions:");
    println!("candidates:{}", link_suggestions.len());
    let mut filtered_suggestions =
        filter_suggestions(link_suggestions, existing_links, &title.to_string());
    // Use parallel iterator to process suggestions in multiple threads
    filtered_suggestions.par_iter_mut().for_each(|suggestion| {
        suggestion.process(shared_conn.clone());
    });
    let mut new_wikitext = wikitext.clone();
    // Print only suggestions that meet the confidence threshold
    let mut offset = 0;
    let mut suggestions: Vec<LinkSuggestionRecord> = Vec::new();
    for suggestion in &filtered_suggestions {
        if suggestion.confidence_score() >= confidence_threshold {
            println!("{suggestion}");
            let (byte_offset_start, byte_offset_end, char_start, char_end, replacement) =
                suggestion
                    .calculate_link_positions_with_char_indices(&wikitext)
                    .unwrap();

            // Calculate new offsets taking into account previous replacements
            let mut range_start = byte_offset_start + offset;
            let mut range_end = byte_offset_end + offset;

            // Make sure range_start is at a char boundary
            while range_start < new_wikitext.len() && !new_wikitext.is_char_boundary(range_start) {
                range_start += 1;
            }

            // Make sure range_end is at a char boundary
            while range_end < new_wikitext.len() && !new_wikitext.is_char_boundary(range_end) {
                range_end += 1;
            }

            // Make sure we're not out of bounds
            if range_start < new_wikitext.len() && range_end <= new_wikitext.len() {
                new_wikitext.replace_range(range_start..range_end, replacement.as_str());
                offset += replacement.len() - (range_end - range_start);
            }
            suggestions.push(LinkSuggestionRecord {
                title: suggestion.title.to_owned(),
                label: suggestion.label.to_owned(),
                frequency: suggestion.frequency.unwrap_or_default(),
                confidence_score: suggestion.confidence_score(),
                byte_offset_start,
                byte_offset_end,
                char_offset_start: char_start,
                char_offset_end: char_end,

                link_text: replacement,
            });
        }
    }
    Ok(LinkSuggestionsResult {
        language: language.to_string(),
        title: title.to_string(),
        confidence_score: confidence_threshold,
        original_wikitext: wikitext,
        new_wikitext,
        suggestions,
    })
}

fn get_db_connection(language: &str) -> Connection {
    let db_path = format!("anchor-dictionaries/{}wiki.sqlite", language);
    Connection::open(&db_path).unwrap_or_else(|_| panic!("Error opening database {db_path}"))
}
