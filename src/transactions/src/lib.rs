/*
  Copyright 2018 The Purple Library Authors
  This file is part of the Purple Library.

  The Purple Library is free software: you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  The Purple Library is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with the Purple Library. If not, see <http://www.gnu.org/licenses/>.
*/

#![feature(extern_prelude)]

#[macro_use] extern crate serde_derive;
#[macro_use] extern crate erased_serde;

extern crate rmp_serde as rmps;
extern crate serde;
extern crate causality;
extern crate crypto;
extern crate account;

mod transaction;
mod call;
mod genesis;
mod open_contract;
mod receive;
mod send;

pub use call::*;
pub use genesis::*;
pub use open_contract::*;
pub use receive::*;
pub use send::*;
pub use transaction::*;

#[derive(Serialize, Deserialize)]
pub enum Tx {
  Call(Call),
  OpenContract(OpenContract),
  Receive(Receive),
  Send(Send) 
}