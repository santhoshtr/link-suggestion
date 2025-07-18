use clap::{Parser, Subcommand};
use linksuggestion_bloom::BloomFilterManager;
use std::{io, path::PathBuf};

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

        /// Desired false positive probability (e.g., 0.001 for .1% or one-in-thousand)
        /// A smaller probability will result in a larger filter.
        #[arg(short, long, default_value = "0.001")]
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
}

// The main function where the program execution begins.
fn main() -> io::Result<()> {
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

            println!("Bloom filter built and saved to {output_filter:?}");
        }
        Commands::Check { filter_file, word } => {
            // Load the Bloom filter from file.
            let filter_manager = BloomFilterManager::load_from_file(filter_file)?;

            // Check the word and display the result.
            filter_manager.check_word_with_output(word);
        }
    }

    Ok(())
}
