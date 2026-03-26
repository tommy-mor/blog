use rocksdb::{MergeOperands, Options, DB};
use std::sync::Arc;

pub type Db = Arc<DB>;

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

pub fn open(path: &str) -> Db {
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_merge_operator_associative("u64_add", u64_add);
    Arc::new(DB::open(&opts, path).unwrap())
}

pub fn increment(db: &DB, slug: &str) {
    db.merge(format!("hits:{slug}").as_bytes(), &1u64.to_le_bytes())
        .unwrap();
}

pub fn get_hits(db: &DB, slug: &str) -> u64 {
    db.get(format!("hits:{slug}").as_bytes())
        .unwrap()
        .and_then(|v| <[u8; 8]>::try_from(v.as_slice()).ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}
