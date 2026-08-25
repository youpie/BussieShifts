use std::{collections::HashMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use time::Date;

use crate::{
    COLLECTION_PATH, DATE_FORMAT, OMLOOP_PATH,
    collection::PdfTimetableCollection,
    parsing::shift_structs::{Shift, ShiftJob, ShiftValidDay},
};

pub struct OmloopIndex {
    index: HashMap<Omloop, HashMap<ShiftValidDay, u8>>,
}

impl OmloopIndex {
    /// Will also save omloops and Self to disk
    pub fn new_omloop_timetable(timetable: &PdfTimetableCollection) {
        debug!("Indexing omlopen");
        let mut omlopen_map = HashMap::new();
        for shift in &timetable.pages {
            if let Some(shift) = Shift::load(timetable, &shift.0).ok() {
                BusOmloopDay::new_or_extend(&mut omlopen_map, shift);
            } else {
                warn!("Skipped loading {}", shift.0);
            }
        }
        for mut omloop in omlopen_map {
            omloop.1.sort_jobs();
            fs::write(
                format!(
                    "test/{:?}_{}_{:?}.json",
                    timetable.start_date, omloop.0.0, omloop.0.1
                ),
                serde_json::to_string_pretty(&omloop.1).unwrap(),
            )
            .unwrap();
        }
    }

    pub fn get(
        omloop: usize,
        day: ShiftValidDay,
        timetable: &PdfTimetableCollection,
    ) -> BusOmloopDay {
        todo!()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BusOmloopDay {
    omloop: usize,
    jobs: Vec<ShiftJob>,
    index: u8,
}

type Omloop = usize;
type Index = u8;

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
                omloop_dag.jobs.push(job.clone());
            } else if let Some(job_omloop) = job.omloop {
                // If the omloop is not found in the Hasmap, we create a new instance
                debug!("Creating new omloop {job_omloop}");
                let new_omloop_day = Self {
                    omloop: job_omloop,
                    jobs: vec![job.clone()],
                    index: 0,
                };
                omlopen.insert((job_omloop, shift.valid_days.clone()), new_omloop_day);
            }
        }
    }

    pub(self) fn sort_jobs(&mut self) {
        self.jobs.sort_by_key(|k| k.start);
    }

    pub(self) fn get_path(omloop: &str, start_date: Date, index: u8) -> PathBuf {
        let start_date = start_date.format(DATE_FORMAT).unwrap();
        PathBuf::from(format!(
            "{COLLECTION_PATH}/{start_date}/{OMLOOP_PATH}/{}",
            Self::get_filename(omloop, index)
        ))
    }

    pub(self) fn get_filename(omloop: &str, index: u8) -> String {
        format!("{index}_{omloop}.json")
    }
}
