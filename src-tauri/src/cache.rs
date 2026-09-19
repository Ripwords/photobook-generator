/// One analysed photo as the cache budget sees it: its features row and its
/// thumbnail, sized together, since evicting one without the other leaves
/// either a re-analysis that keeps a stale thumbnail or a thumbnail nothing
/// will ever show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub hash: String,
    pub bytes: u64,
    pub last_used_at: i64,
    /// A saved book, a draft or an open contact sheet needs this photo.
    /// `resolve_photos` is all or nothing, so evicting one pinned hash
    /// breaks the whole book it belongs to.
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictionPlan {
    pub evict: Vec<String>,
    pub bytes_after: u64,
    /// Pinned entries alone exceed the limit. Nothing more can be evicted
    /// without breaking something the user still has.
    pub over_budget: bool,
}

/// Least recently used first, never a pinned entry. A limit of 0 evicts
/// every unpinned entry, which is what "Clear unused" means.
pub fn plan_eviction(entries: &[CacheEntry], limit_bytes: u64) -> EvictionPlan {
    let mut bytes: u64 = entries.iter().map(|e| e.bytes).sum();
    let mut candidates: Vec<&CacheEntry> = entries.iter().filter(|e| !e.pinned).collect();
    candidates.sort_by(|a, b| a.last_used_at.cmp(&b.last_used_at).then_with(|| a.hash.cmp(&b.hash)));
    let mut evict = Vec::new();
    for entry in candidates {
        if bytes <= limit_bytes {
            break;
        }
        bytes -= entry.bytes;
        evict.push(entry.hash.clone());
    }
    EvictionPlan { evict, bytes_after: bytes, over_budget: bytes > limit_bytes }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hash: &str, bytes: u64, last_used_at: i64, pinned: bool) -> CacheEntry {
        CacheEntry { hash: hash.into(), bytes, last_used_at, pinned }
    }

    #[test]
    fn plan_under_the_limit_evicts_nothing() {
        let entries = [entry("a", 10, 1, false), entry("b", 20, 2, false)];
        let plan = plan_eviction(&entries, 30);
        assert_eq!(plan, EvictionPlan { evict: vec![], bytes_after: 30, over_budget: false });
    }

    #[test]
    fn plan_evicts_the_least_recently_used_unpinned_entries_first() {
        let entries = [
            entry("newest", 10, 300, false),
            entry("oldest", 10, 100, false),
            entry("middle", 10, 200, false),
        ];
        let plan = plan_eviction(&entries, 15);
        assert_eq!(plan.evict, vec!["oldest".to_string(), "middle".to_string()]);
        assert_eq!(plan.bytes_after, 10);
        assert!(!plan.over_budget);
    }

    #[test]
    fn plan_breaks_a_last_used_tie_by_hash() {
        let entries = [entry("b", 10, 5, false), entry("a", 10, 5, false), entry("c", 10, 9, false)];
        assert_eq!(plan_eviction(&entries, 20).evict, vec!["a".to_string()]);
    }

    #[test]
    fn plan_never_evicts_a_pinned_entry_and_reports_over_budget() {
        let entries = [
            entry("book-photo", 100, 1, true),
            entry("stray", 10, 50, false),
            entry("draft-photo", 100, 2, true),
        ];
        let plan = plan_eviction(&entries, 150);
        assert_eq!(plan.evict, vec!["stray".to_string()]);
        assert_eq!(plan.bytes_after, 200);
        assert!(plan.over_budget);
    }

    #[test]
    fn plan_with_a_zero_limit_evicts_every_unpinned_entry() {
        let entries = [entry("a", 1, 3, false), entry("p", 1, 1, true), entry("b", 1, 2, false)];
        let plan = plan_eviction(&entries, 0);
        assert_eq!(plan.evict, vec!["b".to_string(), "a".to_string()]);
        assert_eq!(plan.bytes_after, 1);
        assert!(plan.over_budget);
    }
}
