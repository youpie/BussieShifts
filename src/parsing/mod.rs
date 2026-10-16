pub(super) mod deadhead;
pub(super) mod omloop;
pub(super) mod shift;
pub(super) mod shift_parsing;
pub(super) mod timetable;
pub(super) mod timetable_paths;
pub(super) mod valid_on;
pub(super) mod vehicle;

use std::fs;

pub(self) use deadhead::*;
pub(self) use omloop::*;
pub(self) use shift::*;
// pub(self) use shift_parsing::*;
pub(self) use timetable::*;
pub(self) use timetable_paths::*;
pub(self) use vehicle::*;

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
        let collection = TimetableCollection::new_or_extend(
            &mut timetable_collections,
            file_path.1.into(),
            file_path.0,
        )?;
        if let Some(collection) = collection {
            timetable_collections.push(collection);
        }
    }
    timetable_collections =
        TimetableCollection::add_valid_from_data(timetable_collections, files.valid_from_files);
    timetable_collections =
        TimetableCollection::combine_from_changes_files(timetable_collections, changes_files);
    TimetableCollection::save(&timetable_collections)?;

    let deadhead_locations = DeadheadLocations::load()?;
    deadhead_locations.debug_locations();

    for timetable in &timetable_collections {
        let omloop_index = OmloopDayIndex::new_for_timetable(timetable).note("Loading Omlopen")?;
        DeadheadCollection::new_for_timetable(&deadhead_locations, timetable, &omloop_index)
            .note("Loading Deadheads")?;
        VehicleCollection::parse(timetable, &omloop_index).note("Loading Vehicles")?;
    }

    TimetableCollection::load_to_global()?;
    Ok(())
}
