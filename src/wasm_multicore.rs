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
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use futures::channel::oneshot::{channel, Receiver, Sender};
use futures::executor::block_on;
use futures::future::{join_all, lazy};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;
use wasm_mt::prelude::*;
use wasm_mt::{exec, Thread};

#[derive(Clone)]
pub struct Worker {
    cpus: usize,
    mt: Rc<WasmMt>,
    threads: Rc<Vec<Thread>>,
    next_thread_idx: usize,
}

impl Worker {
    pub(crate) async fn new_with_cpus(cpus: usize) -> Worker {
        let mt = WasmMt::new("./pkg/parallel.js").and_init().await.unwrap();
        let mut threads = Vec::with_capacity(cpus);
        for n in 0..cpus {
            let thread = mt.thread().and_init().await.unwrap();
            thread.set_id(&n.to_string());
            threads.push(thread);
        }

        Worker {
            cpus,
            mt: Rc::new(mt),
            threads: Rc::new(threads),
            next_thread_idx: 0,
        }
    }

    pub fn new() -> Worker {
        let cpus = web_sys::window()
            .expect("Failed to get hardware concurrency from window. This function is only available in the main browser thread.")
            .navigator()
            .hardware_concurrency() as usize;

        block_on(Self::new_with_cpus(cpus))
    }

    pub fn log_num_cpus(&self) -> u32 {
        self.cpus as u32
    }

    // pub async fn compute<F, T, E>(&self, func: F) -> Result<T, E>
    // where
    //     F: FnOnce() -> Result<T, E> + Send + 'static,
    //     T: Serialize + DeserializeOwned + Send + 'static,
    //     E: Serialize + DeserializeOwned + Send + 'static,
    // {
    //     let func_2 = FnOnce!(move || {
    //         let func = func;
    //         func()
    //     });
    //
    //     self.compute_(func_2).await
    // }
    //
    // async fn compute_<F, T, E>(&self, func: F) -> Result<T, E>
    // where
    //     F: FnOnce() -> Result<T, E> + Serialize + DeserializeOwned + Send + 'static,
    //     T: Serialize + DeserializeOwned + Send + 'static,
    //     E: Serialize + DeserializeOwned + Send + 'static,
    // {
    //     let thread = &self.threads[self.next_thread_idx];
    //     let res = exec!(thread, move || {
    //         let func = func;
    //         func()
    //             .map(|val| JsValue::from_serde(&val).unwrap())
    //             .map_err(|err| JsValue::from_serde(&err).unwrap())
    //     })
    //     .await
    //     .map(|val| val.into_serde().unwrap())
    //     .map_err(|err| err.into_serde().unwrap());
    //
    //     // spawn_local(async move {
    //     //     let res = pool_inner.execute_async(f).await.unwrap();
    //     //     let res = res.into_serde();
    //     //
    //     //     if !sender.is_canceled() {
    //     //         let _ = sender.send(res);
    //     //     }
    //     // });
    //     //
    //     // WorkerFuture { receiver }
    //
    //     res
    // }

    pub fn compute<F, T, E>(&self, f: F) -> WorkerFuture<T, E>
    where
        F: FnOnce() -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
    {
        let result = f();

        let (sender, receiver) = channel();
        let _ = sender.send(result);

        let worker_future = WorkerFuture { receiver };

        worker_future
    }

    pub fn scope<'a, F, R>(&self, elements: usize, f: F) -> R
    where
        F: FnOnce(&Scope<'a>, usize) -> R,
    {
        let chunk_size = if elements == 0 { 1 } else { elements };

        let scope = Scope {
            _marker: PhantomData,
        };

        f(&scope, chunk_size)
    }

    pub fn parallel_map<F, E, R>(&self, elements: Vec<E>, f: F) -> Vec<R>
    where
        R: Serialize + DeserializeOwned,
        F: Fn(usize, Vec<E>) -> Vec<R> + Clone + Serialize + DeserializeOwned + 'static,
        E: Serialize + DeserializeOwned + Clone + 'static,
    {
        let num_elems = elements.len();
        let chunk_size = if num_elems < self.cpus {
            1
        } else {
            num_elems / self.cpus
        };

        let mut buf = Vec::new();

        let futures = elements.chunks(chunk_size).enumerate().map(|(chunk_idx, chunk)| {
            let chunk = chunk.to_vec();
            let f = f.clone();
            exec!(&self.threads[n], move || {
                let f = f;
                let res = JsValue::from_serde(f(chunk_idx, chunk)).unwrap(); // FIXME: unwraps
                Ok(res)
            })
        });

        for results in block_on(join_all(futures)) {
            let results: Vec<R> = results.unwrap().into_serde().unwrap(); // FIXME: unwraps
            buf.extend(results);
        }

        buf
    }
}

#[derive(Clone)]
pub struct Scope<'a> {
    _marker: PhantomData<&'a usize>,
}

impl<'a> Scope<'a> {
    pub fn spawn<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Scope<'a>) -> R,
    {
        f(&self)
    }
}

pub struct WorkerFuture<T, E> {
    receiver: Receiver<Result<T, E>>,
}

impl<T: Send + 'static, E: Send + 'static> Future for WorkerFuture<T, E> {
    type Output = Result<T, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let rec = unsafe { self.map_unchecked_mut(|s| &mut s.receiver) };
        match rec.poll(cx) {
            Poll::Ready(v) => {
                if let Ok(v) = v {
                    return Poll::Ready(v);
                } else {
                    panic!("Worker future can not have canceled sender");
                }
            }
            Poll::Pending => {
                return Poll::Pending;
            }
        }
    }
}

impl<T: Send + 'static, E: Send + 'static> WorkerFuture<T, E> {
    pub fn wait(self) -> <Self as Future>::Output {
        block_on(self)
    }
}
