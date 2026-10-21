use actix_web::{HttpResponse, Responder, get, http::header::ContentType, web};
use color_eyre::eyre;
use derive_new::new;
use time::{Date, OffsetDateTime};

use crate::{
    find_pdf_shift, parsing,
    prelude::*,
    shift::Shift,
    timetable::{ShiftEntry, TTBOptions, TimetableCollection},
};

#[derive(Deserialize, new)]
pub struct ShiftQuery {
    date: Option<String>, // Optional date query parameter
}

impl ShiftQuery {
    pub fn get_date(&self) -> Date {
        self.get_date_option()
            .unwrap_or(OffsetDateTime::now_utc().date())
    }

    pub fn get_date_option(&self) -> Option<Date> {
        self.date
            .as_ref()
            .and_then(|date_string| Date::parse(date_string, DATE_FORMAT).ok())
    }
}

#[utoipa::path(responses((status = OK)))]
#[get("/refresh")]
async fn handle_refresh_request() -> HttpResponse {
    match parsing::parse_dienstboekjes() {
        Ok(_) => HttpResponse::Accepted().body("Shifts sucessfully indexed"),
        Err(_err) => HttpResponse::InternalServerError().body(format!(
            "Shifts could not be indexed. Check logs for more details"
        )),
    }
}

#[utoipa::path(responses((status = OK, body = Shift)))]
#[get("/shift/{shift_number}")]
pub async fn get_shift(
    request: web::Path<String>,
    query: web::Query<ShiftQuery>,
) -> impl Responder {
    debug!("Got request for {}", request);

    let custom_date_option = query.get_date_option();

    let request_uppercase = request.to_uppercase();

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

pub fn return_error(error: eyre::Report) -> HttpResponse {
    warn!("Error occured: {error:#?}");
    HttpResponse::InternalServerError().body(format!(
        "<h1>Sorry, something went wrong loading that shift.</h1><br>error: {}",
        error
    ))
}
