use atomic_lifo::AtomicLifo;
use std::thread;

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
