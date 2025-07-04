use std::{env, io};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, TestError>;

#[derive(Debug, Error)]
pub enum TestError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Command IO error: {0}")]
    Command(String),

    #[error("Failed: {0}")]
    Failed(String),

    #[error("Assertion failed: {0}")]
    Cargo(#[from] assert_cmd::cargo::CargoError),

    #[error("Env var error: {0}")]
    Var(#[from] env::VarError),
}
