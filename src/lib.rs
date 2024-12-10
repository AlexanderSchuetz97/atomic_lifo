//! # jni-simple
//! # atomic lifo
//! Lock free thread-safe lifo for rust.
//!
//! ## Example
//! ```rust
//! use std::thread;
//! use atomic_lifo::AtomicLifo;
//!
//! static MT_LIFO: AtomicLifo<u32> = AtomicLifo::new();
//!
//! pub fn example() {
//!     MT_LIFO.push(456);
//!     MT_LIFO.push(123);
//!     let th = {
//!         thread::spawn(move || {
//!             assert_eq!(MT_LIFO.pop(), Some(123));
//!             assert_eq!(MT_LIFO.pop(), Some(456));
//!             assert_eq!(MT_LIFO.pop(), None);
//!         })
//!     };
//!
//!     th.join().unwrap();
//! }
#![no_std]
#![deny(clippy::correctness)]
#![deny(
    clippy::perf,
    clippy::complexity,
    clippy::style,
    clippy::nursery,
    clippy::pedantic,
    clippy::clone_on_ref_ptr,
    clippy::decimal_literal_representation,
    clippy::float_cmp_const,
    clippy::missing_docs_in_private_items,
    clippy::multiple_inherent_impl,
    clippy::unwrap_used,
    clippy::cargo_common_metadata,
    clippy::used_underscore_binding
)]
extern crate alloc;

use alloc::boxed::Box;
use core::ptr::null_mut;
use core::sync::atomic::Ordering::SeqCst;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize};
use defer_heavy::defer;

/// Thread Safe LIFO Stack/Single linked list.
#[derive(Debug, Default)]
pub struct AtomicLifo<T: Sync + Send + 'static> {
    /// amount of concurrent ongoing calls to pop.
    concurrent_pop_count: AtomicUsize,
    /// current generation of hazard nodes
    hazard_generation: AtomicUsize,
    /// threshold counter to catch an edge case when generation never increments to force it to increment and the hazard list to be freed.
    hazard_threshold: AtomicUsize,
    /// provides mutual exclusion to free some elements in the hazard list.
    hazard_lock: AtomicBool,
    /// the head of the hazard list
    hazard_head: AtomicPtr<HazardNode<T>>,
    /// the head of the queue
    head: AtomicPtr<Node<T>>,
}

impl<T: Sync + Send + 'static> Drop for AtomicLifo<T> {
    fn drop(&mut self) {
        unsafe {
            let mut current_free = self.head.load(SeqCst);
            loop {
                if current_free.is_null() {
                    break;
                }

                let node = Box::from_raw(current_free);
                current_free = node.next;
                _ = Box::from_raw(node.value);
            }

            let hazard_head = self.hazard_head.load(SeqCst);
            if !hazard_head.is_null() {
                _ = Box::from_raw(hazard_head);
            }
        }
    }
}

/// Node that contains normal nodes that should be freed later.
#[derive(Debug)]
struct HazardNode<T: Sync + Send + 'static> {
    /// the generation of this hazard node
    generation: usize,
    /// the node we want to free later
    node: *mut Node<T>,
    /// next hazard node
    next: *mut HazardNode<T>,
}

impl<T: Sync + Send + 'static> Drop for HazardNode<T> {
    fn drop(&mut self) {
        if !self.node.is_null() {
            unsafe {
                _ = Box::from_raw(self.node);
            }
        }

        let mut cur_free = self.next;
        while !cur_free.is_null() {
            unsafe {
                let mut cur_free_unbox = Box::from_raw(cur_free);
                cur_free = cur_free_unbox.next;
                cur_free_unbox.next = null_mut();
            }
        }
    }
}

/// Lifo node
#[derive(Debug)]
struct Node<T: Sync + Send + 'static> {
    /// the next node
    next: *mut Node<T>,
    /// the value pointer
    value: *mut T,
}

impl<T: Sync + Send + 'static> AtomicLifo<T> {
    /// Constructs a new empty `AtomicLifo`
    #[must_use]
    pub const fn new() -> Self {
        Self {
            concurrent_pop_count: AtomicUsize::new(0),
            hazard_generation: AtomicUsize::new(0),
            hazard_threshold: AtomicUsize::new(0),
            hazard_lock: AtomicBool::new(false),
            hazard_head: AtomicPtr::new(null_mut()),
            head: AtomicPtr::new(null_mut()),
        }
    }

    /// Free the hazard list if possible.
    unsafe fn free_hazard_list(&self, count: usize) {
        /// To handle overflow we only consider elements to be of an old generation
        /// If the abs diff to the current generation is less than half the possible values.
        const MAX_DIFF: usize = usize::MAX / 2;

        if self.hazard_lock.swap(true, SeqCst) {
            return;
        }

        defer! {
            self.hazard_lock.store(false, SeqCst);
        }

        self.hazard_threshold.store(0, SeqCst);

        //The hazard head may be in flux and I don't bother trying to free it here.
        //The drop of the entire thing will free it.
        let mut cur_ptr = self.hazard_head.load(SeqCst);

        //This is unlikely to iterate too many elements as we call this fn right after we increment count.
        //Meaning the only elements that we have to skip are the ones that other threads pop in the meantime!
        while let Some(cur) = cur_ptr.as_mut() {
            let next_ptr = cur.next;
            let Some(next) = next_ptr.as_ref() else {
                return;
            };

            //Second check prevents funny overflow things.
            if next.generation < count && next.generation.abs_diff(count) <= MAX_DIFF {
                cur.next = null_mut();
                _ = Box::from_raw(next_ptr);
                return;
            }

            cur_ptr = next_ptr;
        }
    }

    pub fn push(&self, value: T) {
        let node = Box::into_raw(Box::new(Node {
            value: Box::into_raw(Box::new(value)),
            next: self.head.load(SeqCst),
        }));

        let node_ref = unsafe { node.as_mut().unwrap_unchecked() };

        loop {
            if self
                .head
                .compare_exchange(node_ref.next, node, SeqCst, SeqCst)
                .is_err()
            {
                node_ref.next = self.head.load(SeqCst);
                continue;
            }

            return;
        }
    }

    ///
    /// Pops the top of the lifo stack
    ///
    /// # Panics
    /// if more than `usize::MAX` concurrent calls in different threads to this fn are made.
    ///
    pub fn pop(&self) -> Option<T> {
        while self.hazard_threshold.load(SeqCst) > 500_000 {
            //This is an edge case where we have an absurd amount of threads spinning
            //on pop and actually succeed in removing elements.
            //This will make acc_count never reach 0 all while the hazard list grows without it ever being freed.
            //To break this we just spin here until the acc_count reaches 0 and the hazard free is invoked by some thread currently still in pop.
            core::hint::spin_loop();
        }

        assert_ne!(
            self.concurrent_pop_count.fetch_add(1, SeqCst),
            usize::MAX,
            "Too many threads calling pop concurrently"
        );

        defer! {
            let sub = self.concurrent_pop_count.fetch_sub(1, SeqCst);
            debug_assert_ne!(sub, 0, "AtomicLifo::poll UNDERFLOW");
            if sub != 1 {
                return;
            }

            let haz_cnt = self.hazard_generation.fetch_add(1, SeqCst);
            unsafe {
                self.free_hazard_list(haz_cnt);
            }
        }

        let removed = loop {
            let head = self.head.load(SeqCst);
            let next = unsafe { head.as_ref()?.next };

            if self
                .head
                .compare_exchange(head, next, SeqCst, SeqCst)
                .is_err()
            {
                continue;
            }

            break head;
        };

        //Safe, removed must be non-null and we "own" it here for a very short time. Other thread may be currently looking at the next pointer only.
        let removed_obj = unsafe { Box::from_raw(removed.as_ref().unwrap_unchecked().value) };

        let count = self.hazard_generation.load(SeqCst);

        let hazard_node = Box::into_raw(Box::new(HazardNode {
            generation: count,
            node: removed,
            next: self.hazard_head.load(SeqCst),
        }));

        loop {
            let node_ref = unsafe { hazard_node.as_mut().unwrap_unchecked() };

            if self
                .hazard_head
                .compare_exchange(node_ref.next, hazard_node, SeqCst, SeqCst)
                .is_err()
            {
                node_ref.next = self.hazard_head.load(SeqCst);
                continue;
            }

            break;
        }

        self.hazard_threshold.fetch_add(1, SeqCst);

        Some(*removed_obj)
    }
}