# atomic lifo
Lock free thread-safe lifo for rust.

## Example
```rust
use std::thread;
use atomic_lifo::AtomicLifo;

static MT_LIFO: AtomicLifo<u32> = AtomicLifo::new();

#[test]
pub fn example() {
    MT_LIFO.push(456);
    MT_LIFO.push(123);
    let th = {
        thread::spawn(move || {
            assert_eq!(MT_LIFO.pop(), Some(123));
            assert_eq!(MT_LIFO.pop(), Some(456));
            assert_eq!(MT_LIFO.pop(), None);
        })
    };

    th.join().unwrap();
}
```
## When to use this crate?
The implementation in this crate is far from optimized and likely to be slower than a `std::sync:mpsc::channel()`,
It was developed for WASM32 with multi threading enabled.
WASM32 has the peculiarity that the main thread is NEVER allowed to block on for example a Mutex.
In addition, there is also no implementation of `std::sync:mpsc::channel()` for WASM32.

This crate serves as my "we have a mpsc channel at home" solution for the absence of a proper `std::sync::mpsc::channel()`
that works with the main thread of a Wasm32 application.

Since this crate uses `no_std` it may also have uses outside the wasm32 use case.
If you have access to a proper implementation of the rust std library 
and do not have to deal with mutexes not being able to block
on some threads like in the wasm32 case then you should not use this crate.

## What algorithm does this crate use?
Essentially a single linked list (or stack) that uses compare and swap to push and pop nodes.

The nodes themselves are freed using the usual hazard pointer techniques.

I use only a single hazard list that counts generations of nodes. 
All nodes that are removed concurrently have the same generation and are only removed when the entire generation has concluded.
The generation for newly removed nodes is incremented every time the concurrent access count drops to 0 ensuring
that no concurrent access to nodes of a concluded generation is still possible.

The hazard list itself is also an internal compare and swap lifo that uses a AtomicBool to ensure mutual exclusion
when freeing its own nodes. 
The hazard list free routine is invoked whenever a thread increments the generation which occurs on a call to `pop()`.

## Is it truly lock free?
No, it has 1 spin lock/loop for an edge case that shouldn't occur unless you want it to.

Basically when many threads constantly call `pop()` the access count can technically never become 0 causing
the generation to never be incremented. This is not a problem unless `pop()` actually removes a lot of elements.
This would cause the hazard list to keep growing until we run out of memory. Therefore, we maintain a counter of the 
amount of elements added to the hazard list since the last time we incremented the generation. Should this counter
exceed a reasonably large threshold then all threads that call `pop()` will spin until the generation has concluded and
therefore a lot of elements on the hazard list were freed.

I was not able to force this to happen even in synthetic tests.

## Does this crate have UB or Memory Leaks?
Miri and Valgrind say that it does not have UB or Memory Leaks, but that is not a 100% guarantee.
If you find a mistake I made when implementing this data structure then I would appreciate feedback as previously 
I have only implemented such data structures on languages with a garbage collector. Writing this crate
was my first experience with using "hazard pointers/lists" or as some people call them "gc at home".