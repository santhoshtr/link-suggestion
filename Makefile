# =========================================================
# NOTE: This Makefile is optimized for parallel execution.
# Run with `make -j` to use all CPU cores.
# =========================================================

init:
	cargo build --profile=release

# Output directories, created on demand. Used as order-only prerequisites (after
# the `|`) so a directory's changing mtime never forces a target to rebuild.
data data/titles data/bloom data/anchor-dictionaries:
	mkdir -p $@

data/wikipedia.list: | data
	curl -s https://noc.wikimedia.org/conf/dblists/closed.dblist > data/closed.dblist
	curl -s https://noc.wikimedia.org/conf/dblists/wikipedia.dblist | grep -E 'wiki$$' | grep -v '^#' | grep -v -f data/closed.dblist > $@
	sed -i '/^arbcom/d' $@
	sed -i '/^sysop/d' $@
	sed -i '/^wg_en/d' $@
	sed -i '/^cebwiki/d' $@
	sed -i '/^warwiki/d' $@
	sed -i '/^be_x_old/d' $@
	rm data/closed.dblist

data/titles/%.titles.list: data/anchor-dictionaries/%.sqlite | data/titles
	./target/release/sqlite-cli --database $< --no-header --query "SELECT DISTINCT link_title FROM links" > $@

data/titles/%.labels.list: data/anchor-dictionaries/%.sqlite | data/titles
	./target/release/sqlite-cli --database $< --no-header --query "SELECT DISTINCT link_label FROM links" > $@

data/bloom/%.bloom: data/titles/%.titles.list | data/bloom
	./target/release/linksuggestion-bloom build -i data/titles/$*.titles.list -o $@

data/bloom/%.labels.bloom: data/anchor-dictionaries/%.sqlite data/titles/%.labels.list | data/bloom
	./target/release/linksuggestion-bloom build -i data/titles/$*.labels.list -o $@

DUMP_BASE_URL := https://dumps.wikimedia.org

data/anchor-dictionaries/%.sqlite: | data/anchor-dictionaries
	./target/release/anchor-dictionary \
		--language $* \
		--input $(DUMP_BASE_URL)/$*/latest/$*-latest-pages-articles.xml.bz2 \
		--format sqlite \
	  --output $@

WIKIS := $(shell cat data/wikipedia.list 2>/dev/null)
WIKI_TARGETS := $(addprefix data/titles/,$(addsuffix .titles.list,$(WIKIS)))
WIKI_BLOOM_TARGETS := $(addprefix data/bloom/, $(addsuffix .bloom,$(WIKIS)))
WIKI_BLOOM_LABELS_TARGETS := $(addprefix data/bloom/, $(addsuffix .labels.bloom,$(WIKIS)))
WIKI_ANCHOR_DICTIONARIES := $(addprefix data/anchor-dictionaries/, $(addsuffix .sqlite,$(WIKIS)))

# Per-wiki convenience target: `make simplewiki` runs the full data prep
# (sqlite + title and label bloom filters) for that one wiki. Defined for every
# wiki in data/wikipedia.list.
$(WIKIS): %: data/bloom/%.bloom data/bloom/%.labels.bloom

# Delete a target if its recipe fails, so an interrupted or failed build never
# leaves a partial file that make would then treat as up to date (the .sqlite
# rule has no prerequisites, so a stale empty file would never be rebuilt).
.DELETE_ON_ERROR:

# Keep the intermediate title/label list files instead of auto-deleting them
# (make would otherwise treat *.labels.list as a throwaway intermediate).
.SECONDARY:

.PHONY: titles bloom bloom-labels anchor-dictionaries $(WIKIS)
titles: $(WIKI_TARGETS)
bloom: $(WIKI_BLOOM_TARGETS)
bloom-labels: $(WIKI_BLOOM_LABELS_TARGETS)
anchor-dictionaries: $(WIKI_ANCHOR_DICTIONARIES)
