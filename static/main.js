function fetch_suggestions(event) {
	event?.preventDefault();
	const language = document.getElementById("language").value;
	const title = document.getElementById("title").value;
	const confidenceScore = document.getElementById("confidence_score").value;
	// Construct URL in the required format
	const url = `/${language}.wikipedia.org/wiki/${title}?confidence_score=${confidenceScore}`;
	// Navigate to the constructed URL
	history.pushState(null, "", url);

	// Fetch the data from the API
	fetch(
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
					responseObj.data.new_wikitext || "";
				highlightLinks(responseObj.data.suggestions);
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

	if (!wikitextElement || !wikitextElement.firstChild) {
		console.warn("Wikitext element or content not found");
		return;
	}

	const textContent = wikitextElement.textContent;

	// Process each suggestion
	if (suggestions && Array.isArray(suggestions)) {
		suggestions.forEach((suggestion) => {
			try {
				// Get the link text to highlight
				const linkText = suggestion.link_text;

				if (!linkText) return;

				// Find first occurrences of the linkText in the wikitext
				let textIndex = 0;
				let startIndex = textContent
					.toLowerCase()
					.indexOf(linkText.toLowerCase());

				if (startIndex < 0) {
					return;
				}
				const endIndex = startIndex + linkText.length;
				// Create a range for this occurrence
				const range = new Range();
				range.setStart(wikitextElement.firstChild, startIndex);
				range.setEnd(wikitextElement.firstChild, endIndex);

				// Add the range to our highlight
				h.add(range);

				// Move past this occurrence for the next search
				textIndex = endIndex;
			} catch (error) {
				console.error("Error highlighting suggestion:", error, suggestion);
			}
		});
	}

	// Register the highlight into the registry
	// This makes the ::highlight() CSS work
	CSS.highlights.set("suggestedlink", h);
}
document.addEventListener("DOMContentLoaded", function () {
	fetch_suggestions();
});
