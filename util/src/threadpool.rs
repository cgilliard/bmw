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

use crate::{ExecutionResponse, ThreadPool, ThreadPoolBuilder, ThreadPoolConfig};
use bmw_deps::futures::executor::block_on;
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::spawn;

info!();

struct FutureWrapper<T> {
	f: Pin<Box<dyn Future<Output = Result<T, Error>> + Send + Sync + 'static>>,
	tx: Sender<ExecutionResponse<T>>,
}

struct ThreadPoolImpl<T> {
	config: ThreadPoolConfig,
	rx: Option<Arc<Mutex<Receiver<FutureWrapper<T>>>>>,
	tx: Option<Sender<FutureWrapper<T>>>,
}

impl Default for ThreadPoolConfig {
	fn default() -> Self {
		Self { size: 10 }
	}
}

impl<T> ThreadPoolImpl<T> {
	fn new(config: ThreadPoolConfig) -> Result<Self, Error> {
		Ok(Self {
			config,
			rx: None,
			tx: None,
		})
	}
}

impl<T: 'static> ThreadPool<T> for ThreadPoolImpl<T> {
	fn execute<F>(&self, f: F) -> Result<Receiver<ExecutionResponse<T>>, Error>
	where
		F: Future<Output = Result<T, Error>> + Send + Sync + 'static,
	{
		let (tx, rx) = channel::<ExecutionResponse<T>>();
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
		for _ in 0..self.config.size {
			let rx = rx.clone();
			spawn(move || -> Result<(), Error> {
				loop {
					let rx = rx.clone();
					let jh = spawn(move || -> Result<(), Error> {
						loop {
							let next = {
								let rx = rx.lock()?;
								rx.recv()?
							};

							match block_on(next.f) {
								Ok(res) => {
									match next.tx.send(ExecutionResponse::Success(res)) {
										Ok(_) => {}
										Err(e) => {
											// supress this message in case user
											// ignores response, which is ok to do
											debug!("error sending response: {}", e)?;
										}
									}
								}
								Err(e) => {
									match next.tx.send(ExecutionResponse::Fail(e)) {
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

					let _ = jh.join();
					warn!("thread panic!")?;
				}
			});
		}
		Ok(())
	}
}
impl ThreadPoolBuilder {
	pub fn build<T: 'static>(config: ThreadPoolConfig) -> Result<impl ThreadPool<T>, Error> {
		Ok(ThreadPoolImpl::new(config)?)
	}
}

#[cfg(test)]
mod test {
	use crate::{ExecutionResponse, ThreadPool, ThreadPoolBuilder, ThreadPoolConfig};
	use bmw_err::{err, ErrKind, Error};
	use bmw_log::*;
	use std::thread::sleep;
	use std::time::Duration;

	info!();

	#[test]
	fn test_threadpool() -> Result<(), Error> {
		let mut tp = ThreadPoolBuilder::build(ThreadPoolConfig {
			size: 10,
			..Default::default()
		})?;
		tp.start()?;

		// simple execution, return value
		let res = tp.execute(async move { Ok(1) })?;
		assert_eq!(res.recv()?, ExecutionResponse::Success(1));

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

		assert_eq!(
			res.recv()?,
			ExecutionResponse::Fail(err!(ErrKind::Test, "test"))
		);

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
		assert_eq!(res.recv()?, ExecutionResponse::Success(5));

		Ok(())
	}
}
