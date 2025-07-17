let suggestions;
let ml_suggestions;

async function fetch_ml_suggestions(event) {
	event?.preventDefault();
	const language = document.getElementById("language").value;
	const title = document.getElementById("title").value;
	const url = `https://api.wikimedia.org/service/linkrecommendation/v1/linkrecommendations/wikipedia/${language}/${title}`;

	// Fetch the data from the API
	return fetch(url)
		.then((response) => {
			if (!response.ok) {
				throw new Error("Network response was not ok");
			}
			return response.json();
		})
		.then((responseObj) => {
			return responseObj.links;
		})
		.catch((error) => {
			console.error("Error fetching data:", error);
			document.querySelector("article pre").textContent =
				"Error fetching data: " + error.message;
		});
}

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
					responseObj.data.wikitext || "";
				suggestions = responseObj.data.suggestions;
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
		CSS.highlights.delete("suggestedlinkml");
	}
}

function highlightLinks(suggestions, ml_suggestions) {
	// Clear any existing highlights first
	clearHighlights();

	// Make sure CSS.highlights API is available
	if (!window.CSS || !CSS.highlights) {
		console.warn("CSS Custom Highlight API not supported in this browser");
		return;
	}

	// Create Highlight
	const h = new Highlight();
	const h_ml = new Highlight();
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
				const linkText = suggestion.link_text;

				if (!linkText) {
					return;
				}
				if (suggestion.score < confidence_score) {
					return;
				}
				// Find first occurrences of the linkText in the wikitext
				let textIndex = suggestion.wikitext_offset;
				// Create a range for this occurrence
				const range = new Range();
				range.setStart(wikitextElement.firstChild, textIndex);
				range.setEnd(wikitextElement.firstChild, textIndex + linkText.length);

				// Add the range to our highlight
				h.add(range);
			} catch (error) {
				console.error("Error highlighting suggestion:", error, suggestion);
			}
		});
	}
	if (ml_suggestions && Array.isArray(ml_suggestions)) {
		ml_suggestions.forEach((suggestion) => {
			try {
				// Get the link text to highlight
				const linkText = suggestion.link_text;
				if (!linkText) {
					return;
				}
				if (suggestion.score < confidence_score) {
					return;
				}
				// Find first occurrences of the linkText in the wikitext
				let textIndex = suggestion.wikitext_offset;
				// Create a range for this occurrence
				const range = new Range();
				range.setStart(wikitextElement.firstChild, textIndex);
				range.setEnd(wikitextElement.firstChild, textIndex + linkText.length);

				// Add the range to our highlight
				h_ml.add(range);
			} catch (error) {
				console.error("Error highlighting suggestion:", error, suggestion);
			}
		});
	}

	// Register the highlight into the registry
	// This makes the ::highlight() CSS work
	CSS.highlights.set("suggestedlink", h);
	CSS.highlights.set("suggestedlinkml", h_ml);
}

function find_suggestion_in_offset(suggestions, offset) {
	// Find a suggestion in suggestions where the offset is inside the char_offset_start..char_offset_end range.
	if (!suggestions || !suggestions.length) {
		return null;
	}
	return suggestions.find((suggestion) => {
		let textIndex = suggestion.wikitext_offset;
		let textEnd = textIndex + suggestion.link_text.length;
		return offset >= textIndex && offset <= textEnd;
	});
}

async function show_suggestion(suggestion) {
	const container = document.getElementById("preview");
	container.innerHTML = "";
	const language = document.getElementById("language").value;
	const wiki_article_element = document.createElement("wiki-article");
	wiki_article_element.language = language;
	wiki_article_element.article =
		suggestion.title?.normalized || suggestion.link_target;
	wiki_article_element.layout = "compact";
	container.append(wiki_article_element);
	const confidence_score_el = document.createElement("div");
	confidence_score_el.innerText = `Confidence score: ${suggestion.score}`;
	container.append(confidence_score_el);

	if (suggestion.frequency) {
		const frequency_el = document.createElement("div");
		frequency_el.innerText = `Linked ${suggestion.frequency} times in ${language} wikipedia`;
		container.append(frequency_el);
		const distributionButtonEl = document.createElement("button");
		distributionButtonEl.innerText = "Link distribution";
		distributionButtonEl.dataset.language = language;
		container.append(distributionButtonEl);
		await renderDistGraph(language, suggestion);
		distributionButtonEl.addEventListener("click", () => {
			document.getElementById("freq-dist-dialog").showModal();
		});
	}
	container.style.display = "block";
}

async function renderDistGraph(language, suggestion) {
	// Initialize the ECharts instance
	const theme = detectTheme() === "dark" ? "dark" : "light";
	var myChart = echarts.init(document.getElementById("freq-dist"), theme);

	// Show a loading animation while we fetch data
	myChart.showLoading();
	// Make the chart responsive to window resizing
	window.addEventListener("resize", function () {
		myChart.resize();
	});

	// Fetch data from our Rust backend
	return fetch(`/api/linkdistribution/${language}`)
		.then((response) => {
			if (!response.ok) {
				throw new Error("Network response was not ok");
			}
			return response.json();
		})
		.then((serverData) => {
			myChart.hideLoading();

			// Update the chart options with the data from the server
			myChart.setOption({
				title: {
					text: `Link distribution of ${language} wikipedia`,
				},
				tooltip: {
					trigger: "axis",
				},
				xAxis: {
					type: "category",
					name: "Article Rank (Logarithmic scale)",
					nameLocation: "middle",
					nameGap: 50,
					data: serverData.data.categories,
				},
				yAxis: {
					type: "value",
					name: "Link frequency (Logarithmic scale)",
					nameLocation: "middle",
					nameGap: 50,
				},
				series: [
					{
						data: serverData.data.data,
						type: "line",
						markLine: {
							data: [
								{
									yAxis: Math.log10(suggestion.frequency),
									label: {
										formatter: `Title ${suggestion.title.normalized}, linked ${suggestion.frequency} times in ${language} wikipedia`,
										position: "middle",
									},
								},
							],
						},
					},
				],
			});
		})
		.catch((error) => {
			myChart.hideLoading();
			console.error("There was a problem with the fetch operation:", error);
			// Optionally display an error message on the chart itself
			myChart.setOption({
				title: {
					text: "Error!",
					subtext: "Could not load chart data from the server.",
				},
			});
		});
}

document.addEventListener("DOMContentLoaded", async function () {
	// Click handler for links
	document
		.getElementById("wikitext")
		.addEventListener("click", async function (event) {
			const selection = window.getSelection();
			if (selection.focusNode && this.contains(selection.focusNode)) {
				const charOffset = selection.focusOffset;
				const focussed_suggestion = find_suggestion_in_offset(
					suggestions.concat(ml_suggestions),
					charOffset,
				);
				// Show suggestion
				if (focussed_suggestion) {
					await show_suggestion(focussed_suggestion);
				} else {
					const container = document.getElementById("preview");
					container.style.display = "none";
				}
			}
		});
	suggestions = await fetch_suggestions();
	if (suggestions) {
		clearHighlights();
		highlightLinks(suggestions, ml_suggestions || []);
	}
	ml_suggestions = await fetch_ml_suggestions();
	document
		.getElementById("suggestionForm")
		.addEventListener("submit", async function (event) {
			suggestions = await fetch_suggestions(event);
			ml_suggestions = await fetch_ml_suggestions();
			event.preventDefault();
			if (suggestions) {
				clearHighlights();
				highlightLinks(suggestions, ml_suggestions || []);
			}
			return false;
		});

	// Register confidence score change handler
	document
		.getElementById("confidence_score")
		.addEventListener("change", async function (event) {
			if (suggestions) {
				clearHighlights();
				highlightLinks(suggestions, ml_suggestions || []);
			}
		});
	document
		.getElementById("algorithm")
		.addEventListener("change", async function (event) {
			const algorithm = event.target.value;
			if (algorithm == "all") {
				highlightLinks(suggestions, ml_suggestions);
			}
			if (algorithm == "new") {
				clearHighlights();
				highlightLinks(suggestions, []);
			}
			if (algorithm == "ml") {
				clearHighlights();
				highlightLinks([], ml_suggestions);
			}
		});

	if (suggestions) {
		clearHighlights();
		highlightLinks(suggestions, ml_suggestions || []);
	}
});

function detectTheme() {
	// Check for saved theme preference or default to 'auto'
	const savedTheme = localStorage.getItem("theme");

	if (savedTheme) {
		return savedTheme;
	}

	// If no saved preference, check system preference
	if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
		return "dark";
	}

	return "light";
}
