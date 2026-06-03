use clap::Parser;
use linksuggestion_core::{debug_text_command, process_links_command};
use std::error::Error;

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
    /// Wikipedia article title: fetch its wikitext and suggest links.
    #[arg(short, long, conflicts_with = "text", required_unless_present = "text")]
    title: Option<String>,
    /// Debug a single word or phrase: report whether it can become a link, with
    /// its frequency distribution and confidence score.
    #[arg(long)]
    text: Option<String>,
    /// Confidence threshold (0.0–1.0). Only used with --title.
    #[arg(short, long, default_value_t = 0.5)]
    confidence: f32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Parse the command-line arguments.
    let cli = Cli::parse();

    if let Some(text) = cli.text.as_deref() {
        debug_text_command(&cli.language, text)?;
    } else {
        // `required_unless_present = "text"` guarantees a title here.
        let title = cli.title.as_deref().unwrap();
        let result = process_links_command(&cli.language, title, cli.confidence).await?;
        for suggestion in result.suggestions {
            println!("{suggestion}");
        }
    }

    Ok(())
}
