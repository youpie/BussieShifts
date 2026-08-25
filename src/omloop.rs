use std::{collections::HashMap, fs, path::PathBuf};

use actix_web::{HttpResponse, Responder, get, http::header::ContentType, web};
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use crate::{
    ShiftQuery,
    collection::PdfTimetableCollection,
    get_valid_timetables,
    parsing::shift_structs::{Shift, ShiftJob, ShiftValidDay},
    return_error,
};

use crate::prelude::*;

type Omloop = usize;
type Index = u8;
type DayOfTheWeek = u8;

#[derive(Debug, Serialize, Deserialize)]
pub struct OmloopIndex {
    index: HashMap<Omloop, HashMap<DayOfTheWeek, Index>>,
}

impl OmloopIndex {
    /// Will also save omloops and Self to disk
    pub fn new_omloop_timetable(timetable: &PdfTimetableCollection) -> Result<Self> {
        debug!("Indexing omlopen for timetable {}", timetable.start_date);
        let mut omlopen_map = HashMap::new();
        for shift in &timetable.pages {
            if let Some(shift) = Shift::load(timetable, &shift.0).ok() {
                BusOmloopDay::new_or_extend(&mut omlopen_map, shift);
            } else {
                warn!("Skipped loading {}", shift.0);
            }
        }

        // create the valid days index map
        let mut valid_index_map = HashMap::new();
        let mut omloop_current_index: HashMap<Omloop, Index> = HashMap::new();
        for mut omloop in omlopen_map {
            omloop.1.sort_jobs();
            let index = if let Some(index) = omloop_current_index.get(&omloop.0.0) {
                let temp_index = *index;
                omloop_current_index.insert(omloop.0.0, temp_index + 1);
                temp_index + 1
            } else {
                omloop_current_index.insert(omloop.0.0, 0);
                0
            };
            for valid_day in omloop.0.1 {
                if !valid_index_map.contains_key(&omloop.0.0) {
                    valid_index_map.insert(omloop.0.0, HashMap::new());
                }
                if let Some(valid_map) = valid_index_map.get_mut(&omloop.0.0) {
                    valid_map.insert(valid_day as u8, index);
                }
            }
            omloop.1.save(timetable.start_date, index)?;
        }

        let index_map = Self {
            index: valid_index_map,
        };

        index_map.save(timetable)?;
        Ok(index_map)
    }

    fn save(&self, timetable: &PdfTimetableCollection) -> Result<()> {
        let path = Self::path(timetable);
        _ = std::fs::create_dir_all(get_base_path(timetable.start_date));
        Ok(fs::write(path, serde_json::to_string(self)?).wrap_err("Failed to save OmloopIndex")?)
    }

    fn load(timetable: &PdfTimetableCollection) -> Result<Self> {
        let path = Self::path(timetable);
        Ok(
            serde_json::from_str(&fs::read_to_string(path).wrap_err("Failed to read OmloopIndex")?)
                .wrap_err("Failed to parse OmloopIndex")?,
        )
    }

    fn path(timetable: &PdfTimetableCollection) -> PathBuf {
        let mut path = BusOmloopDay::get_path(0, timetable.start_date, 0);
        path.set_file_name("index.json");
        path
    }

    pub fn get_omloop(
        omloop: usize,
        day: u8,
        timetable: &PdfTimetableCollection,
    ) -> Result<String> {
        let index_map = Self::load(timetable)?;
        let index = *index_map
            .index
            .get(&omloop)
            .and_then(|v| v.get(&day))
            .result_reason("No omloop report found for that day")?;
        Ok(BusOmloopDay::load_string(
            omloop,
            timetable.start_date,
            index,
        )?)
    }
}

/// Added the shift number to the omloop
#[derive(Debug, Deserialize, Serialize, Clone)]
struct ShiftJobExtended {
    job: ShiftJob,
    shift: String,
}

impl ShiftJobExtended {
    fn from_job(shift: &Shift, job: ShiftJob) -> Self {
        Self {
            job,
            shift: shift.shift_nr.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BusOmloopDay {
    omloop: usize,
    jobs: Vec<ShiftJobExtended>,
    index: u8,
}

impl BusOmloopDay {
    pub(self) fn new_or_extend(
        omlopen: &mut HashMap<(Omloop, Vec<ShiftValidDay>), Self>,
        shift: Shift,
    ) {
        for job in &shift.job {
            // If the omloop of the job is found in the OmloopDay hashmap. We add that job to that OmloopDay
            // We also need to make sure that it is of the same index
            if let Some(job_omloop) = job.omloop
                && let Some(omloop_dag) = omlopen.get_mut(&(job_omloop, shift.valid_days.clone()))
            {
                omloop_dag
                    .jobs
                    .push(ShiftJobExtended::from_job(&shift, job.clone()));
            } else if let Some(job_omloop) = job.omloop {
                // If the omloop is not found in the Hasmap, we create a new instance
                let new_omloop_day = Self {
                    omloop: job_omloop,
                    jobs: vec![ShiftJobExtended::from_job(&shift, job.clone())],
                    index: 0,
                };
                omlopen.insert((job_omloop, shift.valid_days.clone()), new_omloop_day);
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
        Ok(fs::read_to_string(path)?)
    }

    pub(self) fn save(&self, start_date: Date, index: u8) -> Result<()> {
        let path = Self::get_path(self.omloop, start_date, index);
        _ = std::fs::create_dir_all(get_base_path(start_date));
        Ok(fs::write(path, serde_json::to_string_pretty(self)?)?)
    }

    pub(self) fn sort_jobs(&mut self) {
        self.jobs.sort_by_key(|k| k.job.start);
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
    let date = query
        .date
        .as_ref()
        .and_then(|date_string| Date::parse(date_string, DATE_FORMAT).ok())
        .unwrap_or(OffsetDateTime::now_utc().date());

    let day_of_the_week = date.weekday() as u8;
    let timetable = match get_valid_timetables(Some(date), false) {
        Ok(timetable) if let Some(first_timetable) = timetable.0.first().cloned() => {
            first_timetable
        }
        _ => return return_error("Could not get timetable".to_string()),
    };

    match OmloopIndex::get_omloop(request.into_inner(), day_of_the_week, &timetable) {
        Ok(v) => HttpResponse::Ok().content_type(ContentType::json()).body(v),
        Err(e) => return_error(e.to_string()),
    }
}
