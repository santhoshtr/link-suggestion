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

// Define the subcommands: 'build' and 'check'.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Builds a Bloom filter from lines in a file and serializes it to disk.
    Build {
        /// Path to the input file containing words (one per line).
        #[arg(short, long, value_name = "FILE")]
        input_file: PathBuf,

        /// Path to save the serialized Bloom filter.
        #[arg(short, long, value_name = "FILE")]
        output_filter: PathBuf,

        /// Desired false positive probability (e.g., 0.01 for 1%).
        /// A smaller probability will result in a larger filter.
        #[arg(short, long, default_value = "0.01")]
        false_positive_probability: f64,
    },
    /// Checks if a word exists in a previously built Bloom filter.
    Check {
        /// Path to the serialized Bloom filter file.
        #[arg(short, long, value_name = "FILE")]
        filter_file: PathBuf,

        /// The word to check for existence in the filter.
        #[arg(short, long)]
        word: String,
    },
    Links {
        /// Wikipedia language code (e.g., "en", "fr", "de").
        #[arg(short, long)]
        language: String,
        /// Wikipedia article title.
        #[arg(short, long)]
        title: String,
        /// List all possible candidates
        #[arg(short, long)]
        candidates: bool,
        /// Path to the serialized Bloom filter file.
        #[arg(short, long, value_name = "FILE")]
        filter_file: PathBuf,
    },
}

// The main function where the program execution begins.
#[tokio::main]
async fn main() -> io::Result<()> {
    // Parse the command-line arguments.
    let cli = Cli::parse();

    // Match the subcommand to determine which operation to perform.
    match &cli.command {
        Commands::Build {
            input_file,
            output_filter,
            false_positive_probability,
        } => {
            // Build the Bloom filter from the input file.
            let filter_manager =
                BloomFilterManager::build_from_file(input_file, *false_positive_probability)?;

            // Save the filter to the output file.
            filter_manager.save_to_file(output_filter)?;

            println!("Bloom filter built and saved to {:?}", output_filter);
        }
        Commands::Check { filter_file, word } => {
            // Load the Bloom filter from file.
            let filter_manager = BloomFilterManager::load_from_file(filter_file)?;

            // Check the word and display the result.
            filter_manager.check_word_with_output(word);
        }
        Commands::Links {
            candidates,
            filter_file,
            language,
            title,
        } => {
            let mut parser = WikiText::new().unwrap();

            let mut wikitext = fetch_wikipedia_wikitext(language, title).await.unwrap();
            wikitext.push('\n');
            let existing_links = parser.extract_links(wikitext.as_str());
            dbg!(&existing_links);
            let text_segments = parser.extract_text(wikitext.as_str()).unwrap();
            dbg!(&text_segments);

            // Load the bloom filter
            let filter_manager = BloomFilterManager::load_from_file(filter_file)?;

            let mut link_suggestions = Vec::new();

            for segment in text_segments {
                let link_candidates = segment.link_candidates();
                dbg!(&link_candidates);

                // Filter candidates through the bloom filter
                let filtered_candidates: Vec<String> = link_candidates
                    .into_iter()
                    .filter(|candidate| {
                        let wiki_title = WikiTitle::new(candidate);
                        let normalized_title = wiki_title.normalized();
                        dbg!(&normalized_title);
                        filter_manager.check_word(normalized_title)
                    })
                    .collect();

                // Create LinkSuggestion for each filtered candidate
                for candidate in filtered_candidates {
                    let wiki_title = WikiTitle::new(&candidate);
                    let normalized_title = wiki_title.normalized().to_string();

                    link_suggestions.push(LinkSuggestion::new(
                        segment.clone(),
                        wiki_title,
                        candidate,
                    ));
                }
            }

            // Print all link suggestions using the Display trait
            println!("Link suggestions:");
            let link_suggestions = filter_suggestions(link_suggestions);
            for suggestion in &link_suggestions {
                println!("{}", suggestion);
            }
        }
    }

    Ok(())
}
