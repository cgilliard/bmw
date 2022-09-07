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

use crate::types::EventHandlerImpl;
use crate::{Builder, ConnectionData, EventHandler, EventHandlerConfig, ThreadContext};
use bmw_err::*;

impl Builder {
	pub fn build_evh<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>(
		config: EventHandlerConfig,
	) -> Result<impl EventHandler<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>, Error>
	where
		OnRead: Fn(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
			+ Send
			+ 'static
			+ Clone
			+ Sync
			+ Unpin,
		OnAccept: Fn(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
			+ Send
			+ 'static
			+ Clone
			+ Sync
			+ Unpin,
		OnClose: Fn(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
			+ Send
			+ 'static
			+ Clone
			+ Sync
			+ Unpin,
		HouseKeeper:
			Fn(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
		OnPanic:
			Fn(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
	{
		EventHandlerImpl::new(config)
	}
}
