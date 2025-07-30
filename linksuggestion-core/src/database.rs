use rusqlite::Connection;

pub fn get_db_connection(language: &str) -> Connection {
    let data_dir = std::env::var("TOOL_DATA_DIR").unwrap_or_else(|_| ".".to_string());
    let db_path = format!("{data_dir}/anchor-dictionaries/{language}wiki.sqlite");
    Connection::open(&db_path).unwrap_or_else(|_| panic!("Error opening database {db_path}"))
}
