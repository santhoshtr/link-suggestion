use rusqlite::Connection;

pub fn get_db_connection(language: &str) -> Connection {
    let db_path = format!("anchor-dictionaries/{language}wiki.sqlite");
    Connection::open(&db_path).unwrap_or_else(|_| panic!("Error opening database {db_path}"))
}
