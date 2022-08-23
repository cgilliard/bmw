// Copyright (c) 2022, 37 Miners, LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{PoolResult, ThreadPool, ThreadPoolBuilder, ThreadPoolConfig};
use bmw_deps::dyn_clone::clone_box;
use bmw_deps::futures::executor::block_on;
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::spawn;

info!();

struct FutureWrapper<T> {
	f: Pin<Box<dyn Future<Output = Result<T, Error>> + Send + Sync + 'static>>,
	tx: SyncSender<PoolResult<T, Error>>,
}

struct ThreadPoolImpl<T: 'static> {
	config: ThreadPoolConfig,
	rx: Option<Arc<Mutex<Receiver<FutureWrapper<T>>>>>,
	tx: Option<Sender<FutureWrapper<T>>>,
	state: Box<dyn LockBox<ThreadPoolState>>,
}

impl Default for ThreadPoolConfig {
	fn default() -> Self {
		Self {
			min_size: 3,
			max_size: 7,
		}
	}
}

#[derive(Debug, Clone)]
struct ThreadPoolState {
	waiting: usize,
	cur_size: usize,
	config: ThreadPoolConfig,
	stop: bool,
}

impl<T: 'static> Drop for ThreadPoolImpl<T> {
	fn drop(&mut self) {
		match self.stop() {
			Ok(_) => {}
			Err(e) => {
				let _ = error!("unexpected error calling drop: {}", e);
			}
		}
	}
}

impl<T: 'static> ThreadPoolImpl<T> {
	fn new(config: ThreadPoolConfig) -> Result<Self, Error> {
		let state = lock_box!(ThreadPoolState {
			waiting: 0,
			cur_size: config.min_size,
			config: config.clone(),
			stop: false,
		})?;
		Ok(Self {
			config,
			rx: None,
			tx: None,
			state,
		})
	}

	fn run_thread<R: 'static>(
		rx: Arc<Mutex<Receiver<FutureWrapper<R>>>>,
		mut state: Box<dyn LockBox<ThreadPoolState>>,
	) -> Result<(), Error> {
		spawn(move || -> Result<(), Error> {
			loop {
				let rx = rx.clone();
				let mut state_clone = clone_box(&*state);
				let jh = spawn(move || -> Result<(), Error> {
					loop {
						let (next, do_run_thread) = {
							let mut do_run_thread = false;
							{
								let mut state = state_clone.wlock()?;
								let guard = &mut **state.guard();

								debug!("state = {:?}", guard)?;
								// we have too many threads. Exit
								// this one.
								if guard.stop || guard.waiting >= guard.config.min_size {
									return Ok(());
								}
								guard.waiting += 1;
							}
							let rx = rx.lock()?;
							let ret = rx.recv()?;
							{
								let mut state = state_clone.wlock()?;
								let guard = &mut **state.guard();
								guard.waiting = guard.waiting.saturating_sub(1);
								if guard.waiting == 0 {
									if guard.cur_size < guard.config.max_size {
										guard.cur_size += 1;
										do_run_thread = true;
									}
								}
								debug!("cur state = {:?}", guard)?;
							}
							(ret, do_run_thread)
						};

						if do_run_thread {
							debug!("spawning a new thread")?;
							Self::run_thread(rx.clone(), clone_box(&*state_clone))?;
						}

						match block_on(next.f) {
							Ok(res) => {
								match next.tx.send(PoolResult::Ok(res)) {
									Ok(_) => {}
									Err(e) => {
										// supress this message in case user
										// ignores response, which is ok to do
										debug!("error sending response: {}", e)?;
									}
								}
							}
							Err(e) => {
								match next.tx.send(PoolResult::Err(e)) {
									Ok(_) => {}
									Err(e) => {
										// supress this message in case user
										// ignores response, which is ok to do
										debug!("error sending response: {}", e)?;
									}
								}
							}
						}
					}
				});

				match jh.join() {
					Ok(_) => {
						let mut state = state.wlock()?;
						let guard = &mut **state.guard();
						guard.cur_size = guard.cur_size.saturating_sub(1);
						debug!("exiting a thread, ncur={}", guard.cur_size)?;
						break;
					} // reduce thread count so exit this one
					Err(e) => warn!("thread panic: {:?}", e)?,
				}
			}
			Ok(())
		});
		Ok(())
	}
}

impl<T: 'static> ThreadPool<T> for ThreadPoolImpl<T> {
	fn execute<F>(&self, f: F) -> Result<Receiver<PoolResult<T, Error>>, Error>
	where
		F: Future<Output = Result<T, Error>> + Send + Sync + 'static,
	{
		let (tx, rx) = sync_channel::<PoolResult<T, Error>>(self.config.max_size);
		let fw = FutureWrapper { f: Box::pin(f), tx };
		match &self.tx {
			Some(tx) => tx.send(fw)?,
			None => {
				return Err(err!(
					ErrKind::IllegalState,
					"Thread pool has not been initialized"
				));
			}
		}
		Ok(rx)
	}
	fn start(&mut self) -> Result<(), Error> {
		let (tx, rx) = channel();
		let rx = Arc::new(Mutex::new(rx));
		self.rx = Some(rx.clone());
		self.tx = Some(tx.clone());
		for _ in 0..self.config.min_size {
			Self::run_thread(rx.clone(), clone_box(&*self.state))?;
		}
		Ok(())
	}

	fn stop(&mut self) -> Result<(), Error> {
		let mut state = self.state.wlock()?;
		(**state.guard()).stop = true;
		self.tx = None;
		Ok(())
	}

	fn size(&self) -> Result<usize, Error> {
		let state = self.state.rlock()?;
		Ok((**state.guard()).cur_size)
	}
}

impl ThreadPoolBuilder {
	/// Build a [`crate::ThreadPool`] based on the specified [`crate::ThreadPoolConfig`]. Note
	/// that the thread pool will not be usable until the [`crate::ThreadPool::start`] function
	/// has been called.
	pub fn build<T: 'static>(config: ThreadPoolConfig) -> Result<impl ThreadPool<T>, Error> {
		Ok(ThreadPoolImpl::new(config)?)
	}
}

#[cfg(test)]
mod test {
	use crate::threadpool::ThreadPoolImpl;
	use crate::{PoolResult, ThreadPool, ThreadPoolBuilder, ThreadPoolConfig};
	use bmw_deps::dyn_clone::clone_box;
	use bmw_err::{err, ErrKind, Error};
	use bmw_log::*;
	use std::thread::sleep;
	use std::time::Duration;

	info!();

	#[test]
	fn test_threadpool() -> Result<(), Error> {
		let mut tp = ThreadPoolBuilder::build(ThreadPoolConfig {
			min_size: 10,
			..Default::default()
		})?;
		tp.start()?;

		// simple execution, return value
		let res = tp.execute(async move { Ok(1) })?;
		assert_eq!(res.recv()?, PoolResult::Ok(1));

		// increment value using locks
		let mut x = lock!(1)?;
		let x_clone = x.clone();
		tp.execute(async move {
			**x.wlock()?.guard() = 2;
			Ok(1)
		})?;

		let mut count = 0;
		loop {
			sleep(Duration::from_millis(10));
			if **x_clone.rlock()?.guard() == 2 {
				break;
			}
			count += 1;
			assert!(count < 500);
		}

		// return an error
		let res = tp.execute(async move { Err(err!(ErrKind::Test, "test")) })?;

		assert_eq!(res.recv()?, PoolResult::Err(err!(ErrKind::Test, "test")));

		// handle panic
		let res = tp.execute(async move {
			let x: Option<u32> = None;
			let _ = x.unwrap();
			Ok(1)
		})?;

		assert!(res.recv().is_err());

		// 10 more panics to ensure pool keeps running
		for _ in 0..10 {
			let res = tp.execute(async move {
				let x: Option<u32> = None;
				let _ = x.unwrap();
				Ok(1)
			})?;

			assert!(res.recv().is_err());
		}

		// now do a regular request
		let res = tp.execute(async move { Ok(5) })?;
		assert_eq!(res.recv()?, PoolResult::Ok(5));

		Ok(())
	}

	#[test]
	fn test_sizing() -> Result<(), Error> {
		let mut tp = ThreadPoolBuilder::build(ThreadPoolConfig {
			min_size: 2,
			max_size: 4,
			..Default::default()
		})?;
		tp.start()?;
		sleep(Duration::from_millis(1000));
		let mut v = vec![];

		let x = lock!(0)?;

		// first use up all the min_size threads
		for _ in 0..2 {
			let x_clone = x.clone();
			let res = tp.execute(async move {
				loop {
					if **(x_clone.rlock()?.guard()) != 0 {
						break;
					}
					sleep(Duration::from_millis(50));
				}
				Ok(1)
			})?;
			v.push(res);
		}
		assert_eq!(tp.size()?, 2);

		// confirm we can still process
		for _ in 0..10 {
			let mut x_clone = x.clone();
			let res = tp.execute(async move {
				**(x_clone.wlock()?.guard()) = 1;

				Ok(2)
			})?;
			assert_eq!(res.recv()?, PoolResult::Ok(2));
		}

		sleep(Duration::from_millis(2_000));
		assert_eq!(tp.size()?, 2);

		let mut i = 0;
		for res in v {
			assert_eq!(res.recv()?, PoolResult::Ok(1));
			info!("res complete {}", i)?;
			i += 1;
		}

		sleep(Duration::from_millis(1000));

		let mut x2 = lock!(0)?;

		// confirm that the maximum is in place

		// block all 4 threads waiting on x2
		for _ in 0..4 {
			let mut x2_clone = x2.clone();
			tp.execute(async move {
				info!("x0a")?;
				loop {
					if **(x2_clone.rlock()?.guard()) != 0 {
						break;
					}
					sleep(Duration::from_millis(50));
				}
				info!("x2")?;
				**(x2_clone.wlock()?.guard()) += 1;

				Ok(0)
			})?;
		}

		sleep(Duration::from_millis(2_000));
		assert_eq!(tp.size()?, 4);

		// confirm that the next thread cannot be processed
		let mut x2_clone = x2.clone();
		tp.execute(async move {
			info!("x0")?;
			**(x2_clone.wlock()?.guard()) += 1;
			info!("x1")?;
			Ok(0)
		})?;

		// wait
		sleep(Duration::from_millis(2_000));

		// confirm situation hasn't changed
		assert_eq!(**(x2.rlock()?.guard()), 0);

		// unlock the threads by setting x2 to 1
		**(x2.wlock()?.guard()) = 1;

		// wait
		sleep(Duration::from_millis(4_000));
		assert_eq!(**(x2.rlock()?.guard()), 6);
		info!("exit all")?;

		sleep(Duration::from_millis(2_000));
		assert_eq!(tp.size()?, 2);

		tp.stop()?;
		sleep(Duration::from_millis(2_000));
		assert_eq!(tp.size()?, 0);

		Ok(())
	}

	#[test]
	fn test_stop() -> Result<(), Error> {
		let mut tp = ThreadPoolBuilder::build(ThreadPoolConfig {
			min_size: 2,
			max_size: 4,
			..Default::default()
		})?;
		tp.start()?;

		sleep(Duration::from_millis(1000));
		assert_eq!(tp.size()?, 2);
		tp.execute(async move { Ok(()) })?;

		sleep(Duration::from_millis(1000));
		info!("stopping pool")?;
		assert_eq!(tp.size()?, 2);
		tp.stop()?;

		sleep(Duration::from_millis(1000));
		assert_eq!(tp.size()?, 0);
		Ok(())
	}

	#[test]
	fn test_drop() -> Result<(), Error> {
		let state;
		{
			let mut tp = ThreadPoolImpl::new(ThreadPoolConfig {
				min_size: 2,
				max_size: 4,
				..Default::default()
			})?;
			state = clone_box(&*tp.state);
			tp.start()?;
			sleep(Duration::from_millis(1000));
			assert_eq!(tp.size()?, 2);
			tp.execute(async move { Ok(()) })?;

			sleep(Duration::from_millis(1000));
			info!("stopping pool")?;
			assert_eq!(tp.size()?, 2);
		}
		sleep(Duration::from_millis(1000));
		let state = state.rlock()?;
		assert_eq!((**state.guard()).cur_size, 0);
		Ok(())
	}
}
