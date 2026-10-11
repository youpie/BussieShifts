mod collection;
mod deadhead;
mod omloop;
mod shift_parsing;
mod shift_structs;
mod timetable_paths;
mod valid_on;

use std::fs;

pub use collection::*;
pub use deadhead::*;
pub use omloop::*;
pub use shift_parsing::*;
pub use shift_structs::*;
pub use timetable_paths::*;

use crate::prelude::*;
use time::Date;

pub fn parse_dienstboekjes() -> Result<()> {
    let files = TimetablePaths::get()?;
    match fs::create_dir(COLLECTION_PATH) {
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    };
    fs::remove_dir_all(COLLECTION_PATH)?;

    let changes_files: Vec<Vec<Date>> = files
        .changes_files
        .iter()
        .map(|v| valid_on::parse_dates_from_file(v))
        .collect();

    let mut timetable_collections = Vec::new();
    for file_path in files.timetable_pdfs.iter().enumerate() {
        let collection = PdfTimetableCollection::new_or_extend(
            &mut timetable_collections,
            file_path.1.into(),
            file_path.0,
        )?;
        if let Some(collection) = collection {
            timetable_collections.push(collection);
        }
    }
    timetable_collections =
        PdfTimetableCollection::add_valid_from_data(timetable_collections, files.valid_from_files);
    timetable_collections =
        PdfTimetableCollection::combine_from_changes_files(timetable_collections, changes_files);
    PdfTimetableCollection::save(&timetable_collections)?;

    let deadhead_locations = DeadheadLocations::load()?;
    deadhead_locations.debug_locations();

    for timetable in &timetable_collections {
        OmloopDayIndex::new_for_timetable(timetable)?;
        DeadheadCollection::new_for_timetable(&deadhead_locations, timetable)?;
    }

    PdfTimetableCollection::load_to_global()?;
    Ok(())
}
