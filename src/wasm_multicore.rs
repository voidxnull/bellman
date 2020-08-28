extern crate futures;

use std::fmt::Debug;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::channel::oneshot::{channel, Receiver, Sender};
use futures::executor::block_on;
use futures::future::lazy;
use futures::TryFutureExt;
use rayon::{Scope, ThreadPool};
use web_worker::{new_thread_pool, WorkerPool};

pub struct Worker {
    pool: WorkerPool,
    cpus: usize,
}

impl Worker {
    // We don't expose this outside the library so that
    // all `Worker` instances have the same number of
    // CPUs configured.
    pub(crate) fn new_with_cpus(cpus: usize) -> Worker {
        let pool = WorkerPool::new(cpus).unwrap();
        Worker { pool, cpus }
    }

    pub fn new() -> Worker {
        let cpus = web_sys::window()
            .expect("Failed to get hardware concurrency from window. This function is only available in the main browser thread.")
            .navigator()
            .hardware_concurrency() as usize;
        Self::new_with_cpus(cpus)
    }

    pub fn log_num_cpus(&self) -> u32 {
        log2_floor(self.cpus)
    }

    // pub fn spawn<I, E, F, R>(&self, elements: I, f: F) -> R
    // where
    //     I: IntoIterator<Item = E>,
    //     F: FnOnce(&[(usize, E)]) -> R,
    // {
    //     let iter = elements.into_iter();
    //     let num_elems = iter.count();
    //     let chunk_size = if num_elems < self.cpus {
    //         1
    //     } else {
    //         elements / self.cpus
    //     };
    //
    //     for chunk in elements.into_iter().chunks_mut() {
    //         self.pool.exec
    //     }
    // }

    pub fn compute<F, T, E>(&self, f: F) -> impl Future<Output = Result<T, E>>
    where
        F: FnOnce() -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: Debug + Send + 'static,
    {
        self.pool.run_notify(f).unwrap()
    }

    pub fn scope<'a, F, R>(&self, elements: usize, f: F) -> R
    where
        R: Send + 'static,
        F: Send + FnOnce(&Scope<'a>, usize) -> R,
    {
        let chunk_size = if elements < self.cpus() {
            1
        } else {
            elements / self.cpus()
        };

        let pool = new_thread_pool(self.cpus, &self.pool);

        pool.scope(|scope| f(scope, chunk_size))
    }

    #[inline]
    fn cpus(&self) -> usize {
        self.cpus
    }
}

fn log2_floor(num: usize) -> u32 {
    assert!(num > 0);

    let mut pow = 0;

    while (1 << (pow + 1)) <= num {
        pow += 1;
    }

    pow
}

#[test]
fn test_log2_floor() {
    assert_eq!(log2_floor(1), 0);
    assert_eq!(log2_floor(2), 1);
    assert_eq!(log2_floor(3), 1);
    assert_eq!(log2_floor(4), 2);
    assert_eq!(log2_floor(5), 2);
    assert_eq!(log2_floor(6), 2);
    assert_eq!(log2_floor(7), 2);
    assert_eq!(log2_floor(8), 3);
}

#[test]
fn test_trivial_spawning() {
    use self::futures::executor::block_on;

    fn long_fn() -> Result<usize, ()> {
        let mut i: usize = 1;
        println!("Start calculating long task");
        for _ in 0..1000000 {
            i = i.wrapping_mul(42);
        }

        println!("Done calculating long task");

        Ok(i)
    }

    let worker = Worker::new();
    println!("Spawning");
    let fut = worker.compute(|| long_fn());
    println!("Done spawning");

    println!("Will sleep now");

    std::thread::sleep(std::time::Duration::from_millis(10000));

    println!("Done sleeping");

    let _ = block_on(fut);
}
