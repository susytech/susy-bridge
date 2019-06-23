// Copyleft 2017 Superstring.Community
// This file is part of Susy-Bridge.

// Susy-Bridge is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Susy-Bridge is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MSRCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Susy-Bridge.  If not, see <http://www.gnu.org/licenses/>.
extern crate sofabi;
#[macro_use]
extern crate sofabi_derive;
#[macro_use]
extern crate sofabi_contract;

use_contract!(main, "../compiled_contracts/Main.abi");
use_contract!(side, "../compiled_contracts/Side.abi");
#[cfg(feature = "integration-tests")]
use_contract!(test, "../compiled_contracts/RecipientTest.abi");
