use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{LazyLock, RwLock},
};

use lopdf::Document;
use regex::Regex;
use serde::{Deserialize, Serialize};
use time::Date;

use crate::parsing::{shift_parsing::parse_pdf, shift_structs::Shift, valid_on};

use crate::prelude::*;

static ALL_TIMETABLE_COLLECTIONS: LazyLock<RwLock<Vec<PdfTimetableCollection>>> =
    LazyLock::new(|| RwLock::new(vec![]));

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ShiftData {
    pub pages: Vec<u32>,
    pub file_id: usize,
    pub shift_prefix: String,
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
        let shift_data_map = ShiftData::load(&pdf_path, file_id)?;
        let parsed_shifts = parse_pdf(&pdf_path, shift_data_map.clone())?;

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
            info!("Extending existing collection {:?}", &start_date);
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
}

impl ShiftData {
    pub fn load(path: &PathBuf, file_id: usize) -> Result<HashMap<String, ShiftData>> {
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
                        .or_insert(ShiftData {
                            pages: vec![*page_num],
                            file_id,
                            shift_prefix,
                        });
                }
            }
        }
        Ok(index)
    }
}
