//! Growable array.

use core::fmt::Debug;
use core::marker::PhantomData;
use core::mem;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicUsize, Ordering};
use crossbeam_epoch::{Atomic, Guard, Owned, Pointer, Shared};

/// Growable array of `Atomic<T>`.
///
/// This is more complete version of the dynamic sized array from the paper. In the paper, the
/// segment table is an array of arrays (segments) of pointers to the elements. In this
/// implementation, a segment contains the pointers to the elements **or other segments**. In other
/// words, it is a tree that has segments as internal nodes.
///
/// # Example run
///
/// Suppose `SEGMENT_LOGSIZE = 3` (segment size 8).
///
/// When a new `GrowableArray` is created, `root` is initialized with `Atomic::null()`.
///
/// ```text
///                          +----+
///                          |root|
///                          +----+
/// ```
///
/// When you store element `cat` at the index `0b001`, it first initializes a segment.
///
/// ```text
///                          +----+
///                          |root|
///                          +----+
///                            | height: 1
///                            v
///                 +---+---+---+---+---+---+---+---+
///                 |111|110|101|100|011|010|001|000|
///                 +---+---+---+---+---+---+---+---+
///                                           |
///                                           v
///                                         +---+
///                                         |cat|
///                                         +---+
/// ```
///
/// When you store `fox` at `0b111011`, it is clear that there is no room for indices larger than
/// `0b111`. So it first allocates another segment for upper 3 bits and moves the previous root
/// segment (`0b000XXX` segment) under the `0b000XXX` branch of the the newly allocated segment.
///
/// ```text
///                          +----+
///                          |root|
///                          +----+
///                            | height: 2
///                            v
///                 +---+---+---+---+---+---+---+---+
///                 |111|110|101|100|011|010|001|000|
///                 +---+---+---+---+---+---+---+---+
///                                               |
///                                               v
///                                      +---+---+---+---+---+---+---+---+
///                                      |111|110|101|100|011|010|001|000|
///                                      +---+---+---+---+---+---+---+---+
///                                                                |
///                                                                v
///                                                              +---+
///                                                              |cat|
///                                                              +---+
/// ```
///
/// And then, it allocates another segment for `0b111XXX` indices.
///
/// ```text
///                          +----+
///                          |root|
///                          +----+
///                            | height: 2
///                            v
///                 +---+---+---+---+---+---+---+---+
///                 |111|110|101|100|011|010|001|000|
///                 +---+---+---+---+---+---+---+---+
///                   |                           |
///                   v                           v
/// +---+---+---+---+---+---+---+---+    +---+---+---+---+---+---+---+---+
/// |111|110|101|100|011|010|001|000|    |111|110|101|100|011|010|001|000|
/// +---+---+---+---+---+---+---+---+    +---+---+---+---+---+---+---+---+
///                   |                                            |
///                   v                                            v
///                 +---+                                        +---+
///                 |fox|                                        |cat|
///                 +---+                                        +---+
/// ```
///
/// Finally, when you store `owl` at `0b000110`, it traverses through the `0b000XXX` branch of the
/// level-1 segment and arrives at its 0b110` leaf.
///
/// ```text
///                          +----+
///                          |root|
///                          +----+
///                            | height: 2
///                            v
///                 +---+---+---+---+---+---+---+---+
///                 |111|110|101|100|011|010|001|000|
///                 +---+---+---+---+---+---+---+---+
///                   |                           |
///                   v                           v
/// +---+---+---+---+---+---+---+---+    +---+---+---+---+---+---+---+---+
/// |111|110|101|100|011|010|001|000|    |111|110|101|100|011|010|001|000|
/// +---+---+---+---+---+---+---+---+    +---+---+---+---+---+---+---+---+
///                   |                        |                   |
///                   v                        v                   v
///                 +---+                    +---+               +---+
///                 |fox|                    |owl|               |cat|
///                 +---+                    +---+               +---+
/// ```
///
/// When the array is dropped, only the segments are dropped and the **elements must not be
/// dropped/deallocated**.
///
/// ```text
///                 +---+                    +---+               +---+
///                 |fox|                    |owl|               |cat|
///                 +---+                    +---+               +---+
/// ```
///
/// Instead, it should be handled by the container that the elements actually belong to. For
/// example in `SplitOrderedList`, destruction of elements are handled by `List`.
#[derive(Debug)]
pub struct GrowableArray<T> {
    root: Atomic<Segment>,
    _marker: PhantomData<T>,
}

const SEGMENT_LOGSIZE: usize = 10;

struct Segment {
    /// `AtomicUsize` here means `Atomic<T>` or `Atomic<Segment>`.
    inner: [AtomicUsize; 1 << SEGMENT_LOGSIZE],
}

impl Segment {
    fn new() -> Self {
        Self {
            inner: unsafe {
                // SAFETY: `AtomicUsize` can be zero.
                mem::zeroed()
            },
        }
    }

    fn free_all(&mut self) {
        for i in 0..(1 << 10) {
            let curr = (*self)[i].load(Ordering::SeqCst);
            if curr != 0 {
                unsafe {
                    (*(curr as *mut Segment)).free_all();
                }
            }
        }
        unsafe {
            drop(Box::from_raw(self as *mut Segment));
        }
    }

    fn free_with_level(&mut self, l: usize) {
        if l != 0 {
            for i in 0..(1 << 10) {
                let curr = (*self)[i].load(Ordering::SeqCst);
                if curr != 0 {
                    unsafe {
                        (*(curr as *mut Segment)).free_with_level(l - 1);
                    }
                }
            }
        }
        unsafe {
            drop(Box::from_raw(self as *mut Segment));
        }
    }
}

impl Deref for Segment {
    type Target = [AtomicUsize; 1 << SEGMENT_LOGSIZE];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Segment {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Debug for Segment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Segment")
    }
}

impl<T> Drop for GrowableArray<T> {
    /// Deallocate segments, but not the individual elements.
    fn drop(&mut self) {
        // todo!()
        let mut guard = crossbeam_epoch::pin();
        let mut root = self.root.load(Ordering::SeqCst, &guard);
        let height = root.tag();
        unsafe {
            let mut raw = root.with_tag(0).as_raw() as usize;
            if raw != 0 {
                (*(raw as *mut Segment)).free_with_level(height - 1);
            }
        }
    }
}

impl<T> Default for GrowableArray<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> GrowableArray<T> {
    /// Create a new growable array.
    pub fn new() -> Self {
        Self {
            root: Atomic::null(),
            _marker: PhantomData,
        }
    }

    /// Returns the reference to the `Atomic` pointer at `index`. Allocates new segments if
    /// necessary.
    pub fn get(&self, mut index: usize, guard: &Guard) -> &Atomic<T> {
        // todo!()
        let mut v = Vec::new();
        if index == 0 {
            v.insert(0, 0);
        }
        while index != 0 {
            v.insert(0, index & ((1 << 10) - 1));
            index >>= 10;
        }
        loop {
            // println!("index {:?}", v);
            let ori_root = self.root.load(Ordering::SeqCst, guard);
            let height = ori_root.tag();
            // increase the height
            if height < v.len() {
                let mut new_root = Box::into_raw(Box::new(Segment::new()));
                let mut leaf_seg_ptr1 = new_root;
                let mut l1: usize = 0;
                if height != 0 {
                    while l1 < v.len() - height - 1 {
                        unsafe {
                            let new_seg = Box::into_raw(Box::new(Segment::new()));
                            (**leaf_seg_ptr1)[0].store(new_seg as usize, Ordering::SeqCst);
                            leaf_seg_ptr1 = new_seg;
                        }
                        l1 += 1;
                    }
                    unsafe {
                        (**leaf_seg_ptr1)[0].store(ori_root.as_raw() as usize, Ordering::SeqCst);
                    }
                }
                unsafe {
                    if self
                        .root
                        .compare_exchange(
                            ori_root,
                            Shared::from_usize(new_root as usize).with_tag(v.len()),
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                            guard,
                        )
                        .is_err()
                    {
                        (**leaf_seg_ptr1)[0].store(0, Ordering::SeqCst);
                        // (*new_root).free_all();
                        (*new_root).free_with_level(v.len() - 1);
                        continue;
                    }
                }
                continue;
            }
            // not need to increase the height
            for i in 0..(height - v.len()) {
                v.insert(0, 0);
            }
            let mut leaf_seg_ptr = ori_root.as_raw() as *mut Segment;
            let mut l = 0;
            loop {
                if l >= v.len() - 1 {
                    break;
                }
                unsafe {
                    let ptr = (**leaf_seg_ptr)[v[l]].load(Ordering::SeqCst);
                    if ptr == 0 {
                        break;
                    }
                    leaf_seg_ptr = ptr as *mut Segment;
                    l += 1;
                }
            }
            let remaining = v.len() - l - 1;
            if remaining != 0 {
                let mut new_seg = Box::into_raw(Box::new(Segment::new()));
                let mut leaf_seg_ptr1 = new_seg;
                for i in 0..(remaining - 1) {
                    unsafe {
                        let new_seg = Box::into_raw(Box::new(Segment::new()));
                        (**leaf_seg_ptr1)[v[l + i + 1]].store(new_seg as usize, Ordering::SeqCst);
                        leaf_seg_ptr1 = new_seg;
                    }
                }
                unsafe {
                    if (**leaf_seg_ptr)[v[l]]
                        .compare_exchange(0, new_seg as usize, Ordering::SeqCst, Ordering::SeqCst)
                        .is_err()
                    {
                        // (*new_seg).free_all();
                        (*new_seg).free_with_level(v.len() - 1);
                        continue;
                    }
                }
                leaf_seg_ptr = leaf_seg_ptr1;
            }
            return unsafe {
                &*((*leaf_seg_ptr).get_unchecked(v[v.len() - 1]) as *const _ as *const Atomic<T>)
            };
        }
    }
}
