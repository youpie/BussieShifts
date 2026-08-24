use std::{fs, path::PathBuf};

use time::Date;

/// If file could not be loaded for any reason, will print the error, and only return the `pdf_date`
fn get_timetable_valid_on(path: PathBuf, pdf_date: Date) -> Vec<Date> {
    match fs::read_to_string(&path) {
        Ok(file) => {
            let mut lines = file.lines();

            let mut dates = if lines.next() == Some(".") {
                vec![pdf_date]
            } else {
                Vec::new()
            };

            for line in lines {
                if let Ok(date) = time::Date::parse(line, crate::DATE_FORMAT) {
                    dates.push(date);
                }
            }

            dates
        }
        Err(e) => {
            println!("Could not load {path:?}: {}", e.to_string());
            vec![pdf_date]
        }
    }
}
