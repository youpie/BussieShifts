use std::fmt::Display;

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

pub trait ResultLog<T, E> {
    fn error(&self, function_name: &str);
    fn warn(&self, function_name: &str);
    fn warn_owned(self, function_name: &str) -> Self;
    fn info(&self, function_name: &str);
}

impl<T, E> ResultLog<T, E> for Result<T, E>
where
    E: Display,
{
    fn info(&self, function_name: &str) {
        match self {
            Err(err) => {
                info!("Error in function \"{function_name}\": {}", err.to_string())
            }
            _ => (),
        }
    }
    fn warn_owned(self, function_name: &str) -> Self {
        self.inspect_err(|err| warn!("Error in function \"{function_name}\": {}", err.to_string()))
    }
    fn warn(&self, function_name: &str) {
        match self {
            Err(err) => {
                warn!("Error in function \"{function_name}\": {}", err.to_string())
            }
            _ => (),
        }
    }
    fn error(&self, function_name: &str) {
        match self {
            Err(err) => {
                error!("Error in function \"{function_name}\": {}", err.to_string())
            }
            _ => (),
        }
    }
}
