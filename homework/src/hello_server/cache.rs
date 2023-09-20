//! Thread-safe key/value cache.

use std::collections::hash_map::{Entry, HashMap};
use std::hash::Hash;
use std::sync::{Arc, Mutex, RwLock};

/// Cache that remembers the result for each key.
#[derive(Debug, Default)]
pub struct Cache<K, V> {
    // todo! This is an example cache type. Build your own cache type that satisfies the
    // specification for `get_or_insert_with`.
    // inner: Mutex<HashMap<K, V>>,
    inner: Arc<RwLock<HashMap<K, Option<V>>>>,
}

impl<K: Eq + Hash + Clone, V: Clone> Cache<K, V> {
    /// Retrieve the value or insert a new one created by `f`.
    ///
    /// An invocation to this function should not block another invocation with a different key. For
    /// example, if a thread calls `get_or_insert_with(key1, f1)` and another thread calls
    /// `get_or_insert_with(key2, f2)` (`key1≠key2`, `key1,key2∉cache`) concurrently, `f1` and `f2`
    /// should run concurrently.
    ///
    /// On the other hand, since `f` may consume a lot of resource (= money), it's desirable not to
    /// duplicate the work. That is, `f` should be run only once for each key. Specifically, even
    /// for the concurrent invocations of `get_or_insert_with(key, f)`, `f` is called only once.
    ///
    /// Hint: the [`Entry`] API may be useful in implementing this function.
    ///
    /// [`Entry`]: https://doc.rust-lang.org/stable/std/collections/hash_map/struct.HashMap.html#method.entry
    pub fn get_or_insert_with<F: FnOnce(K) -> V>(&self, key: K, f: F) -> V {
        // todo!()
        // try to read with a RwLock to check if the key exists
        loop {
            let inner = self.inner.read().unwrap();
            if let Some(value) = inner.get(&key) {
                if let Some(v) = value {
                    return v.clone();
                } else {
                    // If the value is None, spin
                    continue;
                }
            }
            // If the key doesn't exists
            else {
                break;
            }
        }

        let mut i = 0;
        // If the key doesn't exist, insert a new value using lock
        {
            let mut inner = self.inner.write().unwrap();

            match inner.entry(key.clone()) {
                Entry::Occupied(entry) => {
                    let mut a = entry.get();
                    if let Some(value) = a {
                        return value.clone();
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(None);
                    i = 1;
                }
            }
        }

        if i == 0 {
            loop {
                let inner = self.inner.read().unwrap();
                if let Some(value) = inner.get(&key).unwrap() {
                    return value.clone();
                } else {
                    // If the value is None, spin
                    continue;
                }
            }
        }

        let value = f(key.clone());
        let mut inner = self.inner.write().unwrap();
        inner.insert(key.clone(), Some(value.clone()));
        value
    }
}
