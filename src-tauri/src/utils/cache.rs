use std::collections::HashMap;
use std::hash::Hash;

#[derive(Debug)]
struct CacheSlot<V> {
    value: V,
    last_access_tick: u64,
}

#[derive(Debug)]
pub struct BoundedCache<K, V> {
    capacity: usize,
    next_access_tick: u64,
    entries: HashMap<K, CacheSlot<V>>,
}

impl<K, V> BoundedCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_access_tick: 0,
            entries: HashMap::new(),
        }
    }

    pub fn get_cloned(&mut self, key: &K) -> Option<V> {
        let access_tick = self.bump_access_tick();
        self.entries.get_mut(key).map(|slot| {
            slot.last_access_tick = access_tick;
            slot.value.clone()
        })
    }

    pub fn insert(&mut self, key: K, value: V) {
        let access_tick = self.bump_access_tick();
        self.entries.insert(
            key,
            CacheSlot {
                value,
                last_access_tick: access_tick,
            },
        );
        self.evict_if_needed();
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn bump_access_tick(&mut self) -> u64 {
        let current = self.next_access_tick;
        self.next_access_tick = self.next_access_tick.wrapping_add(1);
        current
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.capacity {
            let evict_key = self
                .entries
                .iter()
                .min_by_key(|(_, slot)| slot.last_access_tick)
                .map(|(key, _)| key.clone());

            let Some(evict_key) = evict_key else {
                break;
            };
            self.entries.remove(&evict_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BoundedCache;

    #[test]
    fn evicts_least_recently_used_entry_when_capacity_is_exceeded() {
        let mut cache = BoundedCache::new(2);
        cache.insert("a", 1);
        cache.insert("b", 2);
        assert_eq!(cache.get_cloned(&"a"), Some(1));

        cache.insert("c", 3);

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get_cloned(&"a"), Some(1));
        assert_eq!(cache.get_cloned(&"b"), None);
        assert_eq!(cache.get_cloned(&"c"), Some(3));
    }
}
