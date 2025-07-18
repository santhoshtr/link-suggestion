use clap::Parser;
use linksuggestion_core::process_links_command;
use std::io;

mod link_suggestion;
mod wiki_title;
mod wikitext;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Wikipedia language code (e.g., "en", "fr", "de").
    #[arg(short, long)]
    language: String,
    /// Wikipedia article title.
    #[arg(short, long)]
    title: String,
    // confidence threshold. Value between 0.0 and 0.1
    #[arg(short, long)]
    confidence: f32,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    // Parse the command-line arguments.
    let cli = Cli::parse();

    // Match the subcommand to determine which operation to perform.
    let result = process_links_command(&cli.language, &cli.title, cli.confidence).await?;
    for suggestion in result.suggestions {
        println!("{suggestion}");
    }

    Ok(())
}
