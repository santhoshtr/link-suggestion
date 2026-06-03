use rusqlite::{Connection, OpenFlags};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    // One SQLite connection per language, per thread. rusqlite::Connection is
    // !Sync, so the recommended pattern is a connection confined to a single
    // thread. actix workers and the rayon pool are long-lived, so each thread
    // opens a connection once and reuses it for the rest of its life.
    static CONNECTIONS: RefCell<HashMap<String, Connection>> = RefCell::new(HashMap::new());
}

fn open_connection(language: &str) -> Connection {
    let data_dir = std::env::var("TOOL_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let db_path = format!("{data_dir}/anchor-dictionaries/{language}wiki.sqlite");
    // Read-only: the data files are generated offline and we only run SELECTs.
    // NO_MUTEX is safe because each connection stays on its owning thread.
    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap_or_else(|_| panic!("Error opening database {db_path}"));
    // cache_size is per connection and multiplies across threads × languages,
    // so keep it modest; mmap_size is page-cache backed and shared by the OS.
    conn.execute_batch("PRAGMA cache_size = -8000; PRAGMA mmap_size = 268435456;")
        .unwrap_or_else(|_| panic!("Error setting PRAGMAs on {db_path}"));
    conn
}

/// Runs `f` with the calling thread's connection for `language`, opening it on
/// first use. The connection reference never escapes the closure, which keeps
/// the thread-local borrow sound.
pub fn with_connection<R>(language: &str, f: impl FnOnce(&Connection) -> R) -> R {
    CONNECTIONS.with(|cell| {
        let mut map = cell.borrow_mut();
        // contains_key + get (not entry) so a cache hit allocates no String.
        if !map.contains_key(language) {
            map.insert(language.to_string(), open_connection(language));
        }
        f(map.get(language).unwrap())
    })
}
