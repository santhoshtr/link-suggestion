use std::io;

mod router;

use actix_files as fs;
use actix_web::{App, HttpServer, middleware::Logger, web};

use router::get_distribution;
use router::index;
use router::robots_txt;
use router::suggest_links_api;
use router::suggestions_view;

// Web server state with SQLite connection
pub struct AppState {}

// Main function to start the server
#[actix_web::main]
async fn main() -> io::Result<()> {
    // Ref https://docs.rs/actix-web/latest/actix_web/middleware/struct.Logger.html
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Create application state with shared database connection
    let app_state = web::Data::new(AppState {});

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8000".to_string())
        .parse::<u16>()
        .unwrap_or(8000);

    println!("Starting server at http://0.0.0.0:{port}");

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(app_state.clone())
            .service(
                fs::Files::new("/static", "static")
                    // .show_files_listing()
                    .use_last_modified(true)
                    .prefer_utf8(true),
            )
            .wrap(
                actix_web::middleware::DefaultHeaders::new()
                    .add(("cache-control", "public, max-age=31536000")),
            )
            .wrap(actix_web::middleware::Compress::default())
            .service(index)
            .service(suggestions_view)
            .service(suggest_links_api)
            .service(get_distribution)
            .service(robots_txt)
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
