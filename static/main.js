let suggestions;
async function fetch_suggestions(event) {
	event?.preventDefault();
	const language = document.getElementById("language").value;
	const title = document.getElementById("title").value;
	const confidenceScore = document.getElementById("confidence_score").value;
	const url = `/${language}.wikipedia.org/wiki/${title}?confidence_score=0.2`;
	// Navigate to the constructed URL
	history.pushState(null, "", url);

	// Fetch the data from the API
	return fetch(
		`/api/suggest_links/${language}.wikipedia.org/wiki/${title}?confidence_score=${confidenceScore}`,
	)
		.then((response) => {
			if (!response.ok) {
				throw new Error("Network response was not ok");
			}
			return response.json();
		})
		.then((responseObj) => {
			if (responseObj.success && responseObj.data) {
				// Update the article section with the new wikitext
				document.querySelector("article pre").textContent =
					responseObj.data.original_wikitext || "";
				suggestions = responseObj.data.suggestions;
				highlightLinks(suggestions);
				return suggestions;
			} else {
				// Show error message
				document.querySelector("article pre").textContent =
					"Error: " + (responseObj.error || "Unknown error occurred");
			}
		})
		.catch((error) => {
			console.error("Error fetching data:", error);
			document.querySelector("article pre").textContent =
				"Error fetching data: " + error.message;
		});
}

function clearHighlights() {
	if (CSS.highlights) {
		CSS.highlights.delete("suggestedlink");
	}
}

function highlightLinks(suggestions) {
	// Clear any existing highlights first
	clearHighlights();

	// Make sure CSS.highlights API is available
	if (!window.CSS || !CSS.highlights) {
		console.warn("CSS Custom Highlight API not supported in this browser");
		return;
	}

	// Create Highlight
	const h = new Highlight();
	const wikitextElement = document.getElementById("wikitext");
	const confidence_score = document.getElementById("confidence_score").value;
	if (!wikitextElement || !wikitextElement.firstChild) {
		console.warn("Wikitext element or content not found");
		return;
	}

	// Process each suggestion
	if (suggestions && Array.isArray(suggestions)) {
		suggestions.forEach((suggestion) => {
			try {
				// Get the link text to highlight
				const linkText = suggestion.label;

				if (!linkText) {
					return;
				}
				if (suggestion.confidence_score < confidence_score) {
					return;
				}
				// Find first occurrences of the linkText in the wikitext
				let textIndex = 0;
				// Create a range for this occurrence
				const range = new Range();
				range.setStart(
					wikitextElement.firstChild,
					suggestion.char_offset_start,
				);
				range.setEnd(wikitextElement.firstChild, suggestion.char_offset_end);

				// Add the range to our highlight
				h.add(range);
			} catch (error) {
				console.error("Error highlighting suggestion:", error, suggestion);
			}
		});
	}

	// Register the highlight into the registry
	// This makes the ::highlight() CSS work
	CSS.highlights.set("suggestedlink", h);
}

function find_suggestion_in_offset(suggestions, offset) {
	// Find a suggestion in suggestions where the offset is inside the char_offset_start..char_offset_end range.
	if (!suggestions || !suggestions.length) {
		return null;
	}
	return suggestions.find((suggestion) => {
		return (
			offset >= suggestion.char_offset_start &&
			offset <= suggestion.char_offset_end
		);
	});
}
function show_suggestion(suggestion) {
	const container = document.getElementById("preview");
	container.innerHTML = "";
	const wiki_article_element = document.createElement("wiki-article");
	wiki_article_element.language = suggestion.title.language;
	wiki_article_element.article = suggestion.title.normalized;
	wiki_article_element.layout = "compact";
	container.append(wiki_article_element);
	const confidence_score_el = document.createElement("div");
	confidence_score_el.innerText = `Confidence score: ${suggestion.confidence_score}`;
	container.append(confidence_score_el);
	const frequency_el = document.createElement("div");
	frequency_el.innerText = `Linked ${suggestion.frequency} times in ${suggestion.language} wikipedia`;
	container.append(frequency_el);
	container.style.display = "block";
}
document.addEventListener("DOMContentLoaded", async function () {
	suggestions = await fetch_suggestions();
	document
		.getElementById("suggestionForm")
		.addEventListener("submit", async function (event) {
			suggestions = await fetch_suggestions(event);
			event.preventDefault();
			return false;
		});

	// Register confidence score change handler
	document
		.getElementById("confidence_score")
		.addEventListener("change", async function (event) {
			if (suggestions) {
				clearHighlights();
				highlightLinks(suggestions);
			}
		});

	// Click handler for links
	document
		.getElementById("wikitext")
		.addEventListener("click", function (event) {
			const selection = window.getSelection();
			if (selection.focusNode && this.contains(selection.focusNode)) {
				const charOffset = selection.focusOffset;
				const focussed_suggestion = find_suggestion_in_offset(
					suggestions,
					charOffset,
				);
				// Show suggestion
				if (focussed_suggestion) {
					show_suggestion(focussed_suggestion);
				} else {
					const container = document.getElementById("preview");
					container.style.display = "none";
				}
			}
		});
});
