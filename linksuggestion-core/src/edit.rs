// The main function where the program execution begins.
use tree_sitter::{InputEdit, Language, Parser, Point};
/// Calculates the byte positions required to convert the label to a wiki internal link.
///
/// Returns a tuple of (start_byte, end_byte, replacement_text) where:
/// - start_byte: The starting byte position within the text segment where the label begins
/// - end_byte: The ending byte position within the text segment where the label ends
/// - replacement_text: The wiki link format string to replace the label with
///
/// The byte positions are calculated relative to the text segment's start_byte.
pub fn calculate_link_edit_positions(
    text: String,
    label: String,
) -> Option<(usize, usize, usize, String, String)> {
    // Find the label within the text segment
    if let Some(label_start) = text.find(label.as_str()) {
        let label_end = label_start + label.len();

        // Create the wiki link replacement text
        let replacement = format!("[[{label}|{label}]]");
        let label_new_end = label_start + replacement.len();
        let mut new_text = text.clone();
        new_text.replace_range(label_start..label_end, replacement.as_str());
        Some((label_start, label_end, label_new_end, replacement, new_text))
    } else {
        None
    }
}
fn main() {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_wikitext::LANGUAGE.into())
        .expect("Error loading Wikitext grammar");
    let source_code = "\nLink to India should appear here\n";
    let mut tree = parser.parse(source_code, None).unwrap();
    let root_node = tree.root_node();
    let (start, end, new_end, replacement, linked_text) =
        calculate_link_edit_positions(source_code.to_string(), "India".to_string()).unwrap();
    tree.edit(&InputEdit {
        start_byte: start,
        start_position: Point::new(1, start),
        old_end_byte: end,
        old_end_position: Point::new(1, end),
        new_end_byte: new_end,
        new_end_position: Point::new(1, new_end),
    });

    let new_tree = parser.parse(&linked_text, Some(&tree)).unwrap();
    dbg!(linked_text);
    dbg!(new_tree.root_node().to_sexp());
}
