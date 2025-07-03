# Query to find all page titles
PAGEQUERY := "select page_title from page where page_namespace=0 and page_is_redirect = 0"
wikipedia.list:
	curl -s https://noc.wikimedia.org/conf/dblists/closed.dblist > closed.dblist 
	curl -s https://noc.wikimedia.org/conf/dblists/wikipedia.dblist | grep -E 'wiki$$' | grep -v '^#' | grep -v -f closed.dblist > $@
	sed -i '/^arbcom/d' $@
	rm closed.dblist

# Create required directories
init:
	cargo build --profile=release 
	mkdir -p titles
	mkdir -p bloom

# Pattern rule for generating .titles.list files
titles/%.titles.list: wikipedia.list | init
	echo $(PAGEQUERY) | analytics-mysql $* > $@

bloom/%.bloom: titles/%.titles.list
	./target/release/bloom-wiki build -i titles/$*.titles.list -o $@

# Generate all targets based on wikipedia.list content
WIKIS := $(shell if [ -f wikipedia.list ]; then cat wikipedia.list; fi)
WIKI_TARGETS := $(addprefix titles/,$(addsuffix .titles.list,$(WIKIS)))
WIKI_BLOOM_TARGETS := $(addprefix bloom/, $(addsuffix .bloom,$(WIKIS)))

clean:
	rm -rf bloom titles *.list

.PHONY: all-titles all-bloom clean
all-titles: wikipedia.list $(WIKI_TARGETS)
all-bloom: $(WIKI_BLOOM_TARGETS)
