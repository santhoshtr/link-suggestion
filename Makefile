# =========================================================
# NOTE: This Makefile is optimized for parallel execution.
# Run with `make -j` to use all CPU cores.
# =========================================================

wikipedia.list:
	curl -s https://noc.wikimedia.org/conf/dblists/closed.dblist > closed.dblist 
	curl -s https://noc.wikimedia.org/conf/dblists/wikipedia.dblist | grep -E 'wiki$$' | grep -v '^#' | grep -v -f closed.dblist > $@
	sed -i '/^arbcom/d' $@
	sed -i '/^sysop/d' $@
	sed -i '/^wg_en/d' $@
	sed -i '/^cebwiki/d' $@
	sed -i '/^warwiki/d' $@
	sed -i '/^be_x_old/d' $@
	rm closed.dblist

init:
	cargo build --profile=release
	mkdir -p data/titles
	mkdir -p data/bloom
	mkdir -p data/anchor-dictionaries

data/titles/%.titles.list: data/anchor-dictionaries/%.sqlite
	./target/release/sqlite-cli --database $< --no-header --query "SELECT DISTINCT link_title FROM links" > $@

data/titles/%.labels.list: data/anchor-dictionaries/%.sqlite
	./target/release/sqlite-cli --database $< --no-header --query "SELECT DISTINCT link_label FROM links" > $@

data/bloom/%.bloom: data/titles/%.titles.list
	./target/release/linksuggestion-bloom build -i data/titles/$*.titles.list -o $@

data/bloom/%.labels.bloom: data/anchor-dictionaries/%.sqlite data/titles/%.labels.list
	./target/release/linksuggestion-bloom build -i data/titles/$*.labels.list -o $@

data/anchor-dictionaries/%.sqlite:
	./target/release/anchor-dictionary \
		--language $* \
		--input /mnt/data/xmldatadumps/public/$*/latest/$*-latest-pages-articles.xml.bz2 \
		--format sqlite \
	  --output $@

WIKIS := $(shell cat wikipedia.list)
WIKI_TARGETS := $(addprefix data/titles/,$(addsuffix .titles.list,$(WIKIS)))
WIKI_BLOOM_TARGETS := $(addprefix data/bloom/, $(addsuffix .bloom,$(WIKIS)))
WIKI_BLOOM_LABELS_TARGETS := $(addprefix data/bloom/, $(addsuffix .labels.bloom,$(WIKIS)))
WIKI_ANCHOR_DICTIONARIES := $(addprefix data/anchor-dictionaries/, $(addsuffix .sqlite,$(WIKIS)))

.PHONY: titles bloom anchor-dictionaries
titles: $(WIKI_TARGETS)
bloom: $(WIKI_BLOOM_TARGETS)
bloom-labels: $(WIKI_BLOOM_LABELS_TARGETS)
anchor-dictionaries: $(WIKI_ANCHOR_DICTIONARIES)
