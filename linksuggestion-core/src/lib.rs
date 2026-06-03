use database::get_db_connection;
use link_suggestion::{LinkSuggestion, LinkSuggestionRecord, filter_suggestions};
use linksuggestion_bloom::BloomFilterManager;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wiki_title::{WikiTitle, fetch_wikipedia_wikitext};
use wikitext::{TextSegment, WikiText};

mod database;
pub mod freq_distribution;
mod link_suggestion;
pub mod wiki_title;
pub mod wikitext;

#[derive(Debug, Deserialize, Serialize)]
pub struct LinkSuggestionsResult {
    pub language: String,
    pub title: String,
    pub confidence_score: f32,
    pub wikitext: String,
    pub suggestions: Vec<LinkSuggestionRecord>,
}

fn load_bloom_filters(language: &str) -> (BloomFilterManager, BloomFilterManager) {
    let data_dir = std::env::var("TOOL_DATA_DIR").unwrap_or_else(|_| ".".to_string());

    let link_title_bloom_filter_file =
        PathBuf::from(format!("{data_dir}/bloom/{language}wiki.bloom"));
    let link_title_filter_manager =
        BloomFilterManager::load_from_file(&link_title_bloom_filter_file)
            .unwrap_or_else(|_| panic!("Error reading file {data_dir}/bloom/{language}wiki.bloom"));

    let link_label_bloom_filter_file =
        PathBuf::from(format!("{data_dir}/bloom/{language}wiki.labels.bloom"));
    let link_label_filter_manager =
        BloomFilterManager::load_from_file(&link_label_bloom_filter_file).unwrap_or_else(|_| {
            panic!(" Error reading file {data_dir}/bloom/{language}wiki.labels.bloom")
        });

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
) -> Result<LinkSuggestionsResult, Box<dyn Error>> {
    let mut parser = WikiText::new().unwrap();

    let source_article = WikiTitle::new(title, language.to_string());
    let mut wikitext = fetch_wikipedia_wikitext(language, title).await?;
    wikitext.push('\n');
    let existing_links = parser.extract_links(wikitext.as_str()).unwrap();
    let text_segments = parser.extract_text(wikitext.as_str()).unwrap();

    // Load bloom filters
    let (title_filter, label_filter) = load_bloom_filters(language);

    // Process all text segments
    let link_suggestions =
        process_text_segments(text_segments, &title_filter, &label_filter, language);

    //dbg!(&link_suggestions);
    // Create a shared connection wrapped in Arc<Mutex<>>
    let connection = get_db_connection(language);
    let shared_conn = Arc::new(Mutex::new(connection));
    let mut filtered_suggestions =
        filter_suggestions(link_suggestions, existing_links, &title.to_string());
    //    dbg!(&filtered_suggestions);
    // Use parallel iterator to process suggestions in multiple threads
    filtered_suggestions.par_iter_mut().for_each(|suggestion| {
        suggestion.process(source_article.clone(), shared_conn.clone());
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
