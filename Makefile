# =========================================================
# NOTE: This Makefile is optimized for parallel execution.
# Run with `make -j` to use all CPU cores.
# =========================================================

# Query to find all page titles
PAGEQUERY := "select page_title from page where page_namespace=0 and page_is_redirect = 0"
wikipedia.list:
	curl -s https://noc.wikimedia.org/conf/dblists/closed.dblist > closed.dblist 
	curl -s https://noc.wikimedia.org/conf/dblists/wikipedia.dblist | grep -E 'wiki$$' | grep -v '^#' | grep -v -f closed.dblist > $@
	sed -i '/^arbcom/d' $@
	sed -i '/^sysop/d' $@
	sed -i '/^wg_en/d' $@
	rm closed.dblist

init:
	cargo build --profile=release 
	mkdir -p titles
	mkdir -p bloom
	mkdir -p anchor-dictionaries

titles/%.titles.list:
	echo $(PAGEQUERY) | analytics-mysql $* > $@

titles/%.labels.list: anchor-dictionaries/%.sqlite
	./target/release/sqlite-cli --database $< --query "SELECT DISTINCT link_label FROM links" > $@
	
bloom/%.bloom: titles/%.titles.list
	./target/release/bloom-builder build -i titles/$*.titles.list -o $@

bloom/%.labels.bloom: titles/%.labels.list
	./target/release/bloom-builder build -i titles/$*.labels.list -o $@

/tmp/%.all-articles.xml:
	bunzip2 < /mnt/data/xmldatadumps/public/$*/latest/$*-latest-pages-articles.xml.bz2 > $@

anchor-dictionaries/%.sqlite: /tmp/%.all-articles.xml
	./target/release/anchor-dictionary -i /tmp/$*.all-articles.xml --format sqlite -o $@

WIKIS := $(shell cat wikipedia.list)
WIKI_TARGETS := $(addprefix titles/,$(addsuffix .titles.list,$(WIKIS)))
WIKI_BLOOM_TARGETS := $(addprefix bloom/, $(addsuffix .bloom,$(WIKIS)))
WIKI_BLOOM_LABELS_TARGETS := $(addprefix bloom/, $(addsuffix .labels.bloom,$(WIKIS)))
WIKI_ANCHOR_DICTIONARIES := $(addprefix anchor-dictionaries/, $(addsuffix .sqlite,$(WIKIS)))

clean:
	rm -rf bloom/*.* titles/*.* *.list anchor-dictionaries/*.sqlite

.PHONY: titles bloom anchor-dictionaries clean
titles: $(WIKI_TARGETS)
bloom: $(WIKI_BLOOM_TARGETS) $(WIKI_BLOOM_LABELS_TARGETS)
anchor-dictionaries: $(WIKI_BLOOM_TARGETS)
