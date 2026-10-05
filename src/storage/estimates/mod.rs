//! Where estimate rows are stored: `EstimateStore` backends (memory,
//! SQLite) and the contract they share.

#[cfg(test)]
mod contract;
pub mod memory;
pub mod sqlite;
