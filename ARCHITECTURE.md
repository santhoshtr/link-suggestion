# Link Suggestion system

This document outlines the architecture of the link suggestions system.

## Introduction

Given a Wikipedia article, we want to find possible text segments in the article that can be converted to a link to existing articles in same wiki. Links help people to learn and discover more content.

- The system should work with all languages where Wikipedia exist.
- The system should respect the conventions about linking in the community. For examples, stop words, numbers, months, continents should not be linked.
- Since this is recommendation system, it is ok if we don't suggest every possible candidates. But, when we suggest something, it is very important that the suggestion is valid.

Constraints set:

- If a text segment was never linked in a wiki, do not suggest that as a candidate. In other words, the text segment will be linked by a human first in a wiki.
- If a text segment is already linked in a wiki article, do not link again.
- Learn from statistical distribution of links in wiki. This will avoid the need for hard-coded no-link rules per wiki.
- The system should be high performant. Links should be suggested for any language and article under a second.
- Should not require pre-calculating(offline calculation) of suggestions.
- The data preparation pipelines should be performant and simple.
- Should not depend on the freshness of the offline data.

## Existing system

See <https://meta.wikimedia.org/wiki/Research:Link_recommendation_model_for_add-a-link_structured_task>

The existing system for link suggestions is a machine learning based approach, developed by research team of Wikimedia foundation and operated by Machine learning team. It is an xgboost based classifier system. There are ongoing efforts to make it work for more languages and avoid having individual models per language.

One of the features for the classifier system is the similarity score between the article under consideration and target article for the link. Semantic similarity of Wikipedia articles are hard problem. Vector embedding of the contents need to created first and then the similarity need to be calculated using techniques like cosine similarity. To calculated vector embedding, we need embedding models and their availability and performance is also a question.

### Machine learning approach

I approached the problem by reading the research about link suggestions and trying to understand the problem modeling. A question I had is, why this problem is probabilistic, and where the need for machine learning comes into the solution.

Following are the features for the classifier model in the existing machine learning based approach.

- ngram: the number of words in the anchor (based on simple tokenization)
- frequency: count of the anchor-link pair in the anchor-dictionary
- ambiguity: how many different candidate links exist for an anchor in the anchor-dictionary
- kurtosis: the kurtosis of the shape of the distribution of candidate-links for a given anchor in the anchor-dictionary
- Levenshtein-distance: the Levenshtein-distance between the anchor and the link. This measures how similar the two strings are. Roughly speaking, it corresponds to the number of single-character edits one has to make to transform one string into another, e.g. the Levenshtein-distance between “kitten” and “sitting” is 3.
- w2v-distance: similarity between the article (source-page) and the link (target-page) based on the content of the pages. This is obtained from wikipedia2vec.

Among this, the w2v-distance require embedding model to prepare vector embedding for the article. We need the embedding computation at the inference stage as well.

The reason for including that feature is the assumption this: "The rationale is that a link might be more likely if the corresponding article is more similar to the source article."

I find it difficult to accept that characterization of links in a Wikipedia article. The outwards links from an article from a politician's article could lead to articles of any category. It could be places, events, geographies, philosophy or anything. The famous Wikipedia rabbit hole concept relies on the fact that from a single wiki article, a casual reader can reach completely unrelated articles and that path takes the reader through a rabbit hole experience. If we confine the outward links closer to the topic of the source article, that is not in alignment with the rabbit hole idea.

To objectively look into this case, let us consider [Rickshaw](https://simple.wikipedia.org/wiki/Rickshaw) article from simple.wikipedia.org. A rickshaw is a kind of vehicle that has two wheels, usually pulled by a human. The outward links in that article are the following:

1. Wheel
2. Vehicle
3. Bicycle
4. Motor
5. Human
6. Japan
7. Bangladesh

As we can easily observe, the articles represented by the target links is not related to Rickshaw. The relation I meant is by vector embedding of article content or Wikipedia article categories or the Topic classification for these articles by [Wikipedia article topic model](https://meta.wikimedia.org/wiki/Machine_learning_models/Production/Language_agnostic_link-based_article_topic).

We can calculate the topics from article topic model as follows:

```bash
curl https://api.wikimedia.org/service/lw/inference/v1/models/outlink-topic-model:predict -X POST -d '{"page_title": "Rickshaw", "lang": "simple"}' -H "Content-type: application/json"
```

```json
{
  "prediction": {
    "article": "https://simple.wikipedia.org/wiki/Rickshaw",
    "results": [
      { "topic": "Geography.Regions.Asia.Asia*", "score": 0.9724247455596924 },
      { "topic": "Culture.Sports", "score": 0.6513648629188538 },
      {
        "topic": "History_and_Society.Transportation",
        "score": 0.546748161315918
      },
      { "topic": "STEM.STEM*", "score": 0.546748161315918 }
    ]
  }
}
```

I would expect the most preferred classification as `Transportation`. The presence of `Culture.Sports` as the second possible topic for Rickshaw is interesting here as Rickshaw is never related to any sports as far as I know.

What we are observing here is consequences of the assumption -The assumption that the outward links in an article are related to the source article. This assumption is the basis for incorporating w2v in feature set for link recommendation. This is also the basis for topic classification model. In topic classification model, the topics of outward links are used as features for classification of source article. That explains why Rickshaw is associated with `Asia` and `Sports`. The STEM topic is present because of `Human` is classified as STEM topic by topic model.

```bash
 curl https://api.wikimedia.org/service/lw/inference/v1/models/outlink-topic-model:predict -X POST -d '{"page_title": "Human", "lang": "simple"}' -H "Content-type: application/json"
```

```json
{
  "prediction": {
    "article": "https://simple.wikipedia.org/wiki/Human",
    "results": [
      { "topic": "STEM.STEM*", "score": 0.9553291201591492 },
      { "topic": "STEM.Biology", "score": 0.59267657995224 }
    ]
  }
}
```

However, It is possible that the largest cluster of topics of outward links relates to source article in many cases. For example, in the case of Rickshaw, the first 4 links- Wheel, Vehicle, Bicycle, Motor are related to the Rickshaw and all are related to Transportation. I would expect this pattern exist for many articles. Because of this, topic classification based on the topics of outward links will practically work in most cases.But it is very important to notice that the reverse relation is not true - that outward links are probable if its topic is related to source article.

For example, If the source article has 20 outward links and 5 links are of topic T1, predicting T1 as the most probable topic for source article may be acceptable. But there are 15 links that has other topics,say, T2, T3,T4, T5, T6. This implies links in source article could be anything from T1..T6 and we cannot give higher preference for links with T1 topic.

Let us take another look at the link suggestions API: Get link suggestions for Mount Everest in simple wikipedia.

```bash
curl 'https://api.wikimedia.org/service/linkrecommendation/v1/linkrecommendations/wikipedia/simple/Mount%20Everest'
```

Gives 3 suggestions

1. New Zealand
2. North pole
3. South pole

All these suggestions are correct. But, this article can be linked to many other articles following the conventions of simple.wikipedia.org. For example, there is a mention of `British people` which is linked 113 times in other articles of simple.wikipedia.org. There is a mention of `human` which is also linked 282 times in simple.wikipedia.org by editors. These suggestions are missing because the algorithm prefer links with similar topics. All the three matches `Geography.Geographical` topic of Mount_Everest.

All this is to say that the similarity of source and target articles is not a feature to consider for this problem. If we remove that from the feature set, all other factors that contribute to the prediction are deterministic features that can be computed relatively easy. However, the statistical distribution of links in a wiki is definitely a contributing factor to prediction. For example, simple.wikipedia.org has practice of linking to words like 'year', 'human', 'heat', 'food', 'water' etc. But this may not be the patters in say, English Wikipedia. Identifying this patterns across all languages will require a statistical model of links.

This learning prompted me to attempt a non-machine learning based approach. I also wanted it very simple and performant.

## Algorithm

### Data preparation

To check whether an arbitrary text segment can point to an article, we need to know if that text segment can correspond to an article. This can be done in many ways:

1. Use a web api like search api to find if a text can match to a title
2. Directly query the mediawiki database table

The first approach, if applied to all possible text segments in an article, will require several API hits and will be very slow. Second approach will also be slow. Additionally, it requires ability to connect to a mediawiki database or replica at runtime, which may not be the case.

Instead of the above approaches, In our system, we build a bloom filter of all titles in a wiki. This is prepared as one time data preparation task. We query a production database to get all titles, and prepare a bloom filter out of it.

[Bloom filters](https://en.wikipedia.org/wiki/Bloom_filter) are compact and extremely fast to tell if a given text segment is present in it or not. If it is not present the result is 100% accurate. If it is present, there is an error margin - false positivity rate, which we can control. In a stat machine, preparing the bloom filter for 342 wikis takes less than a minute when `make -j bloom` command is executed as it parallelize the jobs.

```bash
$ time make  bloom/simplewiki.bloom
echo "select page_title from page where page_namespace=0 and page_is_redirect = 0" | analytics-mysql simplewiki > titles/simplewiki.titles.list
./target/release/bloom-builder build -i titles/simplewiki.titles.list -o bloom/simplewiki.bloom
Building Bloom filter with calculated capacity 271333 and false positive probability 0.001
Added 271333 unique lines to the Bloom filter.
Bloom filter built and saved to "bloom/simplewiki.bloom"

real    0m1.302s
user    0m0.289s
sys     0m0.135s
```

Checking if a title exist or not:

```bash
cargo run --bin bloom-builder -- check  -f bloom/simplewiki.bloom -w Starch
Checking for word: "Starch"
The word "Starch" is PROBABLY in the filter (due to false positives, this is not 100% certain).
```

```bash
cargo run --bin bloom-builder -- check  -f bloom/simplewiki.bloom -w SomeThingThatDoesNotExist
Checking for word: "SomeThingThatDoesNotExist"
cargo run --bin bloom-builder -- check  -f bloom/simplewiki.bloom -w Starch
Checking for word: "SomeThingThatDoesNotExist"
```

You may also notice that this checks are extremely fast. With false positive rate set at 0.001, the size of bloom filter of all titles in Simple wikipedia is 487 kilobytes. Note that it has 271K articles.

With this bloom filter we can check thousands of text segments in fraction of second. There is a 0.001 false positive rate, but we will eliminate them once we shortlist the candidates at later stage.

How often we should update this filter? Suppose a new article is created in simple.wikipedia.org after this filter was created. The filter will tell that article is not present in the filter. And our suggestion system will ignore text segments matching that new title. It is completely acceptable to not suggest a candidate. Additionally, we set a constraint that all the suggestions that we are making are based on link frequency - how many time an article is linked. In the case of new articles, it will take time for editors to start linking to it and meet our frequency thresholds. By that time, we would have updated our filters. Updating this filter once in a month is fair enough.

### Text segments

For a given text, each word in it can be a candidate for link. Phrases consisting of two or more words can also be candidates for linking. For example "United states of America" is a link candidate with 4 words in it. We will extract all combinations of one word, two word, three words, four words.

However, how to find the text runs in a given wikitext content for an article? The wikitext markup for an article will have templates, links,references and such elements. We should be suggesting links only to plain text part of the article. This require parsing the article and identifying ranges of plain texts. We use [tree-sitter-wikitext](https://github.com/santhoshtr/tree-sitter-wikitext/) parser for this purpose. This parser is very fast, error tolerant parser for wikitext and available in C, Go, Rust, JS, Wasm, python environments.We use the rust bindings of tree-sitter-wikitext.

Not all text segment are appropriate for linking. Some communities has their own conventions about linking. For example, years, numbers, stop words, month names, continent names etc are not usually linked in English wikipedia. Hard-coding such rules is one option, but wont scale for all language communities. In our approach, we wont hard code these rules, but we will learn from the frequency distribution of links and get the same conventions in practice.

### Frequency distribution of links

Some articles will be linked a lot in wiki. Some will be linked very rarely. Knowing this pattern accurately will help us to rank and prioritise the suggestions.

To understand this pattern, let us look at simple.wikipedia.org. The title that is most linked in that wiki is [Departments_of_France](https://simple.wikipedia.org/wiki/Departments_of_France) - 23789 times. Communes_of_France, France, United_States(13192), Regions_of_France, Cantons_of_Switzerland, Germany(4491), Italy(4194) etc comes after that. Europe is linked 900 times, Finland linked 486 times,Mathematician - 100 times and Indo-Iranic_languages is linked 3 times and so on. The distribution is non uniform. Our link suggestions should also adhere to this distribution so that it can match the community conventions about linking.

To learn this distribution, we need to collect all links, and their frequency of occurrences. We also need to consider the link label will be different from link title in many cases. We need that information as well.

Preparing this data requires finding all links in all articles in a wiki. Finding all links in an article is also required at inference time(runtime) to avoid linking already linked titles. Here also we will use the tree-sitter-wikitext. Parsing the wikitext dump of a wiki is not an easy task, but tree-sitter-wikitext can handle huge dumps without issues. Parsing ~80 GB wikitext dump of English wikipedia takes about 5 hours. But other wikis can be parsed in minutes. A bottleneck here is the bzip2 compressed dumps as they cannot be decompressed in multi-threaded way. So, we will be confined to a single thread. Our architecture allows to parallelize multiple wiki dump parsing in parallel though. We wont decompress and parse, we will stream the bzip2 file to our link extractor directly.

The output of this parsing is a database in sqlite format per wiki. The above distribution statistics I shared is based on simplewiki.sqlite file(~173MB). For English Wikipedia, this database is about 3.8 GB sqlite file.

### Normalization

## Prediction
