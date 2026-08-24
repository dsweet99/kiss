mod error;
mod keys;
mod load;
mod merge;
mod paths;
mod types;
mod validation;

pub use error::ConfigError;
pub use paths::{
    ConfigPathOverrideGuard, active_kissconfig_path, find_repo_root, kissconfig_path_for_repo,
    kissconfig_path_from_cwd, set_config_path_override,
};
pub use types::{
    Config, ConfigLanguage, LanguageTablesPresent, missing_language_table_message,
    reject_unconfigured_languages,
};
pub use validation::is_similar;

pub(crate) use validation::{
    apply_lenient_string_list, check_unknown_keys, get_usize, parse_string_list_key,
};

#[cfg(test)]
mod tests;
