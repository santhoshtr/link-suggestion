use actix_web::{HttpResponse, Responder, get, web};
use handlebars::Handlebars;
use linksuggestions::process_links_command;
use serde::{Deserialize, Serialize};
use serde_json::json;

// Query parameters
#[derive(Deserialize, Serialize)]
pub struct LookupParams {
    pub confidence_score: Option<f32>,
}

// API response types
#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

// API endpoints
#[get("/api/suggest_links/{language}.wikipedia.org/wiki/{title}")]
pub async fn suggest_links_api(
    path: web::Path<(String, String)>,
    query: web::Query<LookupParams>,
) -> impl Responder {
    let (language, title) = path.into_inner();
    let mut conf_score = 0.5;
    if let Some(confidence_score) = query.confidence_score {
        conf_score = confidence_score;
    }

    match process_links_command(language.as_str(), title.as_str(), conf_score).await {
        Ok(results) => HttpResponse::Ok().json(ApiResponse {
            success: true,
            data: Some(results),
            error: None,
        }),
        Err(err) => HttpResponse::InternalServerError().json(ApiResponse::<()> {
            success: false,
            data: None,
            error: Some(err.to_string()),
        }),
    }
}

#[get("/{language}.wikipedia.org/wiki/{title}")]
async fn suggestions_view(
    path: web::Path<(String, String)>,
    query: web::Query<LookupParams>,
) -> HttpResponse {
    let (language, title) = path.into_inner();
    let mut templatedata = json!({
        "title": "Wiki Link Suggestion",
        "language": language,
        "title":title,
        "confidence_score": 0.5
    });
    let mut conf_score = 0.5;
    if let Some(confidence_score) = query.confidence_score {
        conf_score = confidence_score;
    }
    templatedata["confidence_score"] = serde_json::to_value(conf_score).unwrap_or_default();
    let mut handlebars: Handlebars<'_> = Handlebars::new();
    handlebars.register_escape_fn(handlebars::no_escape);
    handlebars
        .register_template_file("template", "./templates/index.hbs")
        .unwrap();
    handlebars
        .register_template_file("header", "./templates/partials/header.hbs")
        .unwrap();
    let rendered = handlebars.render("template", &templatedata).unwrap();
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

#[get("/")]
async fn index() -> HttpResponse {
    let templatedata = json!({
            "language": "simple",
            "title":"Oxygen",
            "confidence_score": 0.5
    });

    let mut handlebars: Handlebars<'_> = Handlebars::new();
    handlebars.register_escape_fn(handlebars::no_escape);
    handlebars
        .register_template_file("template", "./templates/index.hbs")
        .unwrap();
    handlebars
        .register_template_file("header", "./templates/partials/header.hbs")
        .unwrap();
    let rendered = handlebars.render("template", &templatedata).unwrap();
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

#[get("/robots.txt")]
async fn robots_txt() -> HttpResponse {
    let content = r#"
User-agent: *
Disallow: /"#;

    HttpResponse::Ok().content_type("text/plain").body(content)
}
