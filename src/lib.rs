#![forbid(unsafe_code)]
#![warn(clippy::all)]
// new is just more readable than ..Default::default().
#![allow(clippy::new_without_default)]
// the matches! macro is obscure and not widely known.
#![allow(clippy::match_like_matches_macro)]
// we're not changing public api due to a lint.
#![allow(clippy::upper_case_acronyms)]

//! A simple, safe HTTP client.
//!
//! Ureq's first priority is being easy for you to use. It's great for
//! anyone who wants a low-overhead HTTP client that just gets the job done. Works
//! very well with HTTP APIs. Its features include cookies, JSON, HTTP proxies,
//! HTTPS, and charset decoding.
//!
//! Ureq is in pure Rust for safety and ease of understanding. It avoids using
//! `unsafe` directly. It [uses blocking I/O][blocking] instead of async I/O, because that keeps
//! the API simple and keeps dependencies to a minimum. For TLS, ureq uses
//! [rustls].
//!
//! Version 2.0.0 was released recently and changed some APIs. See the [changelog] for details.
//!
//! [blocking]: #blocking-io-for-simplicity
//! [changelog]: https://github.com/algesten/ureq/blob/master/CHANGELOG.md
//!
//!
//! ## Usage
//!
//! In its simplest form, ureq looks like this:
//!
//!
//! For more involved tasks, you'll want to create an [Agent]. An Agent
//! holds a connection pool for reuse, and a cookie store if you use the
//! "cookies" feature. An Agent can be cheaply cloned due to an internal
//! [Arc](std::sync::Arc) and all clones of an Agent share state among each other. Creating
//! an Agent also allows setting options like the TLS configuration.
//!
//!
//! Ureq supports sending and receiving json, if you enable the "json" feature:
//!
//!
//! ## Error handling
//!
//! ureq returns errors via `Result<T, ureq::Error>`. That includes I/O errors,
//! protocol errors, and status code errors (when the server responded 4xx or
//! 5xx)
//!
//!
//! More details on the [Error] type.
//!
//! ## Features
//!
//! To enable a minimal dependency tree, some features are off by default.
//! You can control them when including ureq as a dependency.
//!
//! `ureq = { version = "*", features = ["json", "charset"] }`
//!
//! * `tls` enables https. This is enabled by default.
//!
//! # Plain requests
//!
//! Most standard methods (GET, POST, PUT etc), are supported as functions from the
//! top of the library ([get()], [post()], [put()], etc).
//!
//! These top level http method functions create a [Request] instance
//! which follows a build pattern. The builders are finished using:
//!
//! * [`.call()`][Request::call()] without a request body.
//!
//! # Blocking I/O for simplicity
//!
//! Ureq uses blocking I/O rather than Rust's newer [asynchronous (async) I/O][async]. Async I/O
//! allows serving many concurrent requests without high costs in memory and OS threads. But
//! it comes at a cost in complexity. Async programs need to pull in a runtime (usually
//! [async-std] or [tokio]). They also need async variants of any method that might block, and of
//! [any method that might call another method that might block][what-color]. That means async
//! programs usually have a lot of dependencies - which adds to compile times, and increases
//! risk.
//!
//! The costs of async are worth paying, if you're writing an HTTP server that must serve
//! many many clients with minimal overhead. However, for HTTP _clients_, we believe that the
//! cost is usually not worth paying. The low-cost alternative to async I/O is blocking I/O,
//! which has a different price: it requires an OS thread per concurrent request. However,
//! that price is usually not high: most HTTP clients make requests sequentially, or with
//! low concurrency.
//!
//! That's why ureq uses blocking I/O and plans to stay that way. Other HTTP clients offer both
//! an async API and a blocking API, but we want to offer a blocking API without pulling in all
//! the dependencies required by an async API.
//!
//!
//!

mod agent;
mod body;
mod error;
mod header;
mod request;
mod response;
mod stream;
mod unit;
mod url;

#[doc(hidden)]

pub use crate::agent::Agent;
pub use crate::error::{Error, ErrorKind, OrAnyStatus, Transport};
pub use crate::request::Request;
pub use crate::response::Response;
pub use crate::stream::Stream;
pub use crate::url::Url;

pub type Result<T> = std::result::Result<T, Error>;

// is_test returns false so long as it has only ever been called with false.
// If it has ever been called with true, it will always return true after that.
// This is a public but hidden function used to allow doctests to use the test_agent.
// Note that we use this approach for doctests rather the #[cfg(test)], because
// doctests are run against a copy of the crate build without cfg(test) set.
// We also can't use #[cfg(doctest)] to do this, because cfg(doctest) is only set
// when collecting doctests, not when building the crate.
#[doc(hidden)]

/// Agents are used to hold configuration and keep state between requests.
fn agent() -> Agent {
    Agent::build()
}

/// Make a GET request.
pub fn get(path: &str) -> Result<Request> {
    agent().get(path)
}

/// Send a GET request.
pub fn send_multiple(path: Vec<Url>) -> Result<Vec<Stream>> {
    agent().get_multiple(path)
}

/// Make a GET request.
pub fn get_response(stream: Stream) -> Result<Response> {
    agent().get_response(stream)
}

