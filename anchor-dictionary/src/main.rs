use clap::Parser;
use linksuggestion_core::wiki_title::WikiTitle;
use linksuggestion_core::wikitext::WikiText;
use quick_xml::NsReader;
use quick_xml::events::Event;
use rusqlite::Connection;
use rusqlite::params;
use std::fs;
use std::io::BufWriter;
use std::io::Write;

#[derive(Parser)]
struct Args {
    /// bz2 compressed XML dump file from a wikipedia
    #[arg(short, long)]
    input: String,
    #[arg(short, long)]
    language: String,

    /// Output file name
    #[arg(short, long, default_value = "links.tsv")]
    output: String,

    /// Output format (tsv or sqlite)
    #[arg(short, long, default_value = "tsv")]
    format: String,

    /// Batch size for SQLite insertions
    #[arg(short, long, default_value = "10000")]
    batch_size: usize,
}

#[derive(Debug, Clone)]
pub struct Article {
    pub language: String,
    pub text: String,
    pub id: String,
    pub namespace: usize,
    pub title: WikiTitle,
    pub redirect: bool,
}

#[derive(Debug, Clone)]
struct LinkRecord {
    pub article_title: String,
    pub link_title: String,
    pub link_label: String,
}

// Example usage
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut parser = WikiText::new().unwrap();
    let mut total_links = 0;
    let mut articles_processed = 0;
    let mut parsing_errors = 0;

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
        // NOTE: All titles in this table are normalized. link_label is lowercase.
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
        // For querying by link_title
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_link_title ON links(link_title)",
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
                    record.link_label.to_lowercase(),
                ])?;
            }
        }
        tx.commit()?;
        batch.clear();
        Ok(())
    };
    let language = if let Some(stripped) = args.language.strip_suffix("wiki") {
        stripped
    } else {
        args.language.as_str()
    };
    // Read the file and pass content to extract_links.
    let file_name = &args.input;
    let file = std::fs::File::open(file_name).unwrap();
    let bz2_file = std::io::BufReader::new(file);
    let decoder = bzip2::bufread::MultiBzDecoder::new(bz2_file);
    let buffered_reader = std::io::BufReader::new(decoder);
    let mut xml_reader = NsReader::from_reader(buffered_reader);
    xml_reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut article = Article {
        language: language.to_owned(),
        text: String::new(),
        id: String::new(),
        namespace: 0,
        title: WikiTitle::new("", language.to_uppercase()),
        redirect: false,
    };
    let mut tag_stack: Vec<String> = Vec::new();

    // Extract the text under the <text> node
    loop {
        match xml_reader.read_event_into(&mut buf) {
            Err(e) => panic!("Error at position {}: {:?}", xml_reader.error_position(), e),
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
                        article.title = WikiTitle::new(
                            e.decode().unwrap().into_owned().as_str(),
                            article.language.to_owned(),
                        );
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
                                link.label.as_deref().unwrap_or(link.title.normalized());

                            if let Some(ref mut writer) = tsv_writer {
                                writeln!(
                                    writer,
                                    "{}\t{}\t{}",
                                    article.title.normalized(),
                                    link.title,
                                    link_label,
                                )?;
                            }

                            if conn.is_some() {
                                batch_buffer.push(LinkRecord {
                                    article_title: article.title.normalized().to_string(),
                                    link_title: link.title.normalized().to_string(),
                                    link_label: link_label.to_lowercase(),
                                });

                                // Flush batch when it reaches the specified size
                                if batch_buffer.len() >= args.batch_size {
                                    if let Some(ref mut connection) = conn {
                                        flush_batch(connection, &mut batch_buffer)?;
                                    }
                                }
                            }
                        }

                        if articles_processed % args.batch_size == 0 {
                            println!(
                                "Articles processed: {articles_processed}, Links collected: {total_links} "
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
        "Articles processed: {articles_processed}\nLinks collected: {total_links}\nErrors: {parsing_errors}\n",
    );
    Ok(())
}
