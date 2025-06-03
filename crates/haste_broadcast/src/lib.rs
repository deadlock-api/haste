#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::lossy_float_literal)]
#![deny(clippy::redundant_clone)]
#![deny(unreachable_pub)]

mod broadcastfile;
mod broadcasthttp;
pub(crate) mod demostream;
mod httpclient;

pub use broadcastfile::BroadcastFile;
pub use broadcasthttp::{default_headers, BroadcastHttp, BroadcastHttpClientError};
pub use httpclient::HttpClient;
