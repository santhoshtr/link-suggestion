#!/bin/bash
# After:
# become linker
LANGUAGES=(simple ml fi ta hi zu ary zea lo pa lb azb min ab co)
REMOTE_BASE="https://analytics.wikimedia.org/published/datasets/one-off/santhosh/link-suggestion"

# Function to handle Ctrl+C
cleanup_function() {
	echo -e "\nCtrl+C detected! Exiting gracefully."
	# Perform any necessary cleanup here
	exit 1 # Exit with a non-zero status to indicate an abnormal exit
}
# Set the trap for SIGINT (Ctrl+C)
trap cleanup_function SIGINT

rm -rf data/bloom data/anchor-dictionaries

for lang in "${LANGUAGES[@]}"; do
	# Create the local directories if they don't exist
	mkdir -p "data/bloom"
	mkdir -p "data/anchor-dictionaries"

	# Download bloom files
	wget "${REMOTE_BASE}/bloom/${lang}wiki.bloom" -P "data/bloom/"
	wget "${REMOTE_BASE}/bloom/${lang}wiki.labels.bloom" -P "data/bloom/"

	# Download anchor-dictionaries files
	wget "${REMOTE_BASE}/anchor-dictionaries/${lang}wiki.sqlite" -P "data/anchor-dictionaries/"
done

toolforge build start https://gitlab.wikimedia.org/toolforge-repos/linker
# You can check the status of the build like this:
# toolforge build show
toolforge webservice buildservice stop
toolforge webservice buildservice start --cpu 3 --mem 2048Mi --mount=all
# To see the logs for your web service, use:
# toolforge webservice buildservice logs -f
