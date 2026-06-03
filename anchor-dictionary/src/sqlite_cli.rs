use clap::Parser;
use rusqlite::{Connection, Result};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Path to the SQLite database file

    #[arg(short, long)]
    database: PathBuf,

    /// SQL query to execute
    #[arg(short, long)]
    query: String,

    /// Suppress the column header row
    #[arg(long)]
    no_header: bool,
}

fn execute_query(db_path: &PathBuf, query: &str, no_header: bool) -> Result<()> {
    let conn = Connection::open(db_path)?;

    let mut stmt = conn.prepare(query)?;

    let column_count = stmt.column_count();
    let column_names: Vec<String> = (0..column_count)
        .map(|i| stmt.column_name(i).unwrap_or("").to_string())
        .collect();

    let rows = stmt.query_map([], |row| {
        let mut values = Vec::new();
        for i in 0..column_count {
            let value: String = match row.get(i) {
                Ok(val) => val,
                Err(_) => "NULL".to_string(),
            };
            values.push(value);
        }
        Ok(values)
    })?;

    // Print column headers
    if !no_header && !column_names.is_empty() {
        println!("{}", column_names.join("\t"));
    }

    // Print rows
    for row in rows {
        let values = row?;
        println!("{}", values.join("\t"));
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    execute_query(&cli.database, &cli.query, cli.no_header)?;

    Ok(())
}
