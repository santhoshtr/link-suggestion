// Import necessary crates for command-line argument parsing.
use clap::{Parser, Subcommand};
use link_suggestion::{LinkSuggestion, filter_suggestions};
use std::io;
use std::path::PathBuf;
use wiki_title::{WikiTitle, fetch_wikipedia_wikitext};
use wikitext::WikiText;

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

// The main function where the program execution begins.
#[tokio::main]
async fn main() -> io::Result<()> {
    // Parse the command-line arguments.
    let cli = Cli::parse();

    // Match the subcommand to determine which operation to perform.
    match &cli.command {
        Commands::Links { language, title } => {
            let mut parser = WikiText::new().unwrap();

            let mut wikitext = fetch_wikipedia_wikitext(language, title).await.unwrap();
            wikitext.push('\n');
            let existing_links = parser.extract_links(wikitext.as_str()).unwrap();
            let text_segments = parser.extract_text(wikitext.as_str()).unwrap();
            dbg!(&existing_links);

            // Load the bloom filter
            let link_title_bloom_filter_file = PathBuf::from(format!("bloom/{language}wiki.bloom"));
            let link_title_filter_manager =
                BloomFilterManager::load_from_file(&link_title_bloom_filter_file)
                    .unwrap_or_else(|_| panic!("Error reading file bloom/{language}wiki.bloom"));
            let link_label_bloom_filter_file =
                PathBuf::from(format!("bloom/{language}wiki.labels.bloom"));
            let link_label_filter_manager = BloomFilterManager::load_from_file(
                &link_label_bloom_filter_file,
            )
            .unwrap_or_else(|_| panic!(" Error reading file bloom/{language}wiki.labels.bloom"));

            let mut link_suggestions = Vec::new();

            for segment in text_segments {
                let link_candidates = segment.link_candidates();

                // Filter candidates through the bloom filter
                let filtered_title_candidates: Vec<String> = link_candidates
                    .into_iter()
                    .filter(|candidate| {
                        let wiki_title = WikiTitle::new(candidate);
                        let normalized_title = wiki_title.normalized();
                        link_title_filter_manager.exist(normalized_title)
                    })
                    .collect();

                // Create LinkSuggestion for each filtered candidate
                for candidate in filtered_title_candidates {
                    let wiki_title = WikiTitle::new(&candidate);
                    link_suggestions.push(LinkSuggestion::new(
                        segment.clone(),
                        wiki_title,
                        candidate,
                    ));
                }
                let filtered_label_candidates: Vec<String> = link_candidates
                    .into_iter()
                    .filter(|candidate| link_label_filter_manager.exist(candidate))
                    .collect();

                for candidate in filtered_label_candidates {
                    // Query links table for link_title where link_label = candidate
                    // using rusqlite and database as anchor-dictionaries/languagewiki.sqlite. AI!
                    let wiki_title = WikiTitle::new(&link_title);
                    link_suggestions.push(LinkSuggestion::new(
                        segment.clone(),
                        wiki_title,
                        candidate,
                    ));
                }
            }

            // Print all link suggestions using the Display trait
            println!("Link suggestions:");
            let link_suggestions = filter_suggestions(link_suggestions, existing_links, title);
            for suggestion in &link_suggestions {
                println!("{suggestion}");
            }
        }
    }

    Ok(())
}
