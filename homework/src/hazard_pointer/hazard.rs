use core::marker::PhantomData;
use core::ptr::{self, NonNull};
use std::collections::HashSet;
use std::fmt;

#[cfg(not(feature = "check-loom"))]
use core::sync::atomic::{fence, AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use crossbeam_epoch::Pointer;
#[cfg(feature = "check-loom")]
use loom::sync::atomic::{fence, AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::ptr::null;

use super::HAZARDS;

/// Represents the ownership of a hazard pointer slot.
pub struct Shield {
    slot: NonNull<HazardSlot>,
    _marker: PhantomData<*mut ()>, // !Send + !Sync
}

impl Shield {
    /// Creates a new shield for hazard pointer.
    pub fn new(hazards: &HazardBag) -> Self {
        let slot = hazards.acquire_slot();
        Self {
            slot: slot.into(),
            _marker: PhantomData,
        }
    }

    /// Store `pointer` to the hazard slot.
    pub fn set<T>(&self, pointer: *mut T) {
        // todo!()
        let mut hzslot = self.slot.as_ptr();
        unsafe { (*hzslot).hazard.store(pointer as usize, Ordering::SeqCst) };
    }

    /// Clear the hazard slot.
    pub fn clear(&self) {
        self.set(ptr::null_mut::<()>())
    }

    /// Check if `src` still points to `pointer`. If not, returns the current value.
    ///
    /// For a pointer `p`, if "`src` still pointing to `pointer`" implies that `p` is not retired,
    /// then `Ok(())` means that shields set to `p` are validated.
    pub fn validate<T>(pointer: *mut T, src: &AtomicPtr<T>) -> Result<(), *mut T> {
        // todo!()
        let ori_ptr = src.load(Ordering::SeqCst);
        if ori_ptr != pointer {
            Err(ori_ptr)
        } else {
            Ok(())
        }
    }

    /// Try protecting `pointer` obtained from `src`. If not, returns the current value.
    ///
    /// If "`src` still pointing to `pointer`" implies that `pointer` is not retired, then `Ok(())`
    /// means that this shield is validated.
    pub fn try_protect<T>(&self, pointer: *mut T, src: &AtomicPtr<T>) -> Result<(), *mut T> {
        self.set(pointer);
        Self::validate(pointer, src).map_err(|new: *mut T| {
            self.clear();
            new
        })
    }

    /// Get a protected pointer from `src`.
    ///
    /// See `try_protect()`.
    pub fn protect<T>(&self, src: &AtomicPtr<T>) -> *mut T {
        let mut pointer = src.load(Ordering::Relaxed);
        while let Err(new) = self.try_protect(pointer, src) {
            pointer = new;
            #[cfg(feature = "check-loom")]
            loom::sync::atomic::spin_loop_hint();
        }
        pointer
    }
}

impl Default for Shield {
    fn default() -> Self {
        Self::new(&HAZARDS)
    }
}

impl Drop for Shield {
    /// Clear and release the ownership of the hazard slot.
    fn drop(&mut self) {
        // todo!()
        unsafe {
            let slot = self.slot.as_ptr();
            (*slot).active.store(false, Ordering::SeqCst);
        }
    }
}

impl fmt::Debug for Shield {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shield")
            .field("slot address", &self.slot)
            .field("slot data", unsafe { self.slot.as_ref() })
            .finish()
    }
}

/// Global bag (multiset) of hazards pointers.
/// `HazardBag.head` and `HazardSlot.next` form a grow-only list of all hazard slots. Slots are
/// never removed from this list. Instead, it gets deactivated and recycled for other `Shield`s.
#[derive(Debug)]
pub struct HazardBag {
    head: AtomicPtr<HazardSlot>,
}

/// See `HazardBag`
#[derive(Debug)]
struct HazardSlot {
    // Whether this slot is occupied by a `Shield`.
    active: AtomicBool,
    // Machine representation of the hazard pointer.
    hazard: AtomicUsize,
    // Immutable pointer to the next slot in the bag.
    next: *const HazardSlot,
}

impl HazardSlot {
    fn new() -> Self {
        // todo!()
        Self {
            active: AtomicBool::new(false),
            hazard: AtomicUsize::new(0),
            next: ptr::null(),
        }
    }
}

impl HazardBag {
    #[cfg(not(feature = "check-loom"))]
    /// Creates a new global hazard set.
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    #[cfg(feature = "check-loom")]
    /// Creates a new global hazard set.
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// Acquires a slot in the hazard set, either by recycling an inactive slot or allocating a new
    /// slot.
    fn acquire_slot(&self) -> &HazardSlot {
        // todo!()
        if let Some(hzslot) = self.try_acquire_inactive() {
            hzslot
        } else {
            let mut new_slot = Box::into_raw(Box::new(HazardSlot::new()));
            unsafe {
                (*new_slot).active.store(true, Ordering::SeqCst);
            }
            loop {
                let ori_head = self.head.load(Ordering::SeqCst);
                unsafe {
                    (*new_slot).next = ori_head;
                }
                if self
                    .head
                    .compare_exchange(ori_head, new_slot, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    continue;
                }
                return unsafe { new_slot.as_ref().expect("Not null") };
            }
        }
    }

    /// Find an inactive slot and activate it.
    fn try_acquire_inactive(&self) -> Option<&HazardSlot> {
        // todo!()
        let mut slot = self.head.load(Ordering::SeqCst);
        loop {
            if slot.is_null() {
                return None;
            }
            unsafe {
                if (*slot)
                    .active
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    slot = (*slot).next.cast_mut();
                    continue;
                }
                return slot.as_ref();
            }
        }
    }

    /// Returns all the hazards in the set.
    pub fn all_hazards(&self) -> HashSet<usize> {
        // todo!()
        let mut hashset = HashSet::new();
        let mut slot = self.head.load(Ordering::SeqCst);
        loop {
            if slot.is_null() {
                return hashset;
            }
            unsafe {
                let mut curr_hazard = (*slot).hazard.load(Ordering::SeqCst);
                if (*slot).active.load(Ordering::SeqCst) {
                    // let mut curr_hazard = (*slot).hazard.load(Ordering::SeqCst);
                    if curr_hazard != 0 {
                        hashset.insert(curr_hazard);
                    }
                }
                slot = (*slot).next.cast_mut();
            }
        }
    }

    /// make all pointer as null.
    pub fn retire_aux(&self, pointer: usize) {
        let mut slot = self.head.load(Ordering::SeqCst);
        loop {
            if slot.is_null() {
                return;
            }
            unsafe {
                let mut curr_hazard = (*slot).hazard.load(Ordering::SeqCst);
                let _ =
                    (*slot)
                        .hazard
                        .compare_exchange(pointer, 0, Ordering::SeqCst, Ordering::SeqCst);
                slot = (*slot).next.cast_mut();
            }
        }
    }
}

impl Drop for HazardBag {
    /// Frees all slots.
    fn drop(&mut self) {
        // todo!()
        let mut curr_slot = self.head.load(Ordering::SeqCst);
        loop {
            if curr_slot.is_null() {
                break;
            }
            let slot_to_remove = curr_slot;
            curr_slot = unsafe { (*curr_slot).next.cast_mut() };
            unsafe { drop(Box::from_raw(slot_to_remove)) };
        }
    }
}

unsafe impl Send for HazardSlot {}
unsafe impl Sync for HazardSlot {}

#[cfg(all(test, not(feature = "check-loom")))]
mod tests {
    use super::{HazardBag, Shield};
    use std::collections::HashSet;
    use std::mem;
    use std::ops::Range;
    use std::sync::{atomic::AtomicPtr, Arc};
    use std::thread;

    const THREADS: usize = 8;
    const VALUES: Range<usize> = 1..1024;

    // `all_hazards` should return hazards protected by shield(s).
    #[test]
    fn all_hazards_protected() {
        let hazard_bag = Arc::new(HazardBag::new());
        (0..THREADS)
            .map(|_| {
                let hazard_bag = hazard_bag.clone();
                thread::spawn(move || {
                    for data in VALUES {
                        let src = AtomicPtr::new(data as *mut ());
                        let shield = Shield::new(&hazard_bag);
                        let _ = shield.protect(&src);
                        // leak the shield so that it is not unprotected.
                        mem::forget(shield);
                    }
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .for_each(|th| th.join().unwrap());
        let all = hazard_bag.all_hazards();
        let values = VALUES.collect();
        assert!(all.is_superset(&values))
    }

    // `all_hazards` should not return values that are no longer protected.
    #[test]
    fn all_hazards_unprotected() {
        let hazard_bag = Arc::new(HazardBag::new());
        (0..THREADS)
            .map(|_| {
                let hazard_bag = hazard_bag.clone();
                thread::spawn(move || {
                    for data in VALUES {
                        let src = AtomicPtr::new(data as *mut ());
                        let shield = Shield::new(&hazard_bag);
                        let _ = shield.protect(&src);
                    }
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .for_each(|th| th.join().unwrap());
        let all = hazard_bag.all_hazards();
        let values = VALUES.collect();
        let intersection: HashSet<_> = all.intersection(&values).collect();
        assert!(intersection.is_empty())
    }

    // `acquire_slot` should recycle existing slots.
    #[test]
    fn recycle_slots() {
        let hazard_bag = HazardBag::new();
        // allocate slots
        let shields = (0..1024)
            .map(|_| Shield::new(&hazard_bag))
            .collect::<Vec<_>>();
        // slot addresses
        let old_slots = shields
            .iter()
            .map(|s| s.slot.as_ptr() as usize)
            .collect::<HashSet<_>>();
        // release the slots
        drop(shields);

        let shields = (0..128)
            .map(|_| Shield::new(&hazard_bag))
            .collect::<Vec<_>>();
        let new_slots = shields
            .iter()
            .map(|s| s.slot.as_ptr() as usize)
            .collect::<HashSet<_>>();

        // no new slots should've been created
        assert!(new_slots.is_subset(&old_slots));
    }
}
