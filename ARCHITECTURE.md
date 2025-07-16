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

## Machine learning or not?

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
