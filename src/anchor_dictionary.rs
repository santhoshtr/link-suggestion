use clap::Parser;
use rusqlite::Connection;
use rusqlite::params;
use std::fs;
use std::io::Write;
use wikitext::WikiText;
mod wiki_title;
mod wikitext;

#[derive(Parser)]
struct Args {
    /// Input file name
    #[arg(short, long)]
    input: String,

    /// Output file name
    #[arg(short, long, default_value = "links.tsv")]
    output: String,

    /// Output format (tsv or sqlite)
    #[arg(short, long, default_value = "tsv")]
    format: String,

    /// Batch size for SQLite insertions
    #[arg(short, long, default_value = "1000")]
    batch_size: usize,
}

#[derive(Debug, Clone)]
pub struct Article {
    pub text: String,
    pub id: String,
    pub namespace: usize,
    pub title: String,
    pub redirect: bool,
}

#[derive(Debug, Clone)]
struct LinkRecord {
    article_title: String,
    link_title: String,
    link_label: String,
}

// Example usage
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut parser = WikiText::new().unwrap();
    let mut total_links = 0;
    let mut articles_processed = 0;
    let mut parsing_errors = 0;

    use std::io::BufWriter;
    let mut tsv_writer = if args.format == "tsv" {
        let tsv_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&args.output)?;
        Some(BufWriter::new(tsv_file))
    } else {
        None
    };

    let mut conn = if args.format == "sqlite" {
        let conn = Connection::open(&args.output)?;

        // Optimize SQLite settings
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "cache_size", 1000000)?;
        conn.pragma_update(None, "temp_store", "memory")?;
        conn.pragma_update(None, "mmap_size", 268435456)?; // 256MB
        conn.execute(
            "CREATE TABLE IF NOT EXISTS links (
                article_title TEXT,
                link_title TEXT,
                link_label TEXT
            )",
            [],
        )?;

        // Create index for better performance if querying later
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_article_title ON links(article_title)",
            [],
        )?;
        // For querying by link_label
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_link_label ON links(link_label)",
            [],
        )?;

        Some(conn)
    } else {
        None
    };

    // Batch buffer for SQLite insertions
    let mut batch_buffer: Vec<LinkRecord> = Vec::with_capacity(args.batch_size);

    // Function to flush batch to SQLite
    let flush_batch = |conn: &mut Connection,
                       batch: &mut Vec<LinkRecord>|
     -> Result<(), Box<dyn std::error::Error>> {
        if batch.is_empty() {
            return Ok(());
        }

        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO links (article_title, link_title, link_label) VALUES (?1, ?2, ?3)",
            )?;

            for record in batch.iter() {
                stmt.execute(params![
                    record.article_title,
                    record.link_title,
                    record.link_label,
                ])?;
            }
        }
        tx.commit()?;
        batch.clear();
        Ok(())
    };

    // Read the file and pass content to extract_links. No need to read from stdin.
    let file_name = &args.input;
    use quick_xml::Reader;
    use quick_xml::events::Event;

    use std::io::BufReader;
    let file = fs::File::open(file_name)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut article = Article {
        text: String::new(),
        id: String::new(),
        namespace: 0,
        title: String::new(),
        redirect: false,
    };
    let mut tag_stack: Vec<String> = Vec::new();

    // Extract the text under the <text> node
    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => panic!("Error at position {}: {:?}", reader.error_position(), e),
            // exits the loop when reaching end of file
            Ok(Event::Eof) => break,

            Ok(Event::Start(e)) => {
                let base_tag = e
                    .name()
                    .into_inner()
                    .to_vec()
                    .into_iter()
                    .map(|c| c as char)
                    .collect::<String>();

                tag_stack.push(base_tag);
                let path = tag_stack.join("/");
                if path == "mediawiki/page" {
                    article.text.clear();
                    article.id.clear();
                    article.namespace = 0;
                    article.redirect = false;
                }
            }
            Ok(Event::Empty(e)) => {
                let base_tag = e
                    .name()
                    .into_inner()
                    .to_vec()
                    .into_iter()
                    .map(|c| c as char)
                    .collect::<String>();
                if base_tag == "redirect" {
                    article.redirect = true
                }
            }
            Ok(Event::Text(e)) => {
                let path = tag_stack.join("/");
                match path.as_str() {
                    "mediawiki/page/revision/text" => {
                        article.text = e.decode().unwrap().into_owned();
                    }
                    "mediawiki/page/id" => {
                        article.id = e.decode().unwrap().into_owned();
                    }
                    "mediawiki/page/ns" => {
                        article.namespace = e.decode().unwrap().parse::<usize>().unwrap_or(999999);
                    }
                    "mediawiki/page/title" => {
                        article.title = e.decode().unwrap().into_owned();
                    }
                    _ => (),
                }
            }
            Ok(Event::End(_e)) => {
                let path = tag_stack.join("/");
                tag_stack.pop();
                if path.as_str() == "mediawiki/page/revision/text" {
                    article.text.push('\n');

                    // Only process links if namespace is 0 and redirect is false
                    if article.namespace == 0 && !article.redirect {
                        articles_processed += 1;
                        let links = match parser.extract_links(&article.text) {
                            Ok(links) => links,
                            Err(_) => {
                                eprintln!(
                                    "Error parsing article: id={}, title={}, namespace={}",
                                    article.id, article.title, article.namespace
                                );
                                let dir = "data";
                                fs::create_dir_all(dir)?;
                                let file_path = format!("{}/{}.wikitext", dir, article.id);
                                let mut file = fs::File::create(file_path)?;
                                file.write_all(article.text.as_bytes())?;
                                // Recreate parser
                                parser = WikiText::new().unwrap();
                                parsing_errors += 1;
                                continue;
                            }
                        };
                        total_links += links.len();

                        for link in links.iter() {
                            let link_label =
                                link.label.as_deref().unwrap_or(&link.title.normalized());

                            if let Some(ref mut writer) = tsv_writer {
                                writeln!(
                                    writer,
                                    "{}\t{}\t{}",
                                    article.title, link.title, link_label,
                                )?;
                            }

                            if conn.is_some() {
                                batch_buffer.push(LinkRecord {
                                    article_title: article.title.clone(),
                                    link_title: link.title.normalized().to_string(),
                                    link_label: link_label.to_string(),
                                });

                                // Flush batch when it reaches the specified size
                                if batch_buffer.len() >= args.batch_size {
                                    if let Some(ref mut connection) = conn {
                                        flush_batch(connection, &mut batch_buffer)?;
                                    }
                                }
                            }
                        }

                        if articles_processed % 1000 == 0 {
                            println!(
                                "Articles processed: {}, Links collected: {}, Batch buffer size: {}",
                                articles_processed,
                                total_links,
                                batch_buffer.len()
                            );
                        }
                    }
                }
            }
            _ => (),
        }
    }

    // Flush any remaining records in the batch buffer
    if let Some(ref mut connection) = conn {
        flush_batch(connection, &mut batch_buffer)?;
    }

    println!(
        "Articles processed: {}\nLinks collected: {}\nErrors: {}\n",
        articles_processed, total_links, parsing_errors
    );
    Ok(())
}
