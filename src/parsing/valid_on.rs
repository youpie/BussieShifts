use std::{fs, path::PathBuf};

use time::Date;

/// Get the dates on which the timetable should be treated as a new timetable. Load this from valid_on.txt
/// If file could not be loaded for any reason, will print the error, and only return an empty vec
pub fn parse_dates_from_file(path: &PathBuf) -> Vec<Date> {
    match fs::read_to_string(&path) {
        Ok(file) => {
            let lines = file.lines();
            let mut dates = Vec::new();
            for line in lines {
                if let Ok(date) = time::Date::parse(&line, crate::DATE_FORMAT) {
                    dates.push(date);
                }
            }
            dates
        }
        Err(e) => {
            warn!("Could not load {path:?}: {}", e.to_string());
            Vec::new()
        }
    }
}
