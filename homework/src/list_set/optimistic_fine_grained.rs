use std::cmp;
use std::mem;
use std::mem::ManuallyDrop;
use std::ptr;
use std::ptr::null;
use std::ptr::null_mut;
use std::sync::atomic::Ordering;

use crate::ConcurrentSet;
use crossbeam_epoch::{pin, unprotected, Atomic, Guard, Owned, Shared};
use cs431::lock::seqlock::{ReadGuard, SeqLock, WriteGuard};

#[derive(Debug)]
struct Node<T> {
    data: T,
    next: SeqLock<Atomic<Node<T>>>,
}

/// Concurrent sorted singly linked list using fine-grained optimistic locking
#[derive(Debug)]
pub struct OptimisticFineGrainedListSet<T> {
    head: SeqLock<Atomic<Node<T>>>,
}

unsafe impl<T: Send> Send for OptimisticFineGrainedListSet<T> {}
unsafe impl<T: Send> Sync for OptimisticFineGrainedListSet<T> {}

#[derive(Debug)]
struct Cursor<'g, T> {
    // reference to the `next` field of previous node which points to the current node
    prev: ReadGuard<'g, Atomic<Node<T>>>,
    curr: Shared<'g, Node<T>>,
}

impl<T> Node<T> {
    fn new(data: T, next: Shared<Self>) -> Owned<Self> {
        Owned::new(Self {
            data,
            next: SeqLock::new(Atomic::from(next)),
        })
    }
}

impl<'g, T: Ord> Cursor<'g, T> {
    /// Moves the cursor to the position of key in the sorted list.
    /// Returns whether the value was found.
    fn find(&mut self, key: &T, guard: &'g Guard) -> Result<bool, ()> {
        loop {
            let Some(b) = (unsafe { self.curr.as_ref() }) else {
                if self.prev.validate() {
                    return Ok(false);
                }
                return Err(());
            };
            match b.data.cmp(key) {
                cmp::Ordering::Less => {
                    let rg_in_b = unsafe { b.next.read_lock() };
                    let ori_self_curr = self.curr;
                    let ori_self_prev = std::mem::replace(&mut self.prev, rg_in_b);
                    self.curr = self.prev.load(Ordering::SeqCst, guard);
                    if !self.prev.validate() {
                        ori_self_prev.finish();
                        return Err(());
                    }
                    if ori_self_prev.finish() {
                        continue;
                    }
                    return Err(());
                }
                cmp::Ordering::Equal => {
                    if self.prev.validate() {
                        return Ok(true);
                    }
                    return Err(());
                }
                cmp::Ordering::Greater => {
                    if self.prev.validate() {
                        return Ok(false);
                    }
                    return Err(());
                }
            }
        }
    }
}

impl<T> OptimisticFineGrainedListSet<T> {
    /// Creates a new list.
    pub fn new() -> Self {
        Self {
            head: SeqLock::new(Atomic::null()),
        }
    }

    fn head<'g>(&'g self, guard: &'g Guard) -> Cursor<'g, T> {
        let prev = unsafe { self.head.read_lock() };
        let curr = prev.load(Ordering::Relaxed, guard);
        Cursor { prev, curr }
    }
}

impl<T: Ord> OptimisticFineGrainedListSet<T> {
    fn find<'g>(&'g self, key: &T, guard: &'g Guard) -> Result<(bool, Cursor<'g, T>), ()> {
        // todo!()
        loop {
            let mut cursor = self.head(guard);
            match cursor.find(key, guard) {
                Err(_) => {
                    cursor.prev.finish();
                    continue;
                }
                Ok(found) => return Ok((found, cursor)),
            }
        }
    }
}

impl<T: Ord> ConcurrentSet<T> for OptimisticFineGrainedListSet<T> {
    fn contains(&self, key: &T) -> bool {
        // todo!()
        loop {
            let guard = &crossbeam_epoch::pin();
            if let Ok((found, cursor)) = self.find(key, guard) {
                if cursor.prev.finish() {
                    return found;
                }
            }
            continue;
        }
    }

    fn insert(&self, key: T) -> bool {
        let guard = &crossbeam_epoch::pin();
        loop {
            let Ok((found, cursor)) = self.find(&key, guard) else {
                continue;
            };
            if found {
                if cursor.prev.finish() {
                    return false;
                }
                continue;
            }
            let Ok(wg_in_a) = cursor.prev.upgrade() else {
                continue;
            };
            let c = (*wg_in_a).load(Ordering::SeqCst, guard);
            let new_node = Node::new(key, c);
            (*wg_in_a).store(new_node, Ordering::SeqCst);
            return true;
        }
    }

    fn remove(&self, key: &T) -> bool {
        // todo!()
        let guard = &crossbeam_epoch::pin();
        loop {
            let Ok((found, cursor)) = self.find(key, guard) else {
                continue;
            };
            // key exists
            if !found {
                if cursor.prev.finish() {
                    return false;
                }
                continue;
            }
            let Ok(wg_in_a) = cursor.prev.upgrade() else {
                continue;
            };
            let b = unsafe { cursor.curr.as_ref().unwrap() };
            let wg_in_b = b.next.write_lock();
            (*wg_in_a).store((*wg_in_b).load(Ordering::SeqCst, guard), Ordering::SeqCst);
            unsafe { crossbeam_epoch::Guard::defer_destroy(guard, cursor.curr) };
            return true;
        }
    }
}

#[derive(Debug)]
pub struct Iter<'g, T> {
    // Can be dropped without validation, because the only way to use cursor.curr is next().
    cursor: ManuallyDrop<Cursor<'g, T>>,
    guard: &'g Guard,
}

impl<T> OptimisticFineGrainedListSet<T> {
    /// An iterator visiting all elements. `next()` returns `Some(Err(()))` when validation fails.
    /// In that case, further invocation of `next()` returns `None`, and the user must restart the
    /// iteration.
    pub fn iter<'g>(&'g self, guard: &'g Guard) -> Iter<'_, T> {
        Iter {
            cursor: ManuallyDrop::new(self.head(guard)),
            guard,
        }
    }
}

impl<'g, T> Iterator for Iter<'g, T> {
    type Item = Result<&'g T, ()>;

    fn next(&mut self) -> Option<Self::Item> {
        // todo!()
        let Some(b) = (unsafe { self.cursor.curr.as_ref() }) else {
            if self.cursor.prev.validate() {
                return None;
            }
            return Some(Err(()));
        };
        let rg_in_b = unsafe { b.next.read_lock() };
        let ori_self_prev = std::mem::replace(&mut self.cursor.prev, rg_in_b);
        if !ori_self_prev.finish() {
            return Some(Err(()));
        }
        self.cursor.curr = self.cursor.prev.load(Ordering::SeqCst, self.guard);
        Some(Ok(&b.data))
    }
}

impl<T> Drop for OptimisticFineGrainedListSet<T> {
    fn drop(&mut self) {
        // todo!()
        let guard = &crossbeam_epoch::pin();
        let mut rg_in_head = unsafe { self.head.read_lock() };
        loop {
            let shared = (*rg_in_head).load(Ordering::SeqCst, guard);
            let Some(curr_node) = (unsafe { shared.as_ref() }) else {
                rg_in_head.finish();
                return;
            };
            rg_in_head.finish();
            rg_in_head = unsafe { curr_node.next.read_lock() };
            unsafe { crossbeam_epoch::Guard::defer_destroy(guard, shared) };
        }
    }
}

impl<T> Default for OptimisticFineGrainedListSet<T> {
    fn default() -> Self {
        Self::new()
    }
}
