use redb::{Database, ReadableTable, TableDefinition};
use std::sync::Arc;

pub type Db = Arc<Database>;

const HITS: TableDefinition<&str, u64> = TableDefinition::new("hits");

pub fn open(path: &str) -> Db {
    let db_path = std::path::Path::new(path);
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).unwrap();
        }
    }
    let db = Database::create(db_path).unwrap();
    let write_txn = db.begin_write().unwrap();
    {
        let _ = write_txn.open_table(HITS).unwrap();
    }
    write_txn.commit().unwrap();
    Arc::new(db)
}

pub fn increment(db: &Database, slug: &str) {
    let write_txn = db.begin_write().unwrap();
    {
        let mut table = write_txn.open_table(HITS).unwrap();
        let val = table
            .get(slug)
            .unwrap()
            .map(|guard| guard.value())
            .unwrap_or(0);
        table.insert(slug, val.saturating_add(1)).unwrap();
    }
    write_txn.commit().unwrap();
}

pub fn get_hits(db: &Database, slug: &str) -> u64 {
    let read_txn = db.begin_read().unwrap();
    let table = read_txn.open_table(HITS).unwrap();
    table
        .get(slug)
        .unwrap()
        .map(|guard| guard.value())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_persists() {
        let dir = std::env::temp_dir().join(format!("blog-redb-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("counter.redb").to_string_lossy().into_owned();
        let db = open(&path);
        assert_eq!(get_hits(&db, "hello"), 0);
        increment(&db, "hello");
        increment(&db, "hello");
        increment(&db, "other");
        assert_eq!(get_hits(&db, "hello"), 2);
        assert_eq!(get_hits(&db, "other"), 1);
        drop(db);
        let db2 = open(&path);
        assert_eq!(get_hits(&db2, "hello"), 2);
        drop(db2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
