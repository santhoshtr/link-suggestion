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
