# Service overview

The service exposes a public HTTP API. Requests pass through the rate limiter
(`src/limiter.rs`), then the response cache (`src/cache.rs`).

Plans are metered by `src/billing.rs`; workspace files are kept by
`src/storage.rs`.
