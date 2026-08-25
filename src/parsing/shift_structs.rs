use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};
use thiserror::Error;
use time::{Date, Time};

use crate::collection::PdfTimetableCollection;

use super::*;

#[derive(Error, Debug, Serialize, Deserialize, Clone)]
pub enum ShiftParseError {
    #[error("Shift on page {page_number} had a generic error {error_string}\nline: {line:?}",error_string = error.to_string())]
    GenericShiftError {
        page_number: u32,
        error: String,
        line: Option<String>,
    },
    #[error("Failed to parse metadata on page {page_number}\nline: {line:?}")]
    MetadataFailure {
        page_number: u32,
        line: Option<String>,
    },
    #[error("{function}: Unwrapped an option while parsing {parsing_job:?}\nline: {line:?}")]
    Option {
        function: String,
        parsing_job: Option<String>,
        line: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Eq, Hash, PartialEq)]
pub enum ShiftValidDay {
    Monday = 0,
    Tuesday,
    Wednsesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
    #[default]
    Unknown,
}

/// **Remove in the future!!!!!!!!!!!!!!!!!!!**
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ShiftValid {
    Weekdays,
    Wednesday,
    #[serde(rename = "Weekdays except Wednesdays")]
    WeekdaysExceptWednesday,
    Saturday,
    Sunday,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ShiftType {
    Vroeg,
    Tussen,
    Dag,
    Gebroken {
        start_break: Option<Time>,
        end_break: Option<Time>,
    }, // If one is none, it means it's half of a broken shift
    Laat,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum JobDrivingType {
    Lijn(u32),
    Mat,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum JobMessageType {
    Meenemen { dienstnummers: Vec<u32> },
    Passagieren { dienstnummer: u32, omloop: String },
    BusOp { lijn: u32 },
    NeemBus { bustype: String },
    Other(String),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum JobType {
    Rijden { drive_type: JobDrivingType },
    Pauze,
    Onderbreking,
    OpAfstap,
    RijklaarMaken,
    StallenAfmelden,
    Melding { message: JobMessageType },
    LoopReis,
    Reserve,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ShiftJob {
    pub job_type: JobType,
    pub start: Option<Time>,
    pub end: Option<Time>,
    pub start_location: Option<String>,
    pub end_location: Option<String>, // If none, it's the same as start
    pub omloop: Option<usize>,
    pub rit: Option<usize>,
}

impl ShiftJob {
    pub fn empty(&self) -> bool {
        if self.job_type == JobType::Unknown
            && self.start.is_none()
            && self.end.is_none()
            && self.start_location.is_none()
            && self.end_location.is_none()
        {
            return true;
        }
        false
    }
}

#[derive(Debug, Serialize, Clone, Deserialize)]
pub struct Shift {
    pub shift_nr: String,
    pub valid_on: ShiftValid,
    pub valid_days: Vec<ShiftValidDay>,
    pub location: String,
    pub shift_type: Option<ShiftType>,
    pub job: Vec<ShiftJob>,
    pub starting_date: Date,
    pub parse_error: Option<Vec<ShiftParseError>>,
}

impl Shift {
    pub fn load(timetable: &PdfTimetableCollection, id: &str) -> Result<Self> {
        Ok(serde_json::from_str::<Self>(&Self::load_json(
            timetable, id,
        )?)?)
    }

    pub fn load_json(timetable: &PdfTimetableCollection, id: &str) -> Result<String> {
        let base_path = timetable.start_date.format(DATE_FORMAT)?;
        let path = PathBuf::from(format!(
            "{COLLECTION_PATH}/{base_path}/{SHIFT_PATH}/{id}.json"
        )); // Because we don't have the shift itself yet, we need to recreate the path and get the date from the timetable collection
        Ok(fs::read_to_string(path)?)
    }

    pub fn save_extracted_shifts(shifts: Vec<Self>) -> Result<()> {
        if let Some(shift) = shifts.first() {
            let path = Self::create_shift_path(shift);
            match std::fs::create_dir_all(&path) {
                Ok(_) => (),
                Err(kind) if kind.kind() == io::ErrorKind::AlreadyExists => (),
                Err(kind) => return Err(kind.into()),
            };
            for shift in shifts {
                let shift_json = serde_json::to_string_pretty(&shift)?;
                let shift_number: String = shift
                    .shift_nr
                    .chars()
                    .filter(|character| character.is_numeric())
                    .collect();
                let mut shift_path = path.clone();
                shift_path.push(shift_number);
                shift_path.set_extension("json");
                fs::write(shift_path, shift_json)?;
            }
            Ok(())
        } else {
            Err(eyre!("No shifts contained"))
        }
    }

    fn create_shift_path(&self) -> PathBuf {
        let start_date = self.starting_date.format(DATE_FORMAT).unwrap();
        PathBuf::from(format!("{COLLECTION_PATH}/{start_date}/{SHIFT_PATH}"))
    }
}
