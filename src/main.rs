use bloom_wiki::process_links_command;
// Import necessary crates for command-line argument parsing.
use clap::{Parser, Subcommand};
use link_suggestion::{LinkSuggestion, filter_suggestions};
use rayon::prelude::*;
use rusqlite::Connection;
use similar::TextDiff;
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

// Define the command-line interface using clap.
// This struct will parse the main arguments.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Define the subcommands.
#[derive(Subcommand, Debug)]
enum Commands {
    Links {
        /// Wikipedia language code (e.g., "en", "fr", "de").
        #[arg(short, long)]
        language: String,
        /// Wikipedia article title.
        #[arg(short, long)]
        title: String,
        // confidence threshold. Value between 0.0 and 0.1
        #[arg(short, long)]
        confidence: f32,
    },
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

fn get_db_connection(language: &str) -> Connection {
    let db_path = format!("anchor-dictionaries/{}wiki.sqlite", language);
    Connection::open(&db_path).unwrap_or_else(|_| panic!("Error opening database {db_path}"))
}

// The main function where the program execution begins.
#[tokio::main]
async fn main() -> io::Result<()> {
    // Parse the command-line arguments.
    let cli = Cli::parse();

    // Match the subcommand to determine which operation to perform.
    match &cli.command {
        Commands::Links {
            language,
            title,
            confidence,
        } => {
            let result = process_links_command(language, title, *confidence).await?;
            for suggestion in result.suggestions {
                println!("{suggestion}");
            }
            let text_diff = TextDiff::from_lines(
                result.original_wikitext.as_str(),
                result.new_wikitext.as_str(),
            );
            print!(
                "{}",
                text_diff
                    .unified_diff()
                    .context_radius(2)
                    .header("old_file", "new_file")
            );
        }
    }

    Ok(())
}
