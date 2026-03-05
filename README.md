# Link Suggestion

Given a Wikipedia article, suggest possible text segments that can be converted to links to existing Wikipedia articles in the same language.

This is not a machine learning-based approach. Instead, it uses anchor dictionaries built from existing links in Wikipedia and statistical distribution of links across the wiki to calculate confidence scores.

## Sub-crates

- **linksuggestion-core**: Core library with link suggestion algorithms, database handling, and wikitext parsing
- **linksuggestion-web**: HTTP API server for serving link suggestions
- **linksuggestion-bloom**: Bloom filter implementation for fast word lookups
- **anchor-dictionary**: CLI tool for building anchor dictionaries from Wikipedia XML dumps

## Architecture

See [ARCHITECTURE.md](./ARCHITECTURE.md) for detailed documentation.

## API Example

```bash
curl "http://localhost:8080/api/suggest?lang=en&title=Radiation"
```

## License

MIT
