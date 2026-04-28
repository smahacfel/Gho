//! Execution Quotes Module
//!
//! Provides the `ExecutableQuoteProvider` — the SSOT for price references
//! used by both Paper and Live backends.

pub mod provider;

pub use provider::{ExecutableQuote, ExecutableQuoteProvider, QuoteProviderConfig, QuoteSource};
