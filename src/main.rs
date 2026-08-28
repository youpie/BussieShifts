use crate::collection::{PdfTimetableCollection, ShiftData};
use crate::omloop::{OmloopDayIndex, get_omloop, get_omloop_overview};
use crate::parsing::shift_structs::Shift;
use crate::parsing::valid_on;
use crate::statistics::handle_stats_request;
use actix_web::http::header::ContentType;
use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use color_eyre::eyre;
use index::handle_index_request;
use qpdf::QPdf;
use serde::Deserialize;
use std::ffi::OsStr;
use std::fs::{self};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Component, PathBuf};
use std::time::SystemTime;
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{Date, OffsetDateTime};
use walkdir::WalkDir;

pub use crate::prelude::*;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

pub mod prelude;

mod collection;
mod deadhead;
#[macro_use]
mod error;
mod index;
mod omloop;
mod parsing;
mod statistics;

type ValidTimetables = Vec<PdfTimetableCollection>;
type NextTimetableChangeDate = Option<Date>;

const COLLECTION_PATH: &str = "pdf_collection";
const SHIFT_PATH: &str = "shifts";
const OMLOOP_PATH: &str = "omlopen";

const BOOKS_PATH: &str = "Dienstboek";

const CHANGE_FOLDER_NAME: &str = "Wijzigingen";

const DATE_FORMAT: &[BorrowedFormatItem<'_>] = format_description!["[day]-[month]-[year]"];

#[derive(Deserialize)]
struct ShiftQuery {
    date: Option<String>, // Optional date query parameter
}

#[derive(Hash, Debug)]
struct TimetablePaths {
    pub timetable_pdfs: Vec<PathBuf>,
    pub valid_from_files: Vec<PathBuf>,
    pub changes_files: Vec<PathBuf>,
}

fn get_timetable_pdf_files() -> Result<TimetablePaths> {
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
        .sort_by_key(|element| get_creation_date(element).unwrap_or(SystemTime::now()));
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

fn load_pdfs_and_index() -> Result<()> {
    let files = get_timetable_pdf_files()?;
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

    for timetable in &timetable_collections {
        OmloopDayIndex::new_omloop_timetable(timetable).unwrap();
    }

    PdfTimetableCollection::load_to_global()?;
    Ok(())
}

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

// load all pdf_collection files. And determine which one is current
// Also if it exists, save the date of when it gets invalidated (when the Next timetable starts)
// In reverse chronological order
// Upcoming timetables are placed in the beginning of the list, also in reverse order
fn get_valid_timetables(
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
    } else if options == TTBOptions::OnlyFirst {
        vec![current_timetables.pop().result_reason("No timetables")?]
    } else {
        current_timetables
    };

    Ok((active_timetables, next_timetable))
}

// Recursive function to find a valid shift
fn find_shift(
    shift_number: &str,
    valid_timetables: &mut Vec<PdfTimetableCollection>,
) -> Option<(PdfTimetableCollection, ShiftData)> {
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

    match current_timetable.clone().pages.get(shift_number) {
        Some(shift) => Some((current_timetable, shift.clone())),
        None => find_shift(shift_number, valid_timetables),
    }
}

fn handle_refresh_request() -> HttpResponse {
    match load_pdfs_and_index() {
        Ok(_) => HttpResponse::Accepted().body("Shifts sucessfully indexed"),
        Err(_err) => HttpResponse::InternalServerError().body(format!(
            "Shifts could not be indexed. Check logs for more details"
        )),
    }
}

#[get("/shift/{shift_number}")]
async fn get_shift(request: web::Path<String>, query: web::Query<ShiftQuery>) -> impl Responder {
    debug!("Got request for {}", request);
    let custom_date_option = query
        .date
        .as_ref()
        .and_then(|date_string| Date::parse(date_string, DATE_FORMAT).ok());

    let request_uppercase = request.to_uppercase();

    // Handle specific request
    if request_uppercase == "REFRESH" {
        return handle_refresh_request();
    } else if request_uppercase == "INDEX" {
        return handle_index_request(custom_date_option);
    } else if request_uppercase == "STATS" {
        return handle_stats_request(custom_date_option);
    }

    let add_upcoming_timetables =
        TTBOptions::from_bool(custom_date_option.is_some(), TTBOptions::AppendFuture);
    let mut valid_timetables =
        match get_valid_timetables(custom_date_option, add_upcoming_timetables) {
            Ok(result) => result.0,
            Err(err) => return return_error(err),
        };

    let mut shift_split = request_uppercase.split(".");
    let shift = shift_split.next().unwrap_or(&request_uppercase);
    let request_extension_option = shift_split.next();

    let shift_prefix: String = shift.chars().filter(|c| c.is_alphabetic()).collect();
    let numeric_shift_number: String = shift.chars().filter(|c| c.is_numeric()).collect();

    let (shift_collection, shift_data) =
        match find_shift(&numeric_shift_number, &mut valid_timetables) {
            Some(shift) => shift,
            None => {
                return HttpResponse::NotFound()
                    .body(format!("<h1>Sorry, shift {shift} was not found</h1>"));
            }
        };

    // Check for correct shift prefix
    if !shift_prefix.is_empty() && shift_prefix != shift_data.shift_prefix {
        // Add exceptions for the shift prefix check
        if !(shift_prefix == "GM" && shift_data.shift_prefix == "G"
            || shift_prefix == "G" && shift_data.shift_prefix == "GM")
        {
            return HttpResponse::NotAcceptable()
            .body(format!("<h1>Incorrect shift type specified.</h1> <br><h2>Please remove \"{shift_prefix}\" or change request to \"{}{numeric_shift_number}\"</h2>",shift_data.shift_prefix));
        }
    }

    if let Some(shift_extension) = request_extension_option
        && shift_extension == "JSON"
    {
        info!("Got JSON request for {request_uppercase}");
        match Shift::load_json(&shift_collection, &numeric_shift_number) {
            Ok(json) => HttpResponse::Ok()
                .content_type(ContentType::json())
                .body(json),
            Err(err) => return_error(err),
        }
    } else {
        info!("Got PDF request for shift {request_uppercase}");
        match find_pdf_shift(&shift_collection, shift_data) {
            Ok(bytes) => HttpResponse::Ok()
                .content_type("application/pdf")
                .body(bytes),
            Err(err) => return_error(err),
        }
    }
}

fn return_error(error: eyre::Report) -> HttpResponse {
    warn!("Error occured: {error:#?}");
    HttpResponse::InternalServerError().body(format!(
        "<h1>Sorry, something went wrong loading that shift.</h1><br>error: {}",
        error
    ))
}

fn find_pdf_shift(
    shift_timetable_collection: &PdfTimetableCollection,
    shift_data: ShiftData,
) -> Result<Vec<u8>> {
    // Get the path of the pdf by getting the file id of the shift data, and using that to find the filename
    let shift_pdf_path = shift_timetable_collection
        .files
        .get(&shift_data.file_id)
        .result_reason("No PDF found")?
        .to_owned();

    let shift_pages = shift_data.pages;
    let full_pdf = QPdf::read(shift_pdf_path)?;
    let shift_pdf = QPdf::empty();
    // Keep only the pages we want
    for page in shift_pages {
        let extracted_pages = full_pdf
            .get_page(page - 1)
            .result_reason("Shift page not found")?;
        shift_pdf.add_page(extracted_pages, false)?;
    }

    Ok(shift_pdf.writer().write_to_memory()?)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    pretty_env_logger::init();
    // Load shift data
    info!("Indexing trip sheets");
    // Get the hash of all files in the folder. If anything changes, the hash changes and so it will reindex
    let mut s = DefaultHasher::new();
    let files = get_timetable_pdf_files().expect("Failed to get timetable files");
    files.hash(&mut s);
    let current_hash = s.finish();
    let _previous_hash_option = fs::read("pdf_hash")
        .ok()
        .and_then(|bytes| Some(u64::from_le_bytes(bytes.try_into().unwrap())));
    // #[cfg(not(debug_assertions))]
    {
        if let Some(previous_hash) = _previous_hash_option {
            if previous_hash != current_hash {
                warn!("Hash is changed, reindexing files");
                load_pdfs_and_index().unwrap();
            } else {
                info!("Hash is the same, so wont reindex");
                if let Err(e) = PdfTimetableCollection::load_to_global() {
                    warn!("Loading the timetables has failed with errror {e:#?}. Will Reindex");
                    load_pdfs_and_index().unwrap();
                }
            }
        } else {
            error!("Could not find previous hash, reindexing");
            load_pdfs_and_index().unwrap();
            PdfTimetableCollection::load_to_global().unwrap();
        }
    }
    #[cfg(debug_assertions)]
    {
        load_pdfs_and_index().unwrap();
    }
    let _ = fs::write("pdf_hash", current_hash.to_le_bytes());

    HttpServer::new(move || {
        App::new()
            .service(get_shift)
            .service(get_omloop_overview)
            .service(get_omloop)
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
