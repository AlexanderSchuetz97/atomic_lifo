use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::SeqCst;
use std::thread;
use std::time::Duration;
use atomic_lifo::AtomicLifo;

static MT_LIFO3: AtomicLifo<u32> = AtomicLifo::new();

#[cfg(test)]
#[test]
pub fn test_mt4_2() {
    let stop = Arc::new(AtomicBool::new(false));
    let mut jh = Vec::new();
    for _ in 0..4 {
        let th1 = {
            let stop_clone = Arc::clone(&stop);
            thread::spawn(move || loop {
                if stop_clone.load(SeqCst) {
                    return;
                }

                if let Some(data) = MT_LIFO3.pop() {
                    assert_eq!(data, 123456);
                }

                thread::yield_now();
            })
        };

        jh.push(th1);
    }

    for _ in 0..2 {
        let th2 = {
            let stop_clone = Arc::clone(&stop);
            thread::spawn(move || loop {
                if stop_clone.load(SeqCst) {
                    return;
                }
                MT_LIFO3.push(123456);
                thread::yield_now();
            })
        };

        jh.push(th2);
    }

    thread::sleep(Duration::from_secs(15));
    stop.store(true, SeqCst);
    for jh in jh {
        jh.join().unwrap();
    }
}