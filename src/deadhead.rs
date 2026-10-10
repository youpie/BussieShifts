/*
Hoe gaat dit werken
POD zoeker hoeft in principe maar 1 keer gegenereerd te worden per dienstregeling. Het is statische data
Een mat rit verschilt wel per dag, wanwege dienstregelingen & valid on dagen. Maar in principe kan het gewoon een lijst zijn die gerserveerd wordt met een request
Wel moeten meerdere bestemmingen samengevoegd worden met behulp van een soort look-up table (ehvsnel, ehvtraag, ehvgar, ehvgas) zijn allemaal eindhoven garague
misschien in het volgende formaat:
[Eindhoven garage]
ehvsnel
ehvtraag
...

[Eindhoven Busstation]
ehvbst
...


*/

use std::{collections::HashMap, fs, path::PathBuf};

use actix_web::{HttpResponse, get, http::header::ContentType, web};
use time::{Time, Weekday};
use tracing::info_span;

use crate::{
    collection::PdfTimetableCollection,
    omloop::{
        BusOmloopDay, OmloopCollectionIO, OmloopDayIndex, ShiftJobExtended, get_dow_and_timetable,
    },
    prelude::*,
    return_error,
};

type GeneralLocation = String;
type SpecificLocation = String;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DeadheadLocations {
    locations_vec: Vec<(String, Vec<String>)>,
    specific_to_general_map: HashMap<SpecificLocation, GeneralLocation>,
    // general_to_specific_map: HashMap<GeneralLocation, SpecificLocation>,
}

impl DeadheadLocations {
    pub fn load() -> Result<Self> {
        let path = PathBuf::from("locations.txt");
        let locations_file = fs::read_to_string(path)?;
        let mut locations_unit = Self {
            locations_vec: Self::parse_locations_file(locations_file),
            specific_to_general_map: HashMap::new(),
            // general_to_specific_map: HashMap::new(),
        };
        locations_unit.locations_vec.iter().for_each(|g| {
            g.1.iter().for_each(|spec| {
                // locations_unit
                //     .specific_to_general_map
                //     .insert(g.0.clone(), spec.clone());
                locations_unit
                    .specific_to_general_map
                    .insert(spec.clone(), g.0.clone());
            })
        });

        Ok(locations_unit)
    }

    pub fn debug_locations(&self) {
        for location in self.locations_vec.iter().enumerate() {
            debug!("General location {}: {}", location.0 + 1, location.1.0);
            for specific_location in &location.1.1 {
                debug!("  - {specific_location}");
            }
        }
    }

    fn parse_locations_file(file: String) -> Vec<(String, Vec<String>)> {
        let mut locations = Vec::new();
        let mut location: (String, Vec<String>) = (String::new(), Vec::new());
        let mut general_found = false;
        for line in file.lines() {
            if line.contains("> ") {
                let specific_location = line.replace("> ", "");
                debug!("Found location {specific_location}");
                location.0 = specific_location;
                general_found = true;
            } else if line.is_empty() && general_found {
                locations.push(location);
                general_found = false;
                location = (String::new(), Vec::new());
            } else if general_found {
                location.1.push(line.to_owned());
            }
        }

        if general_found {
            locations.push(location);
        }

        locations
    }

    pub fn get_general_location(&self, specific_location: &str) -> Option<&str> {
        self.specific_to_general_map
            .get(specific_location)
            .map(|v| v.as_str())
    }

    // pub fn get_specific_location(&self, general_location: &str) -> Option<&str> {
    //     self.general_to_specific_map
    //         .get(general_location)
    //         .map(|v| v.as_str())
    // }
}

#[derive(Debug, Serialize, Deserialize)]
struct DeadheadLocation<'a, 'b> {
    general: Option<&'b str>,
    specific: &'a str,
}

impl<'a, 'b> DeadheadLocation<'a, 'b> {
    pub fn new(specific_location: &'a str, deadhead_locations: &'b DeadheadLocations) -> Self {
        Self {
            general: deadhead_locations.get_general_location(&specific_location),
            specific: specific_location,
        }
    }

    pub fn is_unknown_location(&self) -> bool {
        self.general.is_none()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Deadhead<'a, 'b> {
    #[serde(borrow)]
    from: DeadheadLocation<'a, 'b>,
    #[serde(borrow)]
    to: DeadheadLocation<'a, 'b>,
    start: Time,
    end: Time,
    omloop: Omloop,
    shift: &'a str,
}

impl<'a, 'b> Deadhead<'a, 'b> {
    fn from_extended_job(
        job: &'a ShiftJobExtended,
        omloop: Omloop,
        deadhead_locations: &'b DeadheadLocations,
    ) -> Self {
        Self {
            from: DeadheadLocation::new(&job.start_location.as_ref().unwrap(), deadhead_locations),
            to: DeadheadLocation::new(&job.end_location.as_ref().unwrap(), deadhead_locations),
            start: job.start.unwrap_or(Time::MAX),
            end: job.end.unwrap_or(Time::MAX),
            omloop,
            shift: &job.shift,
        }
    }

    fn is_unknown_location(&self) -> bool {
        self.from.is_unknown_location() || self.to.is_unknown_location()
    }

    pub fn from_omloop(
        omloop_map: &'a BusOmloopDay,
        deadhead_locations: &'b DeadheadLocations,
    ) -> Vec<Self> {
        let span = info_span!("", omloop_map.omloop);
        let _guard = span.enter();

        trace!("Loading deadheads");
        let omloop = omloop_map.omloop;
        let deadheads = omloop_map.get_filled_deadheads();
        let mapped_deadheads: Vec<Deadhead> = deadheads
            .into_iter()
            .filter_map(
                |d| match Self::from_extended_job(d, omloop, deadhead_locations) {
                    val if val.is_unknown_location() => None,
                    val => Some(val),
                },
            )
            .collect();
        mapped_deadheads
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DeadheadCollection<'a> {
    #[serde(borrow)]
    deadheads: Vec<Deadhead<'a, 'a>>,
}

impl<'a> DeadheadCollection<'a> {
    pub fn new(deadheads: Vec<Deadhead<'a, 'a>>) -> Self {
        Self { deadheads }
    }

    pub fn for_general_location(
        &'a self,
        // from_general_location: &str,
        to_general_location: &str,
    ) -> Vec<&'a Deadhead<'a, 'a>> {
        let mut matching_deadheads = Vec::new();
        for deadhead in &self.deadheads {
            // if deadhead.from.general == Some(from_general_location)
            if deadhead.to.general == Some(to_general_location) {
                matching_deadheads.push(deadhead);
            }
        }
        matching_deadheads
    }

    pub fn load_byes_from_general_location(
        weekday: Weekday,
        location: &'a str,
        timetable: &PdfTimetableCollection,
    ) -> Result<Vec<u8>> {
        let mut path = Self::get_path(weekday as usize, timetable.base_start(), location);
        path.set_extension("bin");
        Ok(fs::read(path)?)
    }

    pub fn load_deadheads_from_file(bytes: &'a Vec<u8>) -> Result<DeadheadCollection<'a>> {
        Ok(postcard::from_bytes::<DeadheadCollection<'a>>(
            bytes.as_slice(),
        )?)
    }

    pub fn for_every_weekday(
        deadhead_locations: &DeadheadLocations,
        timetable: &PdfTimetableCollection,
    ) -> Result<()> {
        let mut omlopen_per_day = Vec::new();
        for i in [
            Weekday::Monday,
            Weekday::Tuesday,
            Weekday::Wednesday,
            Weekday::Thursday,
            Weekday::Friday,
            Weekday::Saturday,
            Weekday::Sunday,
        ] {
            let omlopen_on_day = (i, OmloopDayIndex::get_all_omloop(i, timetable)?);
            omlopen_per_day.push(omlopen_on_day);
        }

        for day in omlopen_per_day {
            let omlopen = day
                .1
                .into_iter()
                .map(|omloop| OmloopDayIndex::get_omloop(omloop, day.0, timetable))
                .collect::<Result<Vec<_>>>()?;

            let mut deadheads_this_day = DeadheadCollection::default();
            for omloop in &omlopen {
                let deadheads = DeadheadCollection::from_omloop(omloop, deadhead_locations);
                deadheads_this_day.combine(deadheads);
            }
            deadheads_this_day.deadheads.sort_by_key(|i| i.start);
            deadheads_this_day
                .split_by_general_location()
                .into_iter()
                .map(|v| v.1.save(day.0 as usize, timetable.base_start(), v.0))
                .collect::<Result<()>>()
                .warn("saving deadheads for a location");
        }
        Ok(())
    }

    pub fn from_omloop(
        omloop_map: &'a BusOmloopDay,
        deadhead_locations: &'a DeadheadLocations,
    ) -> Self {
        let deadheads = Deadhead::from_omloop(omloop_map, deadhead_locations);
        Self::new(deadheads)
    }

    fn split_by_general_location(self) -> Vec<(&'a str, Self)> {
        let mut locations_map: HashMap<&str, Vec<Deadhead>> = HashMap::new();
        for deadhead in self.deadheads {
            if let Some(location) = deadhead.from.general
                && !deadhead.to.is_unknown_location()
            {
                locations_map.entry(location).or_default().push(deadhead);
            }
        }

        locations_map
            .into_iter()
            .map(|v| (v.0, Self::new(v.1)))
            .collect::<Vec<_>>()
    }

    fn combine(&mut self, new_collection: Self) -> &Self {
        for deadhead in new_collection.deadheads {
            self.deadheads.push(deadhead);
        }
        self
    }
}

impl<'a> OmloopCollectionIO for DeadheadCollection<'a> {
    const PATH: &str = "deadheads";
    type IndexType = &'a str;
}

#[derive(Deserialize)]
struct DeadheadQuery {
    date: Option<String>,
    from: String,
    to: String,
}

#[get("/deadheads")]
pub async fn get_deadheads(query: web::Query<DeadheadQuery>) -> HttpResponse {
    let (weekday, timetable) = match get_dow_and_timetable(query.date.as_deref()) {
        Ok(v) => v,
        Err(v) => return v,
    };

    match DeadheadCollection::load_byes_from_general_location(weekday, &query.from, &timetable)
        .map(|v| {
            DeadheadCollection::load_deadheads_from_file(&v).map(|i| {
                Ok(serde_json::to_string_pretty(
                    &i.for_general_location(&query.to),
                )?)
            })
        })
        .flatten()
        .flatten()
    {
        Ok(v) => HttpResponse::Ok().content_type(ContentType::json()).body(v),
        Err(e) => return_error(e),
    }
}

#[get("/deadheads/locations")]
pub async fn get_locations() -> HttpResponse {
    match DeadheadLocations::load()
        .map(|v| Ok(serde_json::to_string(&v.locations_vec)?))
        .flatten()
    {
        Ok(val) => HttpResponse::Ok()
            .content_type(ContentType::json())
            .body(val),
        Err(e) => return_error(e),
    }
}
