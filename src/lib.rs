use link_suggestion::{LinkSuggestion, LinkSuggestionRecord, filter_suggestions};
use rayon::prelude::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wiki_title::{WikiTitle, fetch_wikipedia_wikitext};
use wikitext::{TextSegment, WikiText};

mod bloom_filter;
mod link_suggestion;
mod wiki_title;
mod wikitext;
use bloom_filter::BloomFilterManager;

#[derive(Debug, Deserialize, Serialize)]
pub struct LinkSuggestionsResult {
    pub language: String,
    pub title: String,
    pub confidence_score: f32,
    pub wikitext: String,
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
    link_suggestions.sort();
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
    let mut filtered_suggestions =
        filter_suggestions(link_suggestions, existing_links, &title.to_string());
    // Use parallel iterator to process suggestions in multiple threads
    filtered_suggestions.par_iter_mut().for_each(|suggestion| {
        suggestion.process(shared_conn.clone());
    });
    // Print only suggestions that meet the confidence threshold
    let mut suggestions: Vec<LinkSuggestionRecord> = Vec::new();
    for suggestion in &filtered_suggestions {
        if suggestion.confidence_score() >= confidence_threshold {
            let char_start = suggestion
                .calculate_link_positions_with_char_indices(&wikitext)
                .unwrap();

            // Calculate new offsets taking into account previous replacements
            suggestions.push(LinkSuggestionRecord {
                language: language.to_string(),
                title: suggestion.title.to_owned(),
                link_text: suggestion.label.to_owned(),
                frequency: suggestion.frequency.unwrap_or_default(),
                score: suggestion.confidence_score(),
                wikitext_offset: char_start,
            });
        }
    }
    Ok(LinkSuggestionsResult {
        language: language.to_string(),
        title: title.to_string(),
        confidence_score: confidence_threshold,
        wikitext,
        suggestions,
    })
}

fn get_db_connection(language: &str) -> Connection {
    let db_path = format!("anchor-dictionaries/{language}wiki.sqlite");
    Connection::open(&db_path).unwrap_or_else(|_| panic!("Error opening database {db_path}"))
}

#[derive(Serialize)]
pub struct FreqDistribution {
    categories: Vec<String>,
    // The count of items within each category/bin.
    data: Vec<f32>,
}

// The core logic for querying and processing the data.
pub fn generate_chart_data(language: &str) -> Result<FreqDistribution, rusqlite::Error> {
    let conn = get_db_connection(language);

    let mut stmt = conn.prepare(
        "select  count(link_title) as freq from links GROUP by link_title ORDER by freq desc",
    )?;

    let freqs = stmt.query_map([], |row| row.get(0))?;

    let mut bins: HashMap<String, f32> = HashMap::new();

    let mut rank = 1;
    for freq_result in freqs {
        let freq: u32 = freq_result?;
        let normalized_rank = (rank as f32).log10();
        let normalized_freq = (freq as f32).log10();
        let rounded = format!("{normalized_rank:.2}");
        bins.insert(rounded, normalized_freq);
        rank += 1;
    }

    // change to sort the categories by their numeric key. AI?
    let mut sorted_categories: Vec<String> = bins.keys().cloned().collect();
    sorted_categories.sort_by(|a, b| {
        let a_val: f32 = a.parse().unwrap_or(0.0);
        let b_val: f32 = b.parse().unwrap_or(0.0);
        a_val
            .partial_cmp(&b_val)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Create the data vector in the same order as the sorted categories
    let sorted_data: Vec<f32> = sorted_categories
        .iter()
        .map(|category| *bins.get(category).unwrap_or(&0.0))
        .collect();

    Ok(FreqDistribution {
        categories: sorted_categories,
        data: sorted_data,
    })
}
