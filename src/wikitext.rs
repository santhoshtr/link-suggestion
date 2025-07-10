use std::fmt;
use tree_sitter::{Node, ParseOptions, Parser, Query, QueryCursor, Range, StreamingIterator};
use tree_sitter_wikitext::LANGUAGE;

use crate::wiki_title::WikiTitle;

#[derive(Debug, Clone)]
pub struct WikiLink {
    pub label: Option<String>,
    pub title: WikiTitle,
    pub range: Range,
}

#[derive(Debug, Clone)]
pub struct TextSegment {
    pub text: String,
    pub range: Range,
}

impl fmt::Display for TextSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TextSegment[{}:{}-{}:{}]: \"{}\"",
            self.range.start_point.row,
            self.range.start_point.column,
            self.range.end_point.row,
            self.range.end_point.column,
            self.text
        )
    }
}

pub struct WikiText {
    parser: Parser,
    link_query: Query,
    text_query: Query,
}

impl WikiText {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let language = LANGUAGE.into();
        let mut parser = Parser::new();
        parser.set_language(&language)?;

        // Tree-sitter query to match different types of links
        let link_query_str = r#"
            (wikilink
              (wikilink_page) @link.title
              (page_name_segment)? @link.label
            )
        "#;

        let text_query_str = "(paragraph (text) @text )";

        let link_query = Query::new(&language, link_query_str)?;
        let text_query = Query::new(&language, text_query_str)?;

        Ok(WikiText {
            parser,
            link_query,
            text_query,
        })
    }

    pub fn extract_links(&mut self, wikitext: &str) -> Result<Vec<WikiLink>, &'static str> {
        let parse_opts = ParseOptions::new();
        let tree = self.parser.parse_with_options(
            &mut |i, _| {
                if i < wikitext.len() {
                    &wikitext[i..]
                } else {
                    ""
                }
            },
            None,
            Some(parse_opts),
        );
        if let Some(tree) = tree {
            let root_node = tree.root_node();
            let mut query_cursor = QueryCursor::new();
            // query_cursor.set_point_range(ops::Range {
            //     start: Point { row: 0, column: 0 },
            //     end: Point {
            //         row: 200,
            //         column: 0,
            //     },
            // });
            let mut captures =
                query_cursor.captures(&self.link_query, root_node, wikitext.as_bytes());

            let mut links = Vec::new();
            while let Some((mat, _capture_index)) = captures.next() {
                let mut current_link_label = None;
                let mut current_link_title: WikiTitle = WikiTitle::new("", String::from("en"));
                let mut current_link_range = mat.captures[0].node.range();
                // Process all captures in this match
                for capture in mat.captures {
                    let capture_name = &self.link_query.capture_names()[capture.index as usize];
                    let node_text = get_node_text(capture.node, wikitext);

                    match *capture_name {
                        "link.title" => {
                            let title = node_text.trim_matches('"').trim_matches('\'');
                            if !title.contains(':') && !title.contains('.') {
                                current_link_title = WikiTitle::new(title,String::from("en"));
                                current_link_range = capture.node.range();
                            }
                        }
                        "link.label" => {
                            current_link_label = Some(node_text);
                        }
                        _ => {}
                    }
                }
                let current_link = WikiLink {
                    title: current_link_title,
                    range: current_link_range,
                    label: current_link_label,
                };

                // Only add if we found a valid title
                if current_link.title.is_valid() {
                    links.push(current_link);
                }
            }
            Ok(links)
        } else {
            Err("Parse error")
        }
    }

    pub fn extract_text(&mut self, wikitext: &str) -> Result<Vec<TextSegment>, &'static str> {
        let tree = self.parser.parse(wikitext, None);
        if let Some(tree) = tree {
            let root_node = tree.root_node();
            let mut cursor = QueryCursor::new();
            let mut captures = cursor.captures(&self.text_query, root_node, wikitext.as_bytes());

            let mut text_segments = Vec::new();
            while let Some((mat, capture_index)) = captures.next() {
                let capture = mat.captures[*capture_index];
                let capture_name = &self.text_query.capture_names()[capture.index as usize];
                let node_text = get_node_text(capture.node, wikitext);
                if *capture_name == "text" {
                    text_segments.push(TextSegment {
                        text: node_text,
                        range: capture.node.range(),
                    });
                }
            }
            Ok(text_segments)
        } else {
            Err("Parse error")
        }
    }
}

fn get_node_text(node: Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}

impl TextSegment {
    pub fn one_grams(&self) -> Vec<String> {
        self.text
            .split_whitespace()
            .map(|word| {
                // Remove punctuation from the beginning and end of words
                word.trim_matches(|c: char| c.is_ascii_punctuation())
                    .to_lowercase()
            })
            .filter(|word| {
                !word.is_empty() 
                    && word.len() > 1  // Remove single letter words
                    && !word.parse::<f64>().is_ok()  // Remove words that are numbers (including decimals)
            })
            .collect()
    }

    pub fn bigrams(&self) -> Vec<String> {
        let words = self.one_grams();
        let mut bigrams = Vec::new();

        for i in 0..words.len().saturating_sub(1) {
            bigrams.push(format!("{} {}", words[i], words[i + 1]));
        }

        bigrams
    }

    pub fn trigrams(&self) -> Vec<String> {
        let words = self.one_grams();
        let mut trigrams = Vec::new();

        for i in 0..words.len().saturating_sub(2) {
            trigrams.push(format!("{} {} {}", words[i], words[i + 1], words[i + 2]));
        }

        trigrams
    }

    pub fn link_candidates(&self) -> Vec<String> {
        let mut candidates = Vec::new();
        candidates.extend(self.one_grams());
        candidates.extend(self.bigrams());
        candidates.extend(self.trigrams());
        candidates
    }
}
