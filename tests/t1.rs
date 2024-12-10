use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use atomic_lifo::AtomicLifo;

#[test]
pub fn test() {
    let lifo = AtomicLifo::<String>::new();
    assert_eq!(lifo.pop(), None);
    lifo.push(String::from("test1"));
    lifo.push(String::from("test2"));
    lifo.push(String::from("test3"));
    assert_eq!(lifo.pop().unwrap(), String::from("test3"));
    assert_eq!(lifo.pop().unwrap(), String::from("test2"));
    assert_eq!(lifo.pop().unwrap(), String::from("test1"));
    assert_eq!(lifo.pop(), None);
    assert_eq!(lifo.pop(), None);
}

#[test]
pub fn test_drop_with_elements() {
    let lifo = AtomicLifo::<String>::new();
    assert_eq!(lifo.pop(), None);
    lifo.push(String::from("test1"));
    lifo.push(String::from("test2"));
    lifo.push(String::from("test3"));
    lifo.push(String::from("test4"));
    assert_eq!(lifo.pop().unwrap(), String::from("test4"));
    drop(lifo);
}


static MT_LIFO: AtomicLifo<u32> = AtomicLifo::new();

#[cfg(test)]
#[test]
pub fn test_mt() {
    let stop = Arc::new(AtomicBool::new(false));
    let th1 = {
        let stop_clone = Arc::clone(&stop);
        thread::spawn(move || loop {
            if stop_clone.load(SeqCst) {
                return;
            }

            if let Some(data) = MT_LIFO.pop() {
                assert_eq!(data, 123456);
            }
            thread::yield_now();
        })
    };

    let th2 = {
        let stop_clone = Arc::clone(&stop);
        thread::spawn(move || loop {
            if stop_clone.load(SeqCst) {
                return;
            }
            MT_LIFO.push(123456);
            thread::yield_now();
        })
    };

    thread::sleep(Duration::from_secs(15));
    stop.store(true, SeqCst);
    th1.join().unwrap();
    th2.join().unwrap();
}


