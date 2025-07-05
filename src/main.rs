// Import necessary crates for command-line argument parsing.
use clap::{Parser, Subcommand};
use link_suggestion::{LinkSuggestion, filter_suggestions};
use rusqlite::Connection;
use std::io;
use std::path::PathBuf;
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

fn open_database(language: &str) -> Connection {
    let db_path = format!("anchor-dictionaries/{language}wiki.sqlite");
    Connection::open(&db_path).unwrap_or_else(|_| panic!("Error opening database {db_path}"))
}

fn title_exists_in_database(conn: &Connection, normalized_title: &str) -> bool {
    let mut stmt = conn
        .prepare("SELECT 1 FROM links WHERE article_title = ?1 LIMIT 1")
        .unwrap();
    stmt.exists([normalized_title]).unwrap_or(false)
}

fn process_title_candidates(
    segment: &TextSegment,
    candidates: Vec<String>,
    title_filter: &BloomFilterManager,
    conn: &Connection,
) -> Vec<LinkSuggestion> {
    let mut suggestions = Vec::new();

    let filtered_candidates: Vec<String> = candidates
        .into_iter()
        .filter(|candidate| {
            let wiki_title = WikiTitle::new(candidate);
            let normalized_title = wiki_title.normalized();
            title_filter.exist(normalized_title)
        })
        .collect();

    for candidate in filtered_candidates {
        let wiki_title = WikiTitle::new(&candidate);
        let normalized_title = wiki_title.normalized();

        // Account for the false positives in the bloom filter.
        // Now that we have short listed candidate list, make sure they are
        // indeed valid titles. Query the database see existence of link_title
        // matching the normalized title.
        if title_exists_in_database(conn, normalized_title) {
            let confidence_score = 0.5;
            suggestions.push(LinkSuggestion::new(
                segment.clone(),
                wiki_title,
                candidate,
                confidence_score,
            ));
        }
    }

    suggestions
}

fn query_link_titles_for_labels_batch(
    conn: &Connection,
    candidates: &[String],
) -> rusqlite::Result<Vec<(String, String, i32)>> {
    if candidates.is_empty() {
        return Ok(vec![]);
    }

    // Create placeholders for the IN clause
    let placeholders: Vec<&str> = candidates.iter().map(|_| "?").collect();
    let placeholders_str = placeholders.join(",");

    let query = format!(
        "SELECT link_label, link_title, count(link_title) as freq FROM links WHERE link_label IN ({placeholders_str}) COLLATE NOCASE GROUP by link_label, link_title ORDER BY link_label,freq DESC"
    );

    let mut stmt = conn.prepare(&query)?;

    // Convert candidates to rusqlite::types::Value for parameter binding
    let params: Vec<&dyn rusqlite::ToSql> = candidates
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();

    let results: Result<Vec<(String, String, i32)>, _> = stmt
        .query_map(&params[..], |row| {
            Ok((
                row.get::<_, String>(0)?, // link_label
                row.get::<_, String>(1)?, // link_title
                row.get::<_, i32>(2)?,    // freq
            ))
        })?
        .collect();

    results
}

fn should_skip_label_results(results: &[(String, i32)]) -> bool {
    if results.len() > 1 {
        return true;
    }
    if let Some((_, first_freq)) = results.first() {
        if *first_freq <= 1 {
            return true;
        }
    }

    false
}

fn process_label_candidates(
    segment: &TextSegment,
    candidates: Vec<String>,
    label_filter: &BloomFilterManager,
    conn: &Connection,
) -> Vec<LinkSuggestion> {
    let mut suggestions = Vec::new();

    let filtered_candidates: Vec<String> = candidates
        .into_iter()
        .filter(|candidate| label_filter.exist(candidate))
        .collect();

    if filtered_candidates.is_empty() {
        return suggestions;
    }

    // Query all candidates at once
    if let Ok(batch_results) = query_link_titles_for_labels_batch(conn, &filtered_candidates) {
        // Group results by label
        use std::collections::HashMap;
        let mut grouped_results: HashMap<String, Vec<(String, i32)>> = HashMap::new();

        for (label, title, freq) in batch_results {
            grouped_results
                .entry(label)
                .or_insert_with(Vec::new)
                .push((title, freq));
        }

        // Process each label's results
        for (label, results) in grouped_results {
            if should_skip_label_results(&results) {
                continue;
            }

            for (link_title, freq) in results {
                let wiki_title = WikiTitle::new(&link_title);
                let confidence_score = (freq as f32) * 0.1 + 0.1;
                suggestions.push(LinkSuggestion::new(
                    segment.clone(),
                    wiki_title,
                    label.clone(),
                    confidence_score,
                ));
            }
        }
    }

    suggestions
}

fn process_text_segments(
    text_segments: Vec<TextSegment>,
    title_filter: &BloomFilterManager,
    label_filter: &BloomFilterManager,
    conn: &Connection,
) -> Vec<LinkSuggestion> {
    let mut link_suggestions = Vec::new();

    for segment in text_segments {
        let mut link_candidates = segment.link_candidates();
        link_candidates.sort();
        link_candidates.dedup();

        // Process title candidates
        let title_suggestions =
            process_title_candidates(&segment, link_candidates.clone(), title_filter, conn);
        link_suggestions.extend(title_suggestions);

        // Process label candidates
        let label_suggestions =
            process_label_candidates(&segment, link_candidates, label_filter, conn);
        link_suggestions.extend(label_suggestions);
    }

    link_suggestions
}

async fn process_links_command(language: &str, title: &str) -> io::Result<()> {
    let mut parser = WikiText::new().unwrap();

    let mut wikitext = fetch_wikipedia_wikitext(language, title).await.unwrap();
    wikitext.push('\n');
    let existing_links = parser.extract_links(wikitext.as_str()).unwrap();
    let text_segments = parser.extract_text(wikitext.as_str()).unwrap();
    dbg!(&existing_links);

    // Load bloom filters
    let (title_filter, label_filter) = load_bloom_filters(language);

    // Open database connection
    let conn = open_database(language);

    // Process all text segments
    let link_suggestions =
        process_text_segments(text_segments, &title_filter, &label_filter, &conn);

    // Print filtered link suggestions
    println!("Link suggestions:");
    let filtered_suggestions =
        filter_suggestions(link_suggestions, existing_links, &title.to_string());
    for suggestion in &filtered_suggestions {
        println!("{suggestion}");
    }

    Ok(())
}

// The main function where the program execution begins.
#[tokio::main]
async fn main() -> io::Result<()> {
    // Parse the command-line arguments.
    let cli = Cli::parse();

    // Match the subcommand to determine which operation to perform.
    match &cli.command {
        Commands::Links { language, title } => {
            process_links_command(language, title).await?;
        }
    }

    Ok(())
}
