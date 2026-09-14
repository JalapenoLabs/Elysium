// Copyright © 2026 Jalapeno Labs

//! Cross-cutting HTTP behaviour: security headers, CORS, rate limiting, tracing.

pub mod cors;
pub mod rate_limit;
pub mod security_headers;
pub mod trace;
