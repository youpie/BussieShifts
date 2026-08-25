use crate::prelude::*;

#[allow(dead_code)]
pub trait OptionResult<T> {
    fn result(self) -> Result<T>;
    fn result_reason(self, reason: &str) -> Result<T>;
}

impl<T> OptionResult<T> for Option<T> {
    fn result(self) -> Result<T> {
        match self {
            Some(value) => Ok(value),
            None => Err(eyre!("Option Unwrap")),
        }
    }
    fn result_reason(self, reason: &str) -> Result<T> {
        match self {
            Some(value) => Ok(value),
            None => Err(eyre!("{reason}")),
        }
    }
}
