use std::collections::HashMap;

use actix_web::{HttpResponse, get, http::header::ContentType, web};
use derive_new::new;
use time::Weekday;

use super::*;
use crate::prelude::*;

#[derive(Debug, Serialize, Deserialize, Default)]
struct ShiftOmlopen {
    shift: usize,
    omlopen: Vec<Omloop>,
}

impl ShiftOmlopen {
    pub fn new(timetable: &TimetableCollection) -> Vec<Self> {
        let mut shifts_vec = Vec::new();
        for shift in &timetable.shifts {
            let shift_nr = shift.0.to_number();
            let shift = shift.1;

            let mut omlopen: HashMap<Omloop, ()> = HashMap::new();
            // add every omloop of a shift to a hashmap.
            // Where if the omloop is already in the map it does nothing
            shift.job.iter().for_each(|j| {
                if let Some(omloop) = j.omloop.or(j.assumed_omloop) {
                    omlopen.insert(omloop, ());
                }
            });
            let omlopen: Vec<usize> = omlopen.into_keys().collect();
            shifts_vec.push(Self {
                shift: shift_nr,
                omlopen,
            });
        }
        shifts_vec
    }
}

#[derive(Debug, Serialize, Deserialize, Default, new)]
pub struct VehicleCollection {
    weekday: u8,
    vehicle_map: HashMap<u32, String>,
    shifts_omlopen: Vec<ShiftOmlopen>,
    vehicles: Vec<Vehicle>,
}

impl VehicleCollection {
    pub fn parse(
        timetable: &TimetableCollection,
        omloop_index: &OmloopDayIndex,
    ) -> Result<Vec<Self>> {
        let mut vehicle_collections = Vec::new();

        for weekday in Weekday::get_all() {
            let mut vehicle_map = VehicleMap::default();
            let mut vehicles: Vec<_> = omloop_index
                .get_all_omloop(weekday)
                .into_iter()
                .filter_map(|omloop| {
                    omloop_index
                        .get_omloop(omloop, weekday)
                        .map(|collection| {
                            let vehicle_id = vehicle_map.insert(collection.vehicle_type);
                            Vehicle { vehicle_id, omloop }
                        })
                        .ok()
                })
                .collect();

            let inverted_map = vehicle_map.create_map();
            vehicles.filter_vehicles(&inverted_map);

            let shift_collection = ShiftOmlopen::new(timetable);

            let vehicle_collection =
                Self::new(weekday as u8, inverted_map, shift_collection, vehicles);

            vehicle_collection
                .save(0, timetable.base_start(), weekday)
                .warn("Saving vehicle type");

            vehicle_collections.push(vehicle_collection);
        }
        Ok(vehicle_collections)
    }

    pub fn load(start_date: Date, index: Weekday) -> Result<Self> {
        let mut path = Self::get_path(0, start_date, index);
        path.set_extension("bin");
        Ok(postcard::from_bytes(fs::read(path)?.as_slice())?)
    }

    pub fn get_vehicles_for_omlopen(&self, omlopen: Vec<Omloop>) -> Vec<(Omloop, String)> {
        let mut collection = Vec::new();
        for omloop in omlopen {
            let vehicle = self
                .vehicles
                .iter()
                .find(|i| i.omloop == omloop)
                .map(|v| {
                    v.vehicle_id
                        .first()
                        .map(|id| self.vehicle_map.get(id).cloned())
                        .flatten()
                })
                .flatten()
                .unwrap_or("Unknown".to_owned());
            collection.push((omloop, vehicle));
        }
        collection
    }

    pub fn get_all_vehicles(start_date: Date) -> Result<Vec<String>> {
        let mut vehicles = HashMap::new();
        for weekday in Weekday::get_all() {
            let collection = Self::load(start_date, weekday)?;
            collection
                .vehicle_map
                .into_values()
                .into_iter()
                .for_each(|v| {
                    vehicles.insert(v.clone().to_uppercase(), v);
                });
        }
        Ok(vehicles.into_values().collect())
    }
}

impl OmloopCollectionIO for VehicleCollection {
    const PATH: &str = "vehicles";

    type IndexType = Weekday;
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Vehicle {
    omloop: Omloop,
    vehicle_id: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum VehicleCertainty {
    Certain(String),
    Uncertain(String),
    #[default]
    Unknown,
}

impl VehicleCertainty {
    pub fn into_uppercase(&self) -> Self {
        match self.clone() {
            VehicleCertainty::Certain(v) => VehicleCertainty::Certain(v.to_uppercase()),
            VehicleCertainty::Uncertain(v) => VehicleCertainty::Uncertain(v.to_uppercase()),
            VehicleCertainty::Unknown => VehicleCertainty::Unknown,
        }
    }

    pub fn is_certain(&self) -> bool {
        match self {
            Self::Certain(_) => true,
            _ => false,
        }
    }

    pub fn is_unknown(&self) -> bool {
        match self {
            Self::Unknown => true,
            _ => false,
        }
    }

    pub fn to_string(self) -> String {
        match self {
            VehicleCertainty::Certain(v) => v,
            VehicleCertainty::Uncertain(v) => v,
            VehicleCertainty::Unknown => "Unknown".into(),
        }
    }
}

#[derive(Default, Clone)]
struct VehicleMap {
    map: HashMap<VehicleCertainty, (String, u32)>,
    counter: u32,
}

impl VehicleMap {
    pub fn create_map(self) -> HashMap<u32, String> {
        self.map
            .into_iter()
            .filter(|(k, _v)| k.is_certain())
            .map(|(_, v)| (v.1, v.0))
            .collect()
    }

    pub fn insert(&mut self, vehicles: Vec<VehicleCertainty>) -> Vec<u32> {
        let mut ids = Vec::new();
        for vehicle in vehicles {
            let uncertain =
                VehicleCertainty::Uncertain(vehicle.clone().to_string()).into_uppercase();
            if let Some(val) = self.map.get(&uncertain).cloned()
                && vehicle.is_certain()
            {
                self.map.remove(&uncertain);
                ids.push(val.1);
                self.map.insert(vehicle.into_uppercase(), val);
            } else if !vehicle.is_unknown() {
                let val = self
                    .map
                    .entry(vehicle.into_uppercase())
                    .or_insert_with(|| {
                        self.counter += 1;
                        (vehicle.to_string(), self.counter)
                    })
                    .clone();
                ids.push(val.1);
            }
        }
        ids
    }
}

trait FilterVehicles {
    fn filter_vehicles(&mut self, inverted_map: &HashMap<u32, String>);
}

impl FilterVehicles for Vec<Vehicle> {
    fn filter_vehicles(&mut self, inverted_map: &HashMap<u32, String>) {
        for vehicle in self {
            vehicle
                .vehicle_id
                .retain(|id| inverted_map.contains_key(id));
        }
    }
}

#[get("/vehicles/shift/{shift_number}")]
pub async fn get_vehicles_for_shift(
    request: web::Path<usize>,
    query: web::Query<ShiftQuery>,
) -> HttpResponse {
    let shift = request.into_inner();
    let (weekday, timetable) = match get_weekday_and_timetable(query.date.as_deref()) {
        Ok(v) => v,
        Err(v) => return v,
    };

    let vehicle = VehicleCollection::load(timetable.base_start(), weekday).unwrap();
    let omlopen = vehicle
        .shifts_omlopen
        .iter()
        .find(|f| f.shift == shift)
        .map(|v| v.omlopen.clone())
        .unwrap_or_default();
    match serde_json::to_string_pretty(&vehicle.get_vehicles_for_omlopen(omlopen)) {
        Ok(v) => HttpResponse::Ok().content_type(ContentType::json()).body(v),
        Err(e) => return_error(e.into()),
    }
}

#[get("/vehicles/omloop/{shift_number}")]
pub async fn get_vehicles_for_omloop(
    request: web::Path<usize>,
    query: web::Query<ShiftQuery>,
) -> HttpResponse {
    let omloop = request.into_inner();
    let (weekday, timetable) = match get_weekday_and_timetable(query.date.as_deref()) {
        Ok(v) => v,
        Err(v) => return v,
    };

    match VehicleCollection::load(timetable.base_start(), weekday)
        .map(|vc| {
            Ok(serde_json::to_string_pretty(
                &vc.get_vehicles_for_omlopen(vec![omloop])
                    .first()
                    .map(|v| v.1.to_owned()),
            )?)
        })
        .flatten()
    {
        Ok(v) => HttpResponse::Ok().content_type(ContentType::json()).body(v),
        Err(e) => return_error(e),
    }
}

#[get("/vehicles/index")]
pub async fn get_all_vehicles(query: web::Query<ShiftQuery>) -> HttpResponse {
    let (_, timetable) = match get_weekday_and_timetable(query.date.as_deref()) {
        Ok(v) => v,
        Err(v) => return v,
    };

    match VehicleCollection::get_all_vehicles(timetable.base_start())
        .map(|v| Ok(serde_json::to_string_pretty(&v)?))
        .flatten()
    {
        Ok(v) => HttpResponse::Ok().content_type(ContentType::json()).body(v),
        Err(e) => return_error(e),
    }
}
