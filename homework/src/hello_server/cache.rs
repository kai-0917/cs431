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
    inner: Arc<RwLock<HashMap<K, Arc<Mutex<Option<V>>>>>>,
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
        // if let Some(value) = self.inner.read().unwrap().get(&key) {
        //     return value.lock().unwrap().unwrap().clone()
        // }
        
        let mutex = Arc::new(Mutex::new(None));
        // If the key doesn't exist, insert a new value using lock
        {
            let mut inner = self.inner.write().unwrap();

            match inner.entry(key.clone()) {
                Entry::Occupied(entry) => {
                    while let 1 = 1 {
                        let a = entry.get().lock().unwrap();
                        match a.as_ref() {
                            None => {}
                            Some(value) => {
                                return value.clone()
                            }
                        }
                    }
                },
                Entry::Vacant(entry) => {
                    // inner.entry(key.clone()).or_insert_with_key(|key|Arc::new(Mutex::new(f(*key))))
                    //     .lock().unwrap().clone()
                    let a = mutex.clone();
                    entry.insert(a);
                }
            }
        }
        let mut guard = mutex.lock().unwrap();
        // let mut inner = self.inner.write().unwrap();
        let value = f(key.clone());
        *guard = Some(value.clone());
        value
    }
}
