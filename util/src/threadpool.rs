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

use crate::{PoolResult, ThreadPool, ThreadPoolConfig};
use bmw_deps::dyn_clone::clone_box;
use bmw_deps::futures::executor::block_on;
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::spawn;

info!();

struct FutureWrapper<T> {
	f: Pin<Box<dyn Future<Output = Result<T, Error>> + Send + Sync + 'static>>,
	tx: SyncSender<PoolResult<T, Error>>,
}

pub(crate) struct ThreadPoolImpl<T: 'static + Send + Sync> {
	config: ThreadPoolConfig,
	rx: Option<Arc<Mutex<Receiver<FutureWrapper<T>>>>>,
	tx: Option<SyncSender<FutureWrapper<T>>>,
	state: Box<dyn LockBox<ThreadPoolState>>,
	test_config: Option<TestConfig>,
}

impl Default for ThreadPoolConfig {
	fn default() -> Self {
		Self {
			min_size: 3,
			max_size: 7,
			sync_channel_size: 7,
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

pub(crate) struct TestConfig {
	debug_drop_error: bool,
}

impl<T: 'static + Send + Sync> Drop for ThreadPoolImpl<T> {
	fn drop(&mut self) {
		let stop = self.stop();
		if stop.is_err() {
			let e = stop.unwrap_err();
			let _ = error!("unexpected error calling drop: {}", e);
		}
	}
}

impl<T: 'static + Send + Sync> ThreadPoolImpl<T> {
	pub(crate) fn new(
		config: ThreadPoolConfig,
		test_config: Option<TestConfig>,
	) -> Result<Self, Error> {
		if config.min_size == 0 || config.min_size > config.max_size {
			let fmt = "min_size must be > 0 and < max_size";
			return Err(err!(ErrKind::Configuration, fmt));
		}
		let waiting = 0;
		let cur_size = config.min_size;
		let config_clone = config.clone();
		let stop = false;
		let tps = ThreadPoolState {
			waiting,
			cur_size,
			config,
			stop,
		};
		let state = lock_box!(tps)?;

		let config = config_clone;
		let rx = None;
		let tx = None;

		let ret = Self {
			config,
			tx,
			rx,
			state,
			test_config,
		};
		Ok(ret)
	}

	// this function appears to have full test coverage, disable tarpaulin for now
	#[cfg(not(tarpaulin_include))]
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
								let send_res = next.tx.send(PoolResult::Ok(res));
								if send_res.is_err() {
									let e = send_res.unwrap_err();
									debug!("error sending response: {}", e)?;
								}
							}
							Err(e) => {
								debug!("sending an errpre")?;
								let send_res = next.tx.send(PoolResult::Err(e));
								debug!("sending an err")?;
								if send_res.is_err() {
									let e = send_res.unwrap_err();
									debug!("error sending response: {}", e)?;
								} else {
									debug!("sent response ok")?;
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

impl<T: 'static + Send + Sync> ThreadPool<T> for ThreadPoolImpl<T> {
	fn execute<F>(&self, f: F) -> Result<Receiver<PoolResult<T, Error>>, Error>
	where
		F: Future<Output = Result<T, Error>> + Send + Sync + 'static,
	{
		if self.tx.is_none() {
			let fmt = "Thread pool has not been initialized";
			return Err(err!(ErrKind::IllegalState, fmt));
		}

		let (tx, rx) = sync_channel::<PoolResult<T, Error>>(self.config.max_size);
		let fw = FutureWrapper { f: Box::pin(f), tx };
		self.tx.as_ref().unwrap().send(fw)?;
		Ok(rx)
	}
	fn start(&mut self) -> Result<(), Error> {
		let (tx, rx) = sync_channel(self.config.sync_channel_size);
		let rx = Arc::new(Mutex::new(rx));
		self.rx = Some(rx.clone());
		self.tx = Some(tx.clone());
		for _ in 0..self.config.min_size {
			Self::run_thread(rx.clone(), clone_box(&*self.state))?;
		}
		Ok(())
	}

	fn stop(&mut self) -> Result<(), Error> {
		if self.test_config.is_some() && self.test_config.as_ref().unwrap().debug_drop_error {
			return Err(err!(ErrKind::Test, "debug stop"));
		}
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

#[cfg(test)]
mod test {
	use crate::execute;
	use crate::threadpool::TestConfig;
	use crate::threadpool::ThreadPoolImpl;
	use crate::{Builder, PoolResult, ThreadPool, ThreadPoolConfig};
	use bmw_deps::dyn_clone::clone_box;
	use bmw_err::{err, ErrKind, Error};
	use bmw_log::*;
	use std::thread::sleep;
	use std::time::Duration;

	info!();

	#[test]
	fn test_threadpool1() -> Result<(), Error> {
		let mut tp = Builder::build_thread_pool(ThreadPoolConfig {
			min_size: 10,
			max_size: 10,
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
			count += 1;
			assert!(count < 500);
			sleep(Duration::from_millis(10));
			if **x_clone.rlock()?.guard() == 2 {
				break;
			}
		}

		// return an error
		let res = tp.execute(async move { Err(err!(ErrKind::Test, "test")) })?;

		assert_eq!(res.recv()?, PoolResult::Err(err!(ErrKind::Test, "test")));

		// handle panic
		let res = tp.execute(async move {
			let x: Option<u32> = None;
			Ok(x.unwrap())
		})?;

		assert!(res.recv().is_err());

		// 10 more panics to ensure pool keeps running
		for _ in 0..10 {
			let res = tp.execute(async move {
				let x: Option<u32> = None;
				Ok(x.unwrap())
			})?;

			assert!(res.recv().is_err());
		}

		// now do a regular request
		let res = tp.execute(async move { Ok(5) })?;
		assert_eq!(res.recv()?, PoolResult::Ok(5));

		sleep(Duration::from_millis(1000));
		info!("test sending errors")?;

		// send an error and ignore the response
		{
			let res = tp.execute(async move { Err(err!(ErrKind::Test, "")) })?;
			drop(res);
			sleep(Duration::from_millis(1000));
		}
		{
			let res = tp.execute(async move { Err(err!(ErrKind::Test, "test")) })?;
			assert_eq!(res.recv()?, PoolResult::Err(err!(ErrKind::Test, "test")));
		}
		sleep(Duration::from_millis(1_000));

		Ok(())
	}

	#[test]
	fn test_sizing() -> Result<(), Error> {
		let mut tp = ThreadPoolImpl::new(
			ThreadPoolConfig {
				min_size: 2,
				max_size: 4,
				..Default::default()
			},
			None,
		)?;
		tp.start()?;
		let mut v = vec![];

		let x = lock!(0)?;

		loop {
			{
				let state = tp.state.rlock()?;
				if (**state.guard()).waiting == 2 {
					break;
				}
			}
			sleep(Duration::from_millis(100));
		}
		assert_eq!(tp.size()?, 2);
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
		loop {
			{
				let state = tp.state.rlock()?;
				if (**state.guard()).waiting == 1 {
					break;
				}
				info!("waiting = {}", (**state.guard()).waiting)?;
			}
			sleep(Duration::from_millis(100));
		}
		assert_eq!(tp.size()?, 3);

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
		let mut tp = Builder::build_thread_pool(ThreadPoolConfig {
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
	fn test_threadpool_drop() -> Result<(), Error> {
		let state;
		{
			let mut tp = ThreadPoolImpl::new(
				ThreadPoolConfig {
					min_size: 2,
					max_size: 4,
					..Default::default()
				},
				None,
			)?;
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
		let state_guard = state.rlock()?;
		assert_eq!((**state_guard.guard()).cur_size, 0);
		assert_eq!((**state_guard.guard()).stop, true);
		let state;
		{
			let mut tp = ThreadPoolImpl::new(
				ThreadPoolConfig {
					min_size: 2,
					max_size: 4,
					..Default::default()
				},
				Some(TestConfig {
					debug_drop_error: true,
				}),
			)?;
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
		let state_guard = state.rlock()?;

		// other threads die due to the channel being closed so we get to 0 still.
		assert_eq!((**state_guard.guard()).cur_size, 0);
		assert_eq!((**state_guard.guard()).stop, false);

		Ok(())
	}

	#[test]
	fn pass_to_threads() -> Result<(), Error> {
		let mut tp = Builder::build_thread_pool(ThreadPoolConfig {
			min_size: 2,
			max_size: 4,
			..Default::default()
		})?;
		tp.start()?;

		let tp = lock!(tp)?;
		for _ in 0..6 {
			let tp = tp.clone();
			std::thread::spawn(move || -> Result<(), Error> {
				let tp = tp.rlock()?;
				execute!((**tp.guard()), {
					info!("executing in thread pool")?;
					Ok(1)
				})?;
				Ok(())
			});
		}
		Ok(())
	}

	#[test]
	fn test_bad_configs() -> Result<(), Error> {
		assert!(Builder::build_thread_pool::<()>(ThreadPoolConfig {
			min_size: 5,
			max_size: 4,
			..Default::default()
		})
		.is_err());

		assert!(Builder::build_thread_pool::<()>(ThreadPoolConfig {
			min_size: 0,
			max_size: 4,
			..Default::default()
		})
		.is_err());

		let tp = Builder::build_thread_pool::<()>(ThreadPoolConfig {
			min_size: 5,
			max_size: 6,
			..Default::default()
		});
		assert!(tp.is_ok());
		let tp = tp.unwrap();

		assert_eq!(
			tp.execute(async move { Ok(()) }).unwrap_err(),
			err!(
				ErrKind::IllegalState,
				"Thread pool has not been initialized"
			)
		);
		Ok(())
	}
}
