pub use crate::error::*;
pub use color_eyre::Help;
pub use color_eyre::eyre::{Context, Result, WrapErr, eyre};
pub use serde::{Deserialize, Serialize};
use time::Weekday;
pub use tracing::{debug, error, info, span, trace, warn};

pub(crate) use crate::{COLLECTION_PATH, DATE_FORMAT, SHIFT_PATH, ShiftQuery, return_error};

pub trait ToNumber {
    fn to_number(&self) -> usize;
}

impl ToNumber for String {
    fn to_number(&self) -> usize {
        let text_numbers: String = self.chars().filter(|c| c.is_ascii_digit()).collect();
        text_numbers.parse().unwrap_or_default()
    }
}

pub trait GetAll {
    fn get_all() -> Vec<Self>
    where
        Self: std::marker::Sized;
}

impl GetAll for Weekday {
    /// Get an array of every day in the week
    fn get_all() -> Vec<Self>
    where
        Self: std::marker::Sized,
    {
        vec![
            Weekday::Monday,
            Weekday::Tuesday,
            Weekday::Wednesday,
            Weekday::Thursday,
            Weekday::Friday,
            Weekday::Saturday,
            Weekday::Sunday,
        ]
    }
}
