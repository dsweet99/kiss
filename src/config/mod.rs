
mod error;
mod keys;
mod load;
mod merge;
mod types;
mod validation;

pub use error::ConfigError;
pub use types::{Config, ConfigLanguage};
pub use validation::is_similar;

pub(crate) use validation::{
    apply_lenient_string_list, check_unknown_keys, get_usize, parse_string_list_key,
};

#[cfg(test)]
mod tests;
