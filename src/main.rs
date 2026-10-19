use crate::parsing::shift::Shift;
use crate::parsing::timetable::{ShiftEntry, TTBOptions, TimetableCollection};
use crate::parsing::timetable_paths::TimetablePaths;
use crate::parsing::{deadhead::*, omloop::*};
use crate::statistics::handle_stats_request;

use actix_web::http::header::ContentType;
use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use color_eyre::eyre;
use index::handle_index_request;
use qpdf::QPdf;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use time::Date;
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use utoipa_actix_web::AppExt;

use crate::parsing::*;
pub use crate::prelude::*;

pub mod prelude;
#[macro_use]
mod error;
mod index;
mod parsing;
mod statistics;

const COLLECTION_PATH: &str = "pdf_collection";
const SHIFT_PATH: &str = "shifts";

const BOOKS_PATH: &str = "Dienstboek";

const CHANGE_FOLDER_NAME: &str = "Wijzigingen";

const DATE_FORMAT: &[BorrowedFormatItem<'_>] = format_description!["[day]-[month]-[year]"];

#[derive(Deserialize)]
struct ShiftQuery {
    date: Option<String>, // Optional date query parameter
}

fn handle_refresh_request() -> HttpResponse {
    match parsing::parse_dienstboekjes() {
        Ok(_) => HttpResponse::Accepted().body("Shifts sucessfully indexed"),
        Err(_err) => HttpResponse::InternalServerError().body(format!(
            "Shifts could not be indexed. Check logs for more details"
        )),
    }
}

#[utoipa::path(responses((status = OK, body = Shift)))]
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
    let mut valid_timetables = match TimetableCollection::get_valid_timetables(
        custom_date_option,
        add_upcoming_timetables,
    ) {
        Ok(result) => result.0,
        Err(err) => return return_error(err),
    };

    let mut shift_split = request_uppercase.split(".");
    let shift = shift_split.next().unwrap_or(&request_uppercase);
    let request_extension_option = shift_split.next();

    let shift_prefix: String = shift.chars().filter(|c| c.is_alphabetic()).collect();
    let numeric_shift_number: String = shift.chars().filter(|c| c.is_numeric()).collect();

    let (shift_collection, shift_data) =
        match ShiftEntry::find_shift(&numeric_shift_number, &mut valid_timetables) {
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
    shift_timetable_collection: &TimetableCollection,
    shift_data: ShiftEntry,
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
    tracing_subscriber::fmt::init();
    // Load shift data
    info!("Indexing trip sheets");
    // Get the hash of all files in the folder. If anything changes, the hash changes and so it will reindex
    let mut s = DefaultHasher::new();
    let files = TimetablePaths::get().expect("Failed to get timetable files");
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
                parsing::parse_dienstboekjes().unwrap();
            } else {
                info!("Hash is the same, so wont reindex");
                if let Err(e) = TimetableCollection::load_to_global() {
                    warn!("Loading the timetables has failed with errror {e:#?}. Will Reindex");
                    parsing::parse_dienstboekjes().unwrap();
                }
            }
        } else {
            error!("Could not find previous hash, reindexing");
            parsing::parse_dienstboekjes().unwrap();
            TimetableCollection::load_to_global().unwrap();
        }
    }
    #[cfg(debug_assertions)]
    {
        parsing::parse_dienstboekjes().unwrap();
    }
    let _ = fs::write("pdf_hash", current_hash.to_le_bytes());

    HttpServer::new(move || {
        let (server, api) = App::new()
            .into_utoipa_app()
            .service(get_shift)
            .service(get_omloop_overview)
            .service(get_omloop)
            .service(get_deadheads)
            .service(get_locations)
            .service(vehicle::get_all_vehicles)
            .service(vehicle::get_vehicles_for_shift)
            .service(vehicle::get_vehicles_for_omloop)
            .split_for_parts();
        _ = std::fs::write("./doc.json", serde_json::to_string_pretty(&api).unwrap());
        server
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
