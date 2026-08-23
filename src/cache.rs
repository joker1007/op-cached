use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

struct Entry {
    ciphertext: Vec<u8>,
    inserted_at: Instant,
    /// Source file mtime for `op inject` results; None for `op read` entries.
    mtime: Option<SystemTime>,
}

/// In-memory store of GPG-encrypted values.
/// Keys are namespaced: `url:<op://...>` for reads, `file:<abs path>` for injects.
pub struct Cache {
    map: HashMap<String, Entry>,
    ttl: Duration,
}

impl Cache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            map: HashMap::new(),
            ttl,
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Ciphertext for an `op read` URL if present and not expired.
    pub fn get_url(&mut self, url: &str, now: Instant) -> Option<&[u8]> {
        self.get(&format!("url:{url}"), now, None)
    }

    pub fn insert_url(&mut self, url: &str, ciphertext: Vec<u8>, now: Instant) {
        self.insert(format!("url:{url}"), ciphertext, now, None);
    }

    /// Ciphertext for an `op inject` of `path` if present, not expired, and the
    /// recorded mtime matches `mtime`. A stale entry is removed.
    pub fn get_file(&mut self, path: &str, mtime: SystemTime, now: Instant) -> Option<&[u8]> {
        self.get(&format!("file:{path}"), now, Some(mtime))
    }

    pub fn insert_file(
        &mut self,
        path: &str,
        mtime: SystemTime,
        ciphertext: Vec<u8>,
        now: Instant,
    ) {
        self.insert(format!("file:{path}"), ciphertext, now, Some(mtime));
    }

    fn get(&mut self, key: &str, now: Instant, mtime: Option<SystemTime>) -> Option<&[u8]> {
        let stale = self
            .map
            .get(key)
            .is_some_and(|e| now.duration_since(e.inserted_at) >= self.ttl || e.mtime != mtime);
        if stale {
            self.map.remove(key);
            return None;
        }
        self.map.get(key).map(|e| e.ciphertext.as_slice())
    }

    fn insert(
        &mut self,
        key: String,
        ciphertext: Vec<u8>,
        now: Instant,
        mtime: Option<SystemTime>,
    ) {
        self.map.insert(
            key,
            Entry {
                ciphertext,
                inserted_at: now,
                mtime,
            },
        );
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Drop all expired entries. Returns the number removed.
    pub fn sweep(&mut self, now: Instant) -> usize {
        let before = self.map.len();
        let ttl = self.ttl;
        self.map
            .retain(|_, e| now.duration_since(e.inserted_at) < ttl);
        before - self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn url_hit_before_ttl_miss_after() {
        let mut c = Cache::new(secs(10));
        let t0 = Instant::now();
        c.insert_url("op://v/i/f", b"ct".to_vec(), t0);
        assert_eq!(c.get_url("op://v/i/f", t0 + secs(9)), Some(&b"ct"[..]));
        assert_eq!(c.get_url("op://v/i/f", t0 + secs(10)), None);
        assert_eq!(c.len(), 0, "expired entry is removed on get");
    }

    #[test]
    fn unknown_key_is_miss() {
        let mut c = Cache::new(secs(10));
        assert_eq!(c.get_url("op://x", Instant::now()), None);
    }

    #[test]
    fn insert_overwrites_and_refreshes() {
        let mut c = Cache::new(secs(10));
        let t0 = Instant::now();
        c.insert_url("k", b"a".to_vec(), t0);
        c.insert_url("k", b"b".to_vec(), t0 + secs(5));
        assert_eq!(c.get_url("k", t0 + secs(14)), Some(&b"b"[..]));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn url_and_file_namespaces_do_not_collide() {
        let mut c = Cache::new(secs(10));
        let t0 = Instant::now();
        let m = SystemTime::UNIX_EPOCH;
        c.insert_url("x", b"u".to_vec(), t0);
        c.insert_file("x", m, b"f".to_vec(), t0);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get_url("x", t0), Some(&b"u"[..]));
        assert_eq!(c.get_file("x", m, t0), Some(&b"f"[..]));
    }

    #[test]
    fn file_hit_with_same_mtime_miss_when_changed() {
        let mut c = Cache::new(secs(10));
        let t0 = Instant::now();
        let m1 = SystemTime::UNIX_EPOCH + secs(100);
        let m2 = SystemTime::UNIX_EPOCH + secs(200);
        c.insert_file("/a", m1, b"ct".to_vec(), t0);
        assert_eq!(c.get_file("/a", m1, t0 + secs(1)), Some(&b"ct"[..]));
        assert_eq!(c.get_file("/a", m2, t0 + secs(2)), None);
        assert_eq!(c.len(), 0, "mtime-mismatched entry is removed");
    }

    #[test]
    fn file_entry_also_expires_by_ttl() {
        let mut c = Cache::new(secs(10));
        let t0 = Instant::now();
        let m = SystemTime::UNIX_EPOCH;
        c.insert_file("/a", m, vec![], t0);
        assert_eq!(c.get_file("/a", m, t0 + secs(10)), None);
    }

    #[test]
    fn clear_removes_everything() {
        let mut c = Cache::new(secs(10));
        let t0 = Instant::now();
        c.insert_url("a", vec![], t0);
        c.insert_file("b", SystemTime::UNIX_EPOCH, vec![], t0);
        c.clear();
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn sweep_removes_only_expired() {
        let mut c = Cache::new(secs(10));
        let t0 = Instant::now();
        c.insert_url("old", vec![], t0);
        c.insert_url("new", vec![], t0 + secs(8));
        assert_eq!(c.sweep(t0 + secs(12)), 1);
        assert_eq!(c.len(), 1);
        assert!(c.get_url("new", t0 + secs(12)).is_some());
    }
}
