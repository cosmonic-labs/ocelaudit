//! OcelAudit search engine.
//!
//! Hand-rolled fallback chosen in M1 (see `docs/m1-search-engine-decision.md`).
//! Pure Rust; no C deps; fully `wasm32-wasip2`-compatible. The pieces:
//!
//! - `tokenize`: NFKC normalization, accent folding, lowercase, punctuation
//!   stripping, whitespace tokenization. Used at index time and query time.
//! - `model`: input types (`CslEntry`, `EntityType`) and result types (`Hit`,
//!   `Tlp`).
//! - `tlp`: TLP banding rules (RED ≥ 0.95 or exact alias match, YELLOW
//!   [0.75, 0.95), GREEN otherwise).
//! - `engine`: inverted index over tokens, trigram index for substring/typo
//!   recall, BM25 scorer, Jaro-Winkler reranker over top-K candidates,
//!   prefix autocomplete.

pub mod engine;
pub mod model;
pub mod tlp;
pub mod tokenize;

pub use engine::{SearchEngine, SearchParams};
pub use model::{CslEntry, EntityType, Hit, Tlp};

// Re-export `engine` so downstream crates (e.g. csl-service) can name
// `engine::SearchResult` directly without depending on the inner module
// path.
