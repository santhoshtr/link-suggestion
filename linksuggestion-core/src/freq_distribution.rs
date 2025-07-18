use std::collections::HashMap;

use serde::Serialize;

use crate::database::get_db_connection;

#[derive(Serialize)]
pub struct FreqDistribution {
    categories: Vec<String>,
    // The count of items within each category/bin.
    data: Vec<f32>,
}

// The core logic for querying and processing the data.
pub fn generate_chart_data(language: &str) -> Result<FreqDistribution, rusqlite::Error> {
    let conn = get_db_connection(language);

    let mut stmt = conn.prepare(
        "select  count(link_title) as freq from links GROUP by link_title ORDER by freq desc",
    )?;

    let freqs = stmt.query_map([], |row| row.get(0))?;

    let mut bins: HashMap<String, f32> = HashMap::new();

    let mut rank = 1;
    for freq_result in freqs {
        let freq: u32 = freq_result?;
        let normalized_rank = (rank as f32).log10();
        let normalized_freq = (freq as f32).log10();
        let rounded = format!("{normalized_rank:.2}");
        bins.insert(rounded, normalized_freq);
        rank += 1;
    }

    // change to sort the categories by their numeric key. AI?
    let mut sorted_categories: Vec<String> = bins.keys().cloned().collect();
    sorted_categories.sort_by(|a, b| {
        let a_val: f32 = a.parse().unwrap_or(0.0);
        let b_val: f32 = b.parse().unwrap_or(0.0);
        a_val
            .partial_cmp(&b_val)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Create the data vector in the same order as the sorted categories
    let sorted_data: Vec<f32> = sorted_categories
        .iter()
        .map(|category| *bins.get(category).unwrap_or(&0.0))
        .collect();

    Ok(FreqDistribution {
        categories: sorted_categories,
        data: sorted_data,
    })
}
