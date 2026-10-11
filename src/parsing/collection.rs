use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{LazyLock, RwLock},
};

use lopdf::Document;
use regex::Regex;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use super::*;
use crate::prelude::*;

static ALL_TIMETABLE_COLLECTIONS: LazyLock<RwLock<Vec<PdfTimetableCollection>>> =
    LazyLock::new(|| RwLock::new(vec![]));

pub type ValidTimetables = Vec<PdfTimetableCollection>;
pub type NextTimetableChangeDate = Option<Date>;

#[derive(PartialEq)]
pub enum TTBOptions {
    None,
    AppendFuture,
    OnlyFirst,
}

impl TTBOptions {
    pub fn from_bool(true_is_only_first: bool, value_if_false: Self) -> Self {
        match true_is_only_first {
            true => TTBOptions::OnlyFirst,
            false => value_if_false,
        }
    }
}

struct ChangesCollection {
    timetable: PdfTimetableCollection,
    base_start_date: Date,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PdfTimetableCollection {
    pub start_date: Date,
    valid_from: Vec<Date>,
    pub files: HashMap<usize, String>,
    pub pages: HashMap<String, ShiftData>,
    #[serde(skip)]
    /// Will be skipped during deserialization
    pub shifts: HashMap<String, Shift>,
}

impl PdfTimetableCollection {
    // Load every PDF and group them
    pub fn new_or_extend(
        existing_timetables: &mut Vec<Self>,
        pdf_path: PathBuf,
        file_id: usize,
    ) -> Result<Option<Self>> {
        // Load the PDF document.
        let shift_data_map = ShiftData::new_from_pdf(&pdf_path, file_id)?;
        let parsed_shifts = shift_parsing::parse_pdf(&pdf_path, shift_data_map.clone())?;

        let start_date = parsed_shifts
            .first()
            .result_reason("No shifts found")?
            .starting_date;

        let mut parsed_shift_map = HashMap::new();
        for shift in parsed_shifts {
            parsed_shift_map.insert(shift.shift_nr.clone(), shift);
        }

        if let Some(existing_collection) = existing_timetables
            .iter_mut()
            .find(|item| item.start_date == start_date)
        {
            debug!("Extending existing collection {:?}", &start_date);
            existing_collection
                .files
                .insert(file_id, pdf_path.to_string_lossy().to_string());
            existing_collection.pages.extend(shift_data_map);

            existing_collection.shifts.extend(parsed_shift_map);
            Ok(None)
        } else {
            info!("Writing new collection {:?}", &start_date);
            Ok(Some(PdfTimetableCollection {
                valid_from: vec![start_date.clone()],
                start_date,
                files: HashMap::from([(file_id, pdf_path.to_string_lossy().to_string())]),
                pages: shift_data_map,
                shifts: parsed_shift_map,
            }))
        }
    }

    pub fn add_valid_from_data(
        current_timetable_collections: Vec<Self>,
        valid_from_paths: Vec<PathBuf>,
    ) -> Vec<Self> {
        let mut valid_froms = Vec::new();
        for valid_from_path in valid_from_paths {
            valid_froms.push(valid_on::parse_dates_from_file(&valid_from_path));
        }

        let mut new_timetable_collections = Vec::new();
        for mut timetable in current_timetable_collections {
            if let Some(valid_from) = valid_froms
                .iter()
                .find(|date| date.first() == Some(&timetable.start_date))
            // If the first date in the valid_from file is equal to the start date of the timetable, that valid_form file is associated with that timetable
            {
                debug!(
                    "Found matching valid_from file for timetable {:?}",
                    valid_from.first()
                );
                timetable.valid_from = valid_from.clone();
            }
            new_timetable_collections.push(timetable);
        }
        new_timetable_collections
    }

    pub fn combine_from_changes_files(
        timetables: Vec<Self>,
        changes_files: Vec<Vec<Date>>,
    ) -> Vec<Self> {
        debug!(
            "Timetables {}, changes {}",
            timetables.len(),
            changes_files.len()
        );
        // Timetables that need changing can be identified by their start date being found in a changes_file on the first line
        // Partition finds these timetables and splits them, the rest are timetables that either don't need changing
        // Or are changed timetables themselves
        let (mut ttbs_with_changes_needed, changes_ttbs_and_unchanging_ttbs): (Vec<_>, Vec<_>) =
            timetables.into_iter().partition(|v| {
                changes_files
                    .iter()
                    .any(|cf| cf.first() == Some(&v.base_start()))
            });
        let mut changes_collection = Vec::new();
        // changed timetables are identified by their start date being contained in a changes_file
        // Because we already filtered out the timetables that need changes, we don't need to filter the first line
        let (_changes_timetables, unchanging_ttb): (Vec<_>, Vec<_>) =
            changes_ttbs_and_unchanging_ttbs.into_iter().partition(|t| {
                let file = changes_files.iter().find(|cf| cf.contains(&t.base_start()));
                if let Some(file) = file
                    && let Some(first_date) = file.first()
                {
                    changes_collection.push(ChangesCollection {
                        timetable: t.clone(),
                        base_start_date: *first_date,
                    });
                    true
                } else {
                    false
                }
            });
        debug!(
            "ttc: {}, uct: {}, ct: {}",
            ttbs_with_changes_needed.len(),
            unchanging_ttb.len(),
            changes_collection.len(),
        );
        // append the changing timetables to the timetables that need changing by linking them together
        for timetable in &mut ttbs_with_changes_needed {
            if let Some(changes_ttb) = changes_collection
                .iter()
                .find(|changes_ttb| changes_ttb.base_start_date == timetable.base_start())
            {
                info!(
                    "Timetable {} has changes from timetable {}. Appending",
                    timetable.base_start(),
                    changes_ttb.timetable.base_start()
                );
                timetable.files.extend(changes_ttb.timetable.files.clone());
                timetable.pages.extend(changes_ttb.timetable.pages.clone());

                for shift in &changes_ttb.timetable.shifts {
                    timetable.shifts.insert(shift.0.to_owned(), shift.1.clone());
                }
            }
        }

        let mut modified_timetables_list = unchanging_ttb;
        modified_timetables_list.extend(ttbs_with_changes_needed);
        modified_timetables_list
    }

    pub fn save(collections: &Vec<Self>) -> Result<()> {
        for collection in collections {
            Shift::save_extracted_shifts(
                collection
                    .shifts
                    .clone()
                    .into_iter()
                    .map(|(_id, score)| score)
                    .collect(),
            )?;
            let mut output_path = collection.collection_path();
            output_path.set_extension("json");
            fs::write(
                output_path,
                serde_json::to_string_pretty(collection).unwrap(),
            )?;
        }
        Ok(())
    }

    fn collection_path(&self) -> PathBuf {
        let valid_from_string = self.start_date.format(DATE_FORMAT).unwrap();
        PathBuf::from(format!("{}/{}", COLLECTION_PATH, valid_from_string))
    }

    pub fn load_to_global() -> Result<()> {
        let collections_on_disk = fs::read_dir(COLLECTION_PATH)?;
        let mut collections: Vec<Self> = vec![];
        for file_result in collections_on_disk {
            let file = file_result?;
            if file.file_type()?.is_dir() {
                continue;
            }
            let collection_file: PdfTimetableCollection =
                serde_json::from_slice(&fs::read(file.path())?)?;

            for valid_date in &collection_file.valid_from {
                let mut temp_collection = collection_file.clone();
                temp_collection.start_date = valid_date.clone();
                collections.push(temp_collection);
            }
        }
        collections.sort_by_key(|key| key.start_date);
        *ALL_TIMETABLE_COLLECTIONS.try_write().ok().result()? = collections;
        Ok(())
    }

    pub fn get_global() -> Result<Vec<Self>> {
        let collections = (*ALL_TIMETABLE_COLLECTIONS.read().ok().result()?).to_vec();
        Ok(collections)
    }

    // Will return the normal `start_date` if `valid_from` is empty
    pub fn base_start(&self) -> Date {
        match self.valid_from.first() {
            Some(base) => base.clone(),
            None => {
                warn!("Got request for base start date, was unable to aqquire");
                self.start_date
            }
        }
    }

    // load all pdf_collection files. And determine which one is current
    // Also if it exists, save the date of when it gets invalidated (when the Next timetable starts)
    // In reverse chronological order
    // Upcoming timetables are placed in the beginning of the list, also in reverse order
    pub fn get_valid_timetables(
        date: Option<Date>,
        options: TTBOptions,
    ) -> Result<(ValidTimetables, NextTimetableChangeDate)> {
        let collections = PdfTimetableCollection::get_global()?;
        let current_date = match date {
            Some(date) => date,
            None => OffsetDateTime::now_utc().date(),
        };
        let mut upcoming_timetables: Vec<PdfTimetableCollection> = vec![];
        let mut current_timetables: Vec<PdfTimetableCollection> = vec![];
        // Loop over all files in the collection folder
        for timetable_collection in collections {
            //if the current collection date is higher than the last but lower than the system date. Make this the most recent one
            if timetable_collection.start_date > current_date {
                upcoming_timetables.push(timetable_collection);
            }
            // Create a list of all currently valid timetables
            else if timetable_collection.start_date <= current_date {
                current_timetables.push(timetable_collection)
            }
        }

        let next_timetable = upcoming_timetables
            .last()
            .and_then(|x| Some(x.start_date.clone()));
        // The future timetables should be the first in the list
        let active_timetables = if options == TTBOptions::AppendFuture {
            // first pop the last timetable (the most recent timetable)
            let recent_timetable = current_timetables.pop();
            let mut new_timetables = current_timetables;
            // add the upcoming timetables to the non future timetables (except the first timetable)
            new_timetables.append(&mut upcoming_timetables);
            // Add the most recent timetable back to the list
            if let Some(recent_timetable) = recent_timetable {
                new_timetables.push(recent_timetable);
            }
            new_timetables
        // } else if options == TTBOptions::OnlyFirst {
        // vec![current_timetables.pop().result_reason("No timetables")?]
        } else {
            current_timetables
        };

        Ok((active_timetables, next_timetable))
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ShiftData {
    pub pages: Vec<u32>,
    pub file_id: usize,
    pub shift_prefix: String,
}

impl ShiftData {
    pub(self) fn new_from_pdf(
        path: &PathBuf,
        file_id: usize,
    ) -> Result<HashMap<String, ShiftData>> {
        let doc = Document::load(&path)?;

        // Define a regex pattern that finds "Dienst" followed by a trip number.
        let re = Regex::new(r"Dienst\s*(\b[A-Z]{1,2} \d{4}\b)")?;
        let mut index: HashMap<String, ShiftData> = HashMap::new();

        // Iterate over all pages in the PDF.
        // `get_pages` returns a map of page numbers to their internal object IDs.
        for (page_num, _) in doc.get_pages().iter() {
            // Extract text from the current page.
            let text = doc.extract_text(&[*page_num]).unwrap_or_default();

            // Search for matches in the page text.
            for cap in re.captures_iter(&text) {
                // Capture the group that contains the trip number.
                let shift_name = cap.get(1).map_or("", |m| m.as_str()).to_string();
                let shift_number: String = shift_name
                    .chars()
                    .filter(|character| character.is_numeric())
                    .collect();
                let shift_prefix: String = shift_name
                    .chars()
                    .filter(|character| character.is_alphabetic())
                    .collect();
                if !shift_number.is_empty() {
                    index
                        .entry(shift_number)
                        .and_modify(|shift_data| shift_data.pages.push(*page_num))
                        .or_insert(Self {
                            pages: vec![*page_num],
                            file_id,
                            shift_prefix,
                        });
                }
            }
        }
        Ok(index)
    }

    // Recursive function to find a valid shift
    pub fn find_shift(
        shift_number: &str,
        valid_timetables: &mut Vec<PdfTimetableCollection>,
    ) -> Option<(PdfTimetableCollection, Self)> {
        let current_timetable = match valid_timetables.pop() {
            Some(timetable) => timetable,
            None => return None, // If there are no more valid timetables while this check runs, the shift is not available
        };

        // // The current logic is (probably) sound enough to only return the first valid timetable. So the recursion is no longer needed
        // if return_first {
        //     return current_timetable
        //         .pages
        //         .get(shift_number)
        //         .cloned()
        //         .map(|val| (current_timetable, val));
        // }

        match current_timetable.pages.get(shift_number) {
            Some(shift) => Some((current_timetable.clone(), shift.clone())),
            None => Self::find_shift(shift_number, valid_timetables),
        }
    }
}
