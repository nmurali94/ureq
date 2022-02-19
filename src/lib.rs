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
//! * `cookies` enables cookies.
//! * `json` enables [Response::into_json()] and [Request::send_json()] via serde_json.
//! * `charset` enables interpreting the charset part of the Content-Type header
//!    (e.g.  `Content-Type: text/plain; charset=iso-8859-1`). Without this, the
//!    library defaults to Rust's built in `utf-8`.
//! * `socks-proxy` enables proxy config using the `socks4://`, `socks4a://`, `socks5://` and `socks://` (equal to `socks5://`) prefix.
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
//! * [`.send()`][Request::send()] with a request body as [Read][std::io::Read] (chunked encoding support for non-known sized readers).
//! * [`.send_string()`][Request::send_string()] body as string.
//! * [`.send_bytes()`][Request::send_bytes()] body as bytes.
//! * [`.send_form()`][Request::send_form()] key-value pairs as application/x-www-form-urlencoded.
//!
//! # JSON
//!
//! By enabling the `ureq = { version = "*", features = ["json"] }` feature,
//! the library supports serde json.
//!
//! * [`request.send_json()`][Request::send_json()] send body as serde json.
//! * [`response.into_json()`][Response::into_json()] transform response to json.
//!
//! # Content-Length and Transfer-Encoding
//!
//! The library will send a Content-Length header on requests with bodies of
//! known size, in other words, those sent with
//! [`.send_string()`][Request::send_string()],
//! [`.send_bytes()`][Request::send_bytes()],
//! [`.send_form()`][Request::send_form()], or
//! [`.send_json()`][Request::send_json()]. If you send a
//! request body with [`.send()`][Request::send()],
//! which takes a [Read][std::io::Read] of unknown size, ureq will send Transfer-Encoding:
//! chunked, and encode the body accordingly. Bodyless requests
//! (GETs and HEADs) are sent with [`.call()`][Request::call()]
//! and ureq adds neither a Content-Length nor a Transfer-Encoding header.
//!
//! If you set your own Content-Length or Transfer-Encoding header before
//! sending the body, ureq will respect that header by not overriding it,
//! and by encoding the body or not, as indicated by the headers you set.
//!
//!
//! # Character encoding
//!
//! By enabling the `ureq = { version = "*", features = ["charset"] }` feature,
//! the library supports sending/receiving other character sets than `utf-8`.
//!
//! For [`response.into_string()`][Response::into_string()] we read the
//! header `Content-Type: text/plain; charset=iso-8859-1` and if it contains a charset
//! specification, we try to decode the body using that encoding. In the absence of, or failing
//! to interpret the charset, we fall back on `utf-8`.
//!
//! Similarly when using [`request.send_string()`][Request::send_string()],
//! we first check if the user has set a `; charset=<whatwg charset>` and attempt
//! to encode the request body using that.
//!
//!
//! # Proxying
//!
//! ureq supports two kinds of proxies,  HTTP [`CONNECT`], [`SOCKS4`] and [`SOCKS5`], the former is
//! always available while the latter must be enabled using the feature
//! `ureq = { version = "*", features = ["socks-proxy"] }`.
//!
//! Proxies settings are configured on an [Agent] (using [AgentBuilder]). All request sent
//! through the agent will be proxied.
//!
//! ## Example using HTTP CONNECT
//!
//!
//! ## Example using SOCKS5
//!
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
//! [async]: https://rust-lang.github.io/async-book/01_getting_started/02_why_async.html
//! [async-std]: https://github.com/async-rs/async-std#async-std
//! [tokio]: https://github.com/tokio-rs/tokio#tokio
//! [what-color]: https://journal.stuffwithstuff.com/2015/02/01/what-color-is-your-function/
//! [`CONNECT`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/CONNECT
//! [`SOCKS4`]: https://en.wikipedia.org/wiki/SOCKS#SOCKS4
//! [`SOCKS5`]: https://en.wikipedia.org/wiki/SOCKS#SOCKS5
//!
//! ------------------------------------------------------------------------------
//!
//! Ureq is inspired by other great HTTP clients like
//! [superagent](http://visionmedia.github.io/superagent/) and
//! [the fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API).
//!
//! If ureq is not what you're looking for, check out these other Rust HTTP clients:
//! [surf](https://crates.io/crates/surf), [reqwest](https://crates.io/crates/reqwest),
//! [isahc](https://crates.io/crates/isahc), [attohttpc](https://crates.io/crates/attohttpc),
//! [actix-web](https://crates.io/crates/actix-web), and [hyper](https://crates.io/crates/hyper).
//!

mod agent;
mod body;
mod error;
mod header;
//mod pool;
//mod proxy;
mod request;
mod response;
mod stream;
mod unit;
mod url;

#[doc(hidden)]
//mod testserver;

pub use crate::agent::Agent;
pub use crate::agent::AgentBuilder;
pub use crate::error::{Error, ErrorKind, OrAnyStatus, Transport};
pub use crate::request::Request;
pub use crate::response::Response;

#[cfg(feature = "json")]
pub use serde_json::{to_value as serde_to_value, Map as SerdeMap, Value as SerdeValue};

pub type Result<T> = std::result::Result<T, Error>;

/// Creates an [AgentBuilder].
pub fn builder() -> AgentBuilder {
    AgentBuilder::new()
}

// is_test returns false so long as it has only ever been called with false.
// If it has ever been called with true, it will always return true after that.
// This is a public but hidden function used to allow doctests to use the test_agent.
// Note that we use this approach for doctests rather the #[cfg(test)], because
// doctests are run against a copy of the crate build without cfg(test) set.
// We also can't use #[cfg(doctest)] to do this, because cfg(doctest) is only set
// when collecting doctests, not when building the crate.
#[doc(hidden)]

/// Agents are used to hold configuration and keep state between requests.
pub fn agent() -> Agent {
    AgentBuilder::new().build()
}

/// Make a GET request.
pub fn get(path: &str) -> Result<Request> {
    agent().get(path)
}

