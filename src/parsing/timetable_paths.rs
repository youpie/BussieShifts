use std::{
    ffi::OsStr,
    fs,
    path::{Component, PathBuf},
    time::SystemTime,
};

use walkdir::WalkDir;

use crate::{BOOKS_PATH, CHANGE_FOLDER_NAME, prelude::*};

#[derive(Hash, Debug)]
pub struct TimetablePaths {
    pub timetable_pdfs: Vec<PathBuf>,
    pub valid_from_files: Vec<PathBuf>,
    pub changes_files: Vec<PathBuf>,
}

impl TimetablePaths {
    pub fn get() -> Result<TimetablePaths> {
        let mut timetable_pdfs = Vec::new();
        let mut updated_timetable_pdfs = Vec::new();
        let mut valid_from_files = Vec::new();
        let mut changes_files = Vec::new();
        for entry in WalkDir::new(BOOKS_PATH).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path
                .components()
                .find(|components| *components == Component::Normal(OsStr::new(CHANGE_FOLDER_NAME)))
                .is_some()
            {
                debug!("Found {path:?}");
                if path.is_file() {
                    updated_timetable_pdfs.push(path.to_path_buf());
                }
            } else if path.file_name() == Some(OsStr::new("valid_from.txt")) {
                valid_from_files.push(path.to_path_buf());
            } else if path.file_name() == Some(OsStr::new("changes.txt")) {
                changes_files.push(path.to_path_buf());
            } else if path.is_file() && path.extension() == Some(OsStr::new("pdf")) {
                // Skip directories
                timetable_pdfs.push(path.to_path_buf());
            } else if path.is_file() {
                info!("Ignoring file {path:?} (probably missing .pdf)");
            }
        }
        updated_timetable_pdfs
            .sort_by_key(|element| Self::get_creation_date(element).unwrap_or(SystemTime::now()));
        debug!("Changed sheets: {updated_timetable_pdfs:#?}");
        timetable_pdfs.extend(updated_timetable_pdfs);
        Ok(TimetablePaths {
            timetable_pdfs,
            valid_from_files,
            changes_files,
        })
    }

    fn get_creation_date(path: &PathBuf) -> Result<SystemTime> {
        Ok(fs::metadata(path)?.created()?)
    }
}
