#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::lossy_float_literal)]
#![deny(clippy::redundant_clone)]
#![deny(unreachable_pub)]

//! overengineered piece of crap.

mod error;
mod parser;
mod span;
mod tokenizer;

pub use error::Error;
pub use parser::{parse, Expr, Lit};
pub use span::Span;
pub use tokenizer::{Token, TokenKind, Tokenizer};
