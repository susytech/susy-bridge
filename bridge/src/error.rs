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

//! error chain

use std::io;
use tokio_timer::{TimeoutError, TimerError};
use {sofabi, rustc_hex, toml, susyweb};

error_chain! {
    types {
        Error, ErrorKind, ResultExt, Result;
    }

    foreign_links {
        Io(io::Error);
        Toml(toml::de::Error);
        Sofabi(sofabi::Error);
        Timer(TimerError);
        Hex(rustc_hex::FromHexError);
    }

    errors {
        TimedOut {
            description("Request timed out"),
            display("Request timed out"),
        }
        // workaround for error_chain not allowing to check internal error kind
        // https://github.com/rust-lang-nursery/error-chain/issues/206
        MissingFile(filename: String) {
            description("File not found"),
            display("File {} not found", filename),
        }
        // workaround for lack of susyweb:Error Display and Error implementations
        SusyWeb(err: susyweb::Error) {
            description("susyweb error"),
            display("{:?}", err),
        }
    }
}

// tokio timer `Timeout<F>` can only wrap futures `F` whose assocaited `Error` type
// satisfies `From<TimeoutError<F>>`
//
// `susyweb::CallResult`'s associated error type `Error` which is `susyweb::Error`
// does not satisfy `From<TimeoutError<F>>`.
// thus we can't use `Timeout<susyweb::CallResult>`.
// we also can't implement `From<TimeoutError<F>` for `susyweb::Error` since
// we control neither of the types.
//
// instead we implement `TimeoutError<F>` for `Error` and `From<susyweb::Error>`
// for `Error` so we can convert `susyweb::Error` into `Error` and then use that
// with `Timeout`.

impl<F> From<TimeoutError<F>> for Error {
    fn from(err: TimeoutError<F>) -> Self {
        match err {
            TimeoutError::Timer(_, timer_error) => timer_error.into(),
            TimeoutError::TimedOut(_) => ErrorKind::TimedOut.into(),
        }
    }
}

impl From<susyweb::Error> for Error {
    fn from(err: susyweb::Error) -> Self {
        ErrorKind::SusyWeb(err).into()
    }
}
