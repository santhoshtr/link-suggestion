use clap::{Parser, Subcommand};
use rusqlite::{Connection, Result};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]

struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]

enum Commands {
    /// Extract distinct link labels from the links table
    ExtractLabels {
        /// Path to the SQLite database file

        #[arg(short, long, value_name = "FILE")]
        database: PathBuf,
    },
}

fn extract_distinct_labels(db_path: &PathBuf) -> Result<()> {
    let conn = Connection::open(db_path)?;

    let mut stmt = conn.prepare("SELECT DISTINCT link_label FROM links")?;

    let label_iter = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;

    for label in label_iter {
        println!("{}", label?);
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::ExtractLabels { database } => {
            extract_distinct_labels(database)?;
        }
    }

    Ok(())
}
