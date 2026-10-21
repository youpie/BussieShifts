use crate::parsing::timetable::{ShiftEntry, TimetableCollection};
use crate::parsing::timetable_paths::TimetablePaths;

use actix_web::{App, HttpServer};
use qpdf::QPdf;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;

use crate::parsing::*;
pub use crate::prelude::*;

pub mod prelude;
#[macro_use]
mod error;
mod index;
mod parsing;
mod shift_endpoint;
mod statistics;

const COLLECTION_PATH: &str = "pdf_collection";
const SHIFT_PATH: &str = "shifts";

const BOOKS_PATH: &str = "Dienstboek";

const CHANGE_FOLDER_NAME: &str = "Wijzigingen";

const DATE_FORMAT: &[BorrowedFormatItem<'_>] = format_description!["[day]-[month]-[year]"];

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
            .service(shift_endpoint::get_shift)
            .service(shift_endpoint::handle_refresh_request)
            .service(statistics::handle_stats_request)
            .service(index::handle_index_request)
            .service(omloop::get_omloop_overview)
            .service(omloop::get_omloop)
            .service(deadhead::get_deadheads)
            .service(deadhead::get_locations)
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
