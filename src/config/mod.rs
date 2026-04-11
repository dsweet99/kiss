//! Configuration loading and threshold merging.

mod error;
mod keys;
mod load;
mod merge;
mod types;
mod validation;

pub use error::ConfigError;
pub use types::{Config, ConfigLanguage};
pub use validation::is_similar;

pub(crate) use validation::{check_unknown_keys, get_usize};

#[cfg(test)]
mod tests;
