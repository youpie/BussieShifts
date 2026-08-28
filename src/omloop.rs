use std::{collections::HashMap, fs, path::PathBuf};

use actix_web::{HttpResponse, Responder, get, http::header::ContentType, web};
use color_eyre::Section;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime, Time, Weekday};

use crate::{
    ShiftQuery,
    collection::PdfTimetableCollection,
    get_valid_timetables,
    parsing::shift_structs::{JobType, Shift, ShiftJob, ShiftValidDay},
    return_error,
};

use crate::prelude::*;

type Omloop = usize;
type Index = u8;
type DayOfTheWeek = u8;

/*
!!! Currently the get_all_omlopen function does not work satisfactory
This is because it can only show a single dienstregeling. Which does not work with wijzigingen
New feature: Wijzigingen in dienstregelingen should be absorbed to a single dienstregeling file
And then the api should only search for a single dienstregeling file, instead of recusively searching */

#[derive(Debug, Serialize, Deserialize)]
pub struct OmloopDayIndex {
    timetable_date: Date,
    day_indexes: HashMap<Omloop, HashMap<DayOfTheWeek, Index>>,
}

impl OmloopDayIndex {
    /// Will also save omloops and Self to disk
    pub fn new_omloop_timetable(timetable: &PdfTimetableCollection) -> Result<Self> {
        debug!("Indexing omlopen for timetable {}", timetable.base_start());
        let mut omlopen_map = HashMap::new();
        for shift in &timetable.shifts {
            BusOmloopDay::new_or_extend(&mut omlopen_map, shift);
        }

        // create the valid days index map
        let mut index_map_for_omloop = HashMap::new();
        let mut current_index_for_omloop: HashMap<Omloop, Index> = HashMap::new();
        for mut omloop in omlopen_map {
            omloop.1.sort_jobs();
            // Find the current valid index for a given omloop
            // A new index is for a new valid day array of an omloop
            // Say an omloop is valid on    MA/DI/WO
            // And on                       DO/VR
            // then MA and DI and WO have index 0
            // And DO and VR     have index 1
            // ZA/ZO simply have no entry as the omloop is not used on that day
            let index_entry = current_index_for_omloop.entry(omloop.0.0).or_insert(0);
            for valid_day in omloop.0.1 {
                // For every day this omloop with this day array is valid, add an entry to the map
                // Where the day of the week is the key and the index code is the value
                index_map_for_omloop
                    .entry(omloop.0.0)
                    .or_insert(HashMap::new())
                    .insert(valid_day as u8, *index_entry);
            }
            omloop.1.save(timetable.base_start(), *index_entry)?;
            *index_entry += 1;
        }

        let index_map = Self {
            timetable_date: timetable.base_start(),
            day_indexes: index_map_for_omloop,
        };

        index_map.save(timetable)?;
        Ok(index_map)
    }

    fn save(&self, timetable: &PdfTimetableCollection) -> Result<()> {
        _ = std::fs::create_dir_all(get_base_path(timetable.base_start()));
        let path = Self::path(timetable, false);
        fs::write(path, serde_json::to_string(self)?).note("Failed to save OmloopIndex")?;
        let path = Self::path(timetable, true);
        fs::write(path, postcard::to_allocvec(self)?)?;
        Ok(())
    }

    fn load(timetable: &PdfTimetableCollection) -> Result<Self> {
        let path = Self::path(timetable, true);
        debug!("Trying to load timetable {path:?}");
        Ok(postcard::from_bytes(
            &fs::read(&path).note(format!("Failed to read OmloopIndex: {:?}", path))?,
        )
        .note("Failed to parse OmloopIndex")?)
    }

    fn path(timetable: &PdfTimetableCollection, binary: bool) -> PathBuf {
        let mut path = BusOmloopDay::get_path(0, timetable.base_start(), 0);
        path.set_file_name("index");
        path.set_extension(match binary {
            true => "bin",
            false => "json",
        });
        path
    }

    pub fn get_omloop(
        omloop: usize,
        day_of_week: Weekday,
        mut timetables: Vec<PdfTimetableCollection>,
    ) -> Result<String> {
        let index_map = Self::find_omloop(omloop, day_of_week, &mut timetables)?;
        let index = *index_map
            .day_indexes
            .get(&omloop)
            .and_then(|v| v.get(&(day_of_week as u8)))
            .result_reason("No omloop report found for that day")?;
        Ok(BusOmloopDay::load_string(
            omloop,
            index_map.timetable_date,
            index,
        )?)
    }

    pub fn get_all_omloop(day: Weekday, timetable: &PdfTimetableCollection) -> Result<String> {
        let date_index = Self::load(timetable)?;
        let omlopen = date_index
            .day_indexes
            .iter()
            .filter(|v| v.1.contains_key(&(day as u8)))
            .map(|v| *v.0)
            .collect::<Vec<usize>>();
        Ok(serde_json::to_string_pretty(&omlopen)?)
    }

    fn find_omloop(
        omloop: Omloop,
        day_of_week: Weekday,
        valid_timetables: &mut Vec<PdfTimetableCollection>,
    ) -> Result<OmloopDayIndex> {
        let current_timetable = match valid_timetables.pop() {
            Some(timetable) => timetable,
            None => return Err(eyre!("Could not find any timetable with that omloop")), // If there are no more valid timetables while this check runs, the shift is not available
        };
        let current_omloop_index = Self::load(&current_timetable)?;
        match current_omloop_index
            .day_indexes
            .iter()
            .find(|v| *v.0 == omloop && v.1.contains_key(&(day_of_week as u8)))
        {
            Some(_) => Ok(current_omloop_index),
            None => Self::find_omloop(omloop, day_of_week, valid_timetables),
        }
    }
}

/// Added the shift number to the omloop
#[derive(Debug, Deserialize, Serialize, Clone)]
struct ShiftJobExtended {
    job_type: JobType,
    start: Option<Time>,
    end: Option<Time>,
    start_location: Option<String>,
    end_location: Option<String>,
    rit: Option<usize>,
    shift: String,
    #[serde(skip)]
    next_day: bool,
}

impl ShiftJobExtended {
    fn from_job(shift: &Shift, job: ShiftJob) -> Self {
        Self {
            job_type: job.job_type,
            start: job.start,
            end: job.end,
            start_location: job.start_location,
            end_location: job.end_location,
            rit: job.rit,
            shift: shift.shift_nr.to_owned(),
            next_day: job.next_day,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BusOmloopDay {
    omloop: usize,
    jobs: Vec<ShiftJobExtended>,
    day_index: u8,
}

impl BusOmloopDay {
    pub(self) fn new_or_extend(
        omlopen: &mut HashMap<(Omloop, Vec<ShiftValidDay>), Self>,
        shift: &Shift,
    ) {
        for job in &shift.job {
            // If the omloop of the job is found in the OmloopDay hashmap. We add that job to that OmloopDay
            // We also need to make sure that it is of the same index
            if let Some(job_omloop) = job.omloop.or(job.assumed_omloop) {
                if let Some(omloop_dag) = omlopen.get_mut(&(job_omloop, shift.valid_days.clone())) {
                    omloop_dag
                        .jobs
                        .push(ShiftJobExtended::from_job(&shift, job.clone()));
                } else {
                    // If the omloop is not found in the Hasmap, we create a new instance
                    let new_omloop_day = Self {
                        omloop: job_omloop,
                        jobs: vec![ShiftJobExtended::from_job(&shift, job.clone())],
                        day_index: 0,
                    };
                    omlopen.insert((job_omloop, shift.valid_days.clone()), new_omloop_day);
                }
            }
        }
    }

    pub(self) fn _load(omloop: Omloop, start_date: Date, index: u8) -> Result<Self> {
        Ok(serde_json::from_str(&Self::load_string(
            omloop, start_date, index,
        )?)?)
    }

    pub fn load_string(omloop: Omloop, start_date: Date, index: u8) -> Result<String> {
        let path = Self::get_path(omloop, start_date, index);
        debug!("Trying to load omloop {path:?}");
        Ok(fs::read_to_string(path)?)
    }

    pub(self) fn save(&self, start_date: Date, index: u8) -> Result<()> {
        let path = Self::get_path(self.omloop, start_date, index);
        _ = std::fs::create_dir_all(get_base_path(start_date));
        Ok(fs::write(path, serde_json::to_string_pretty(self)?)?)
    }

    pub(self) fn sort_jobs(&mut self) {
        let jobs = std::mem::take(&mut self.jobs); // Never used this before. But AI recommended it
        let (mut next_day, mut same_day): (Vec<_>, Vec<_>) =
            jobs.into_iter().partition(|v| v.next_day);
        same_day.sort_by_key(|v| v.start);
        next_day.sort_by_key(|v| v.start);
        same_day.append(&mut next_day);
        self.jobs = same_day;
    }

    pub(self) fn get_path(omloop: Omloop, start_date: Date, index: u8) -> PathBuf {
        let mut path = get_base_path(start_date);
        path.push(Self::get_filename(omloop, index));
        path
    }

    pub(self) fn get_filename(omloop: Omloop, index: u8) -> String {
        format!("{index}_{omloop}.json")
    }
}

fn get_base_path(start_date: Date) -> PathBuf {
    let start_date = start_date.format(DATE_FORMAT).unwrap();
    PathBuf::from(format!("{COLLECTION_PATH}/{start_date}/{OMLOOP_PATH}/"))
}

#[get("/omloop/{omloop_number}")]
pub async fn get_omloop(
    request: web::Path<usize>,
    query: web::Query<ShiftQuery>,
) -> impl Responder {
    let (day_of_the_week, timetables) = match get_dow_and_timetable(query) {
        Ok(v) => v,
        Err(v) => return v,
    };

    match OmloopDayIndex::get_omloop(request.into_inner(), day_of_the_week, timetables) {
        Ok(v) => HttpResponse::Ok().content_type(ContentType::json()).body(v),
        Err(e) => return_error(e),
    }
}

#[get("/omloop/index")]
pub async fn get_omloop_overview(query: web::Query<ShiftQuery>) -> HttpResponse {
    let (day_of_the_week, timetable) = match get_dow_and_timetable(query) {
        Ok(v) => v,
        Err(v) => return v,
    };

    let timetable = match timetable.first() {
        Some(v) => v,
        None => return return_error(eyre!("Could not find any timetables")),
    };

    match OmloopDayIndex::get_all_omloop(day_of_the_week, &timetable) {
        Ok(omlopen) => HttpResponse::Ok()
            .content_type(ContentType::json())
            .body(omlopen),
        Err(e) => return_error(e),
    }
}

fn get_dow_and_timetable(
    date_query: web::Query<ShiftQuery>,
) -> Result<(Weekday, Vec<PdfTimetableCollection>), HttpResponse> {
    let date = date_query
        .date
        .as_ref()
        .and_then(|date_string| {
            Date::parse(date_string, DATE_FORMAT)
                .warn_owned("parsing date")
                .ok()
        })
        .unwrap_or(OffsetDateTime::now_utc().date());

    let day_of_the_week = date.weekday();

    let timetable = match get_valid_timetables(Some(date), false) {
        Ok(v) => v.0,
        _ => return Err(return_error(eyre!("Could not get timetable"))),
    };

    Ok((day_of_the_week, timetable))
}
