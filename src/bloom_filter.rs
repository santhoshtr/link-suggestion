use bloomfilter::Bloom;
use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

/// A wrapper around the Bloom filter that provides high-level operations
/// for building filters from files and checking words.
pub struct BloomFilterManager {
    bloom: Bloom<str>,
}

impl BloomFilterManager {
    /// Builds a Bloom filter from the lines of an input file.
    /// The capacity of the Bloom filter is determined by the number of lines in the input file.
    ///
    /// # Arguments
    /// * `input_file` - Path to the file containing words, one per line.
    /// * `false_positive_probability` - The desired false positive rate (e.g., 0.01 for 1%).
    ///
    /// # Returns
    /// A new `BloomFilterManager` instance containing the built filter.
    pub fn build_from_file(
        input_file: &PathBuf,
        false_positive_probability: f64,
    ) -> io::Result<Self> {
        // First pass: count lines to determine capacity
        let file = File::open(input_file)?;
        let reader = io::BufReader::new(file);
        let mut capacity = 0;
        for _line in reader.lines() {
            capacity += 1;
        }

        if capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Input file is empty or contains only blank lines. Cannot build a filter with 0 capacity.",
            ));
        }

        println!(
            "Building Bloom filter with calculated capacity {} and false positive probability {}",
            capacity, false_positive_probability
        );

        // Create a new Bloom filter with the calculated capacity and false positive probability.
        let mut bloom = Bloom::new_for_fp_rate(capacity, false_positive_probability).unwrap();

        // Second pass: add lines to the Bloom filter
        let file = File::open(input_file)?;
        let reader = io::BufReader::new(file);

        let mut added_count = 0;
        // Iterate over each line in the input file.
        for line in reader.lines() {
            let line = line?; // Unwrap the Result<String, Error> for each line.
            let trimmed_line = line.trim(); // Remove leading/trailing whitespace.
            if !trimmed_line.is_empty() {
                bloom.set(trimmed_line); // Add the trimmed line to the Bloom filter.
                added_count += 1;
            }
        }
        println!("Added {} unique lines to the Bloom filter.", added_count);

        Ok(BloomFilterManager { bloom })
    }

    /// Loads a Bloom filter from a serialized file.
    ///
    /// # Arguments
    /// * `filter_file` - Path to the serialized Bloom filter file.
    ///
    /// # Returns
    /// A new `BloomFilterManager` instance containing the loaded filter.
    pub fn load_from_file(filter_file: &PathBuf) -> io::Result<Self> {
        println!("Loading Bloom filter from {:?}", filter_file);

        // Open the serialized Bloom filter file.
        let mut file = File::open(filter_file)?;
        // Read the entire content of the file into a byte vector.
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        // Reconstruct the Bloom filter from the raw bytes.
        let bloom = Bloom::from_slice(&bytes).map_err(|e| {
            std::io::Error::other(format!(
                "Failed to reconstruct Bloom filter from bytes: {}",
                e
            ))
        })?;

        Ok(BloomFilterManager { bloom })
    }

    /// Saves the Bloom filter to a file.
    ///
    /// # Arguments
    /// * `output_filter` - Path where the serialized Bloom filter will be saved.
    pub fn save_to_file(&self, output_filter: &PathBuf) -> io::Result<()> {
        // Write the Bloom filter slice directly to the file.
        let mut file = File::create(output_filter)?;
        file.write_all(self.bloom.as_slice())?; // Write the raw bytes to the file.
        Ok(())
    }

    /// Checks if a given word is present in the Bloom filter.
    ///
    /// # Arguments
    /// * `word` - The word to check.
    ///
    /// # Returns
    /// `true` if the word is probably in the filter, `false` if it's definitely not.
    pub fn check_word(&self, word: &str) -> bool {
        self.bloom.check(word)
    }

    /// Checks if a given word is present in the Bloom filter and prints the result.
    ///
    /// # Arguments
    /// * `word` - The word to check.
    pub fn check_word_with_output(&self, word: &str) {
        println!("Checking for word: \"{}\"", word);

        // Check if the word is potentially in the Bloom filter.
        if self.check_word(word) {
            println!(
                "The word \"{}\" is PROBABLY in the filter (due to false positives, this is not 100% certain).",
                word
            );
        } else {
            println!("The word \"{}\" is DEFINITELY NOT in the filter.", word);
        }
    }
}
