//! Split-ordered linked list.

use core::mem;
use core::sync::atomic::{AtomicUsize, Ordering};
use crossbeam_epoch::{Atomic, Guard, Owned};
use cs431::lockfree::list::{Cursor, List, Node};
use std::mem::size_of;
use std::ops::Deref;
use std::thread::current;
use std::usize;

use super::growable_array::GrowableArray;
use crate::NonblockingMap;

/// Lock-free map from `usize` in range [0, 2^63-1] to `V`.
///
/// NOTE: We don't care about hashing in this homework for simplicity.
#[derive(Debug)]
pub struct SplitOrderedList<V> {
    /// Lock-free list sorted by recursive-split order. Use `None` sentinel node value.
    list: List<usize, Option<V>>,
    /// array of pointers to the buckets
    buckets: GrowableArray<Node<usize, Option<V>>>,
    /// number of buckets
    size: AtomicUsize,
    /// number of items
    count: AtomicUsize,
}

impl<V> Default for SplitOrderedList<V> {
    fn default() -> Self {
        Self {
            list: List::new(),
            buckets: GrowableArray::new(),
            size: AtomicUsize::new(2),
            count: AtomicUsize::new(0),
        }
    }
}

impl<V> SplitOrderedList<V> {
    /// `size` is doubled when `count > size * LOAD_FACTOR`.
    const LOAD_FACTOR: usize = 2;

    /// Creates a new split ordered list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a cursor and moves it to the bucket for the given index.  If the bucket doesn't
    /// exist, recursively initializes the buckets.
    fn lookup_bucket<'s>(&'s self, index: usize, guard: &'s Guard) -> Cursor<'s, usize, Option<V>> {
        // todo!()
        let bucket = self.buckets.get(index, guard);
        let sent_key = index.reverse_bits();
        let mut new_node: Owned<Node<usize, Option<V>>>;
        loop {
            let ori_sen_node = bucket.load(Ordering::SeqCst, guard);
            if !ori_sen_node.is_null() {
                return Cursor::new(bucket, ori_sen_node);
            }
            if index == 0 {
                let mut head = self.list.head(guard);
                new_node = Owned::new(Node::new(sent_key, None));
                match head.insert(new_node, guard) {
                    Ok(()) => {
                        bucket.store(head.curr(), Ordering::SeqCst);
                        return head;
                    }
                    Err(n) => {
                        continue;
                    }
                }
            }
            let mut parent = self.size.load(Ordering::SeqCst);
            loop {
                parent >>= 1;
                if parent <= index {
                    break;
                }
            }
            let parent_index = index - parent;
            let mut prev_bucket = self.lookup_bucket(parent_index, guard);
            let Ok(found) = prev_bucket.find_harris_michael(&sent_key, guard) else {
                continue;
            };
            if found {
                return prev_bucket;
            }
            let mut new_node = Owned::new(Node::new(sent_key, None));
            match prev_bucket.insert(new_node, guard) {
                Ok(()) => {
                    bucket.store(prev_bucket.curr(), Ordering::SeqCst);
                    return prev_bucket;
                }
                Err(n) => {
                    continue;
                }
            }
        }
    }

    /// Moves the bucket cursor returned from `lookup_bucket` to the position of the given key.
    /// Returns `(size, found, cursor)`
    fn find<'s>(
        &'s self,
        key: &usize,
        guard: &'s Guard,
    ) -> (usize, bool, Cursor<'s, usize, Option<V>>) {
        // todo!()
        let bucket_index = (*key) % self.size.load(Ordering::SeqCst);
        let spl_ord_key = (key.reverse_bits()) | 1;
        loop {
            let mut cursor = self.lookup_bucket(bucket_index, guard);
            if let Ok(found) = cursor.find_harris_michael(&spl_ord_key, guard) {
                return (self.size.load(Ordering::SeqCst), found, cursor);
            }
        }
    }

    fn assert_valid_key(key: usize) {
        assert!(key.leading_zeros() != 0);
    }

    fn resize(&self, guard: &Guard) {
        let ori_size = self.size.load(Ordering::SeqCst);
        if self.count.load(Ordering::SeqCst) / ori_size > Self::LOAD_FACTOR {
            let _ = self.size.compare_exchange(
                ori_size,
                ori_size * 2,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }
    }
}

impl<V> NonblockingMap<usize, V> for SplitOrderedList<V> {
    fn lookup<'a>(&'a self, key: &usize, guard: &'a Guard) -> Option<&'a V> {
        Self::assert_valid_key(*key);
        // todo!()
        let (_, found, cursor) = self.find(key, guard);
        if found {
            let node = cursor.lookup().unwrap();
            Some(node.as_ref().unwrap())
        } else {
            None
        }
    }

    fn insert(&self, key: &usize, value: V, guard: &Guard) -> Result<(), V> {
        Self::assert_valid_key(*key);
        // todo!()
        let spl_ord_key = key.reverse_bits() | 1;
        let mut new_node = Owned::new(Node::new(spl_ord_key, Some(value)));
        loop {
            let (_, found, mut cursor) = self.find(key, guard);
            if found {
                return Err(new_node.into_box().into_value().unwrap());
            }
            match cursor.insert(new_node, guard) {
                Err(n) => {
                    new_node = n;
                    continue;
                }
                Ok(()) => {
                    self.count.fetch_add(1, Ordering::SeqCst);
                    self.resize(guard);
                    return Ok(());
                }
            }
        }
    }

    fn delete<'a>(&'a self, key: &usize, guard: &'a Guard) -> Result<&'a V, ()> {
        Self::assert_valid_key(*key);
        // todo!()
        loop {
            let (_, found, cursor) = self.find(key, guard);
            if !found {
                return Err(());
            }
            match cursor.delete(guard) {
                Err(()) => continue,
                Ok(v) => {
                    self.count.fetch_sub(1, Ordering::SeqCst);
                    return Ok(v.as_ref().unwrap());
                }
            }
        }
    }
}
