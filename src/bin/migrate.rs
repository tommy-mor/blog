//! One-off migration: import per-post view counts from the legacy RocksDB
//! store into the redb counter database.
//!
//! The old counter used keys `hits:<slug>` with 8-byte little-endian u64
//! values accumulated through a merge operator. This tool opens the old
//! RocksDB read-only (safe while the app is running), resolves the merge
//! operands with the same operator the old code used, and writes the totals
//! into the redb file.
//!
//! Usage:
//!   migrate <rocksdb-dir> <redb-file> [--sum] [--cleanup]
//!
//!   <rocksdb-dir>  directory holding the legacy RocksDB files (e.g. ./data-live)
//!   <redb-file>    destination redb file (e.g. ./counter.redb)
//!   --sum          add legacy counts onto existing redb values.
//!                  Default is max(existing, legacy), which is idempotent
//!                  and safe to re-run.
//!   --cleanup      after a successful migration, delete the RocksDB-internal
//!                  files from <rocksdb-dir>. Never touches *.redb or any
//!                  unrecognized files.
//!
//! Fly.io workflow (volume `blog_data` mounted at /app/data):
//!   1. Copy the live data down:
//!        fly ssh sftp get -r /app/data ./data-live
//!   2. Migrate locally (needs clang to build: cargo build --features migrate):
//!        ./target/debug/migrate ./data-live ./counter.redb
//!   3. If the new (redb) build is already running, stop the machine first so
//!      you don't overwrite a live database file, and use --sum against a
//!      copy of the live counter.redb to keep counts gathered since the deploy:
//!        cp ./data-live/counter.redb ./live-counter.redb   # if present
//!        ./target/debug/migrate ./data-live ./counter.redb --sum
//!        fly machine stop <machine-id>
//!        fly ssh sftp put ./counter.redb /app/data/counter.redb
//!        fly machine start <machine-id>
//!      If the old (rocksdb) build is still running, just upload and deploy:
//!        fly ssh sftp put ./counter.redb /app/data/counter.redb
//!        fly deploy
//!   4. Once the new build is serving with correct counts, remove the stale
//!      RocksDB files from the volume (re-run with --cleanup against a fresh
//!      copy, or delete CURRENT / MANIFEST-* / OPTIONS-* / *.sst / *.log /
//!      LOCK / LOG from /app/data via `fly ssh console`, keeping counter.redb).

use redb::{Database, ReadableTable, TableDefinition};
use rocksdb::{IteratorMode, MergeOperands, Options, DB};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const HITS: TableDefinition<&str, u64> = TableDefinition::new("hits");
const PREFIX: &str = "hits:";

/// Must be identical to the merge operator the old counter used, otherwise
/// RocksDB cannot resolve the accumulated merge operands on read and the
/// totals will be wrong.
fn u64_add(_key: &[u8], existing: Option<&[u8]>, operands: &MergeOperands) -> Option<Vec<u8>> {
    let mut val = existing
        .and_then(|v| <[u8; 8]>::try_from(v).ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0);
    for op in operands {
        if let Ok(arr) = <[u8; 8]>::try_from(op) {
            val = val.saturating_add(u64::from_le_bytes(arr));
        }
    }
    Some(val.to_le_bytes().to_vec())
}

/// Parse one RocksDB entry into (slug, count). Returns None for keys outside
/// the `hits:` namespace or malformed values.
fn parse_entry(key: &[u8], value: &[u8]) -> Option<(String, u64)> {
    let key = std::str::from_utf8(key).ok()?;
    let slug = key.strip_prefix(PREFIX)?;
    if slug.is_empty() {
        return None;
    }
    let bytes = <[u8; 8]>::try_from(value).ok()?;
    Some((slug.to_string(), u64::from_le_bytes(bytes)))
}

fn read_rocksdb(path: &Path) -> Result<HashMap<String, u64>, String> {
    let mut opts = Options::default();
    opts.set_merge_operator_associative("u64_add", u64_add);
    let db = DB::open_for_read_only(&opts, path, false)
        .map_err(|e| format!("open rocksdb {}: {e}", path.display()))?;
    let mut counts = HashMap::new();
    for item in db.iterator(IteratorMode::Start) {
        let (k, v) = item.map_err(|e| format!("iterate rocksdb: {e}"))?;
        if let Some((slug, n)) = parse_entry(&k, &v) {
            counts.insert(slug, n);
        }
    }
    Ok(counts)
}

fn write_redb(path: &Path, counts: &HashMap<String, u64>, sum: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create parent dir: {e}"))?;
        }
    }
    let db = Database::create(path).map_err(|e| format!("open redb {}: {e}", path.display()))?;
    let txn = db.begin_write().map_err(|e| format!("begin redb write: {e}"))?;
    {
        let mut table = txn
            .open_table(HITS)
            .map_err(|e| format!("open redb table: {e}"))?;
        let mut slugs: Vec<&String> = counts.keys().collect();
        slugs.sort();
        for slug in slugs {
            let old = counts[slug];
            let existing = table
                .get(slug.as_str())
                .map_err(|e| format!("read redb: {e}"))?
                .map(|g| g.value())
                .unwrap_or(0);
            let new = if sum {
                existing.saturating_add(old)
            } else {
                existing.max(old)
            };
            table
                .insert(slug.as_str(), new)
                .map_err(|e| format!("write redb: {e}"))?;
            println!("{slug}: legacy={old} existing={existing} -> {new}");
        }
    }
    txn.commit().map_err(|e| format!("commit redb: {e}"))?;
    Ok(())
}

/// True for files owned by RocksDB itself. Never matches *.redb or anything
/// unrecognized, so cleanup is conservative by default.
fn is_rocksdb_file(name: &str) -> bool {
    name == "CURRENT"
        || name == "IDENTITY"
        || name == "LOCK"
        || name == "LOG"
        || name.starts_with("MANIFEST-")
        || name.starts_with("OPTIONS-")
        || name.starts_with("LOG.old")
        || name.ends_with(".sst")
        || name.ends_with(".log")
}

fn cleanup_rocksdb(dir: &Path) -> Result<(), String> {
    let mut removed = 0;
    let mut kept = Vec::new();
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("list {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("list {}: {e}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".redb") || !is_rocksdb_file(&name) {
            kept.push(name);
            continue;
        }
        std::fs::remove_file(entry.path())
            .map_err(|e| format!("remove {}: {e}", entry.path().display()))?;
        removed += 1;
    }
    println!("removed {removed} RocksDB file(s)");
    if !kept.is_empty() {
        kept.sort();
        println!("left in place: {}", kept.join(", "));
    }
    Ok(())
}

fn print_usage() {
    eprintln!("usage: migrate <rocksdb-dir> <redb-file> [--sum] [--cleanup]");
}

fn run(rocks: &Path, redb_path: &Path, sum: bool, cleanup: bool) -> Result<(), String> {
    if !rocks.is_dir() {
        return Err(format!("rocksdb dir not found: {}", rocks.display()));
    }
    if redb_path.is_dir() {
        return Err(format!(
            "redb destination is a directory: {}",
            redb_path.display()
        ));
    }
    let counts = read_rocksdb(rocks)?;
    println!("found {} slug(s) in {}", counts.len(), rocks.display());
    write_redb(redb_path, &counts, sum)?;
    println!("wrote {}", redb_path.display());
    if cleanup {
        cleanup_rocksdb(rocks)?;
    }
    Ok(())
}

fn main() {
    let mut positional = Vec::new();
    let mut sum = false;
    let mut cleanup = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--sum" => sum = true,
            "--cleanup" => cleanup = true,
            "-h" | "--help" => {
                print_usage();
                return;
            }
            _ if arg.starts_with('-') => {
                eprintln!("unknown flag: {arg}");
                print_usage();
                std::process::exit(1);
            }
            _ => positional.push(arg),
        }
    }
    if positional.len() != 2 {
        print_usage();
        std::process::exit(1);
    }
    let rocks = PathBuf::from(&positional[0]);
    let redb_path = PathBuf::from(&positional[1]);
    if let Err(e) = run(&rocks, &redb_path, sum, cleanup) {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hits_entries() {
        let (slug, n) =
            parse_entry(b"hits:my-post", &42u64.to_le_bytes()).expect("should parse");
        assert_eq!(slug, "my-post");
        assert_eq!(n, 42);
    }

    #[test]
    fn rejects_non_hits_keys_and_bad_values() {
        assert!(parse_entry(b"other:my-post", &1u64.to_le_bytes()).is_none());
        assert!(parse_entry(b"hits:my-post", &[1, 2, 3]).is_none());
        assert!(parse_entry(b"hits:", &1u64.to_le_bytes()).is_none());
        assert!(parse_entry(&[0xff, 0xfe], &1u64.to_le_bytes()).is_none());
    }

    #[test]
    fn cleanup_matches_only_rocksdb_files() {
        for f in [
            "CURRENT",
            "IDENTITY",
            "LOCK",
            "LOG",
            "LOG.old.123",
            "MANIFEST-000001",
            "OPTIONS-000003",
            "000123.sst",
            "000124.log",
        ] {
            assert!(is_rocksdb_file(f), "{f} should match");
        }
        for f in [
            "counter.redb",
            "counter.redb.lock",
            "posts",
            "data",
            "MANIFEST",
            "OPTIONS",
            "sst",
        ] {
            assert!(!is_rocksdb_file(f), "{f} should not match");
        }
    }
}
