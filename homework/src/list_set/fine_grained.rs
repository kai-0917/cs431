use std::cmp;
use std::mem;
use std::ptr;
use std::sync::{Mutex, MutexGuard};

use crate::ConcurrentSet;

#[derive(Debug)]
struct Node<T> {
    data: T,
    next: Mutex<*mut Node<T>>,
}

/// Concurrent sorted singly linked list using fine-grained lock-coupling.
#[derive(Debug)]
pub struct FineGrainedListSet<T> {
    head: Mutex<*mut Node<T>>,
}

unsafe impl<T: Send> Send for FineGrainedListSet<T> {}
unsafe impl<T: Send> Sync for FineGrainedListSet<T> {}

// reference to the `next` field of previous node which points to the current node
struct Cursor<'l, T>(MutexGuard<'l, *mut Node<T>>);

impl<T> Node<T> {
    fn new(data: T, next: *mut Self) -> *mut Self {
        Box::into_raw(Box::new(Self {
            data,
            next: Mutex::new(next),
        }))
    }
}

impl<T: Ord> Cursor<'_, T> {
    /// Moves the cursor to the position of key in the sorted list.
    /// Returns whether the value was found.
    fn find(&mut self, key: &T) -> bool {
        // todo!()
        loop {
            // if the node pointer within the cursor's MG is null
            if (*self.0).is_null() {
                return false;
            }
            // there exists a next node, retrieve the data
            let next_data = unsafe { &(**self.0).data };
            // if exists, the cursor points to the matching node
            if next_data == key {
                return true;
            }
            // if not exists, the cursor points to the smallest node whose data is larger than key
            if next_data > key {
                return false;
            }
            if next_data < key {
                self.0 = unsafe { (**self.0).next.lock().unwrap() };
            }
        }
    }
}

impl<T> FineGrainedListSet<T> {
    /// Creates a new list.
    pub fn new() -> Self {
        Self {
            head: Mutex::new(ptr::null_mut()),
        }
    }
}

impl<T: Ord> FineGrainedListSet<T> {
    fn find(&self, key: &T) -> (bool, Cursor<'_, T>) {
        // todo!()
        let mut cursor = Cursor(self.head.lock().unwrap());
        (cursor.find(key), cursor)
    }
}

impl<T: Ord> ConcurrentSet<T> for FineGrainedListSet<T> {
    fn contains(&self, key: &T) -> bool {
        self.find(key).0
    }

    fn insert(&self, key: T) -> bool {
        // todo!()
        let mut cursor = Cursor(self.head.lock().unwrap());
        if cursor.find(&key) {
            false
        } else {
            let new_node = Node::new(key, *cursor.0);
            *cursor.0 = new_node;
            true
        }
    }

    fn remove(&self, key: &T) -> bool {
        // todo!()
        let mut cursor = Cursor(self.head.lock().unwrap());
        if !cursor.find(key) {
            false
        } else {
            let node_found_ptr = *cursor.0;
            *cursor.0 = unsafe { *(**cursor.0).next.lock().unwrap() };
            unsafe {
                drop(Box::from_raw(node_found_ptr));
            }
            true
        }
    }
}

#[derive(Debug)]
pub struct Iter<'l, T>(MutexGuard<'l, *mut Node<T>>);

impl<T> FineGrainedListSet<T> {
    /// An iterator visiting all elements.
    pub fn iter(&self) -> Iter<'_, T> {
        Iter(self.head.lock().unwrap())
    }
}

impl<'l, T> Iterator for Iter<'l, T> {
    type Item = &'l T;

    fn next(&mut self) -> Option<Self::Item> {
        // todo!()
        if (*self.0).is_null() {
            None
        } else {
            let result = unsafe { Some(&(**self.0).data) };
            self.0 = unsafe { (**self.0).next.lock().unwrap() };
            result
        }
    }
}

impl<T> Drop for FineGrainedListSet<T> {
    fn drop(&mut self) {
        // todo!()
        let mut mg_head = self.head.lock().unwrap();
        while !(*mg_head).is_null() {
            let node_to_free = *mg_head;
            *mg_head = unsafe { *(**mg_head).next.lock().unwrap() };
            unsafe {
                drop(Box::from_raw(node_to_free));
            }
        }
    }
}

impl<T> Default for FineGrainedListSet<T> {
    fn default() -> Self {
        Self::new()
    }
}
