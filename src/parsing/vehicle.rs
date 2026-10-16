use std::collections::HashMap;

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
    pub fn load(
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
    map: HashMap<VehicleCertainty, u32>,
    counter: u32,
}

impl VehicleMap {
    pub fn create_map(self) -> HashMap<u32, String> {
        self.map
            .into_iter()
            .filter(|(k, _v)| k.is_certain())
            .map(|(k, v)| (v, k.to_string()))
            .collect()
    }

    pub fn insert(&mut self, vehicles: Vec<VehicleCertainty>) -> Vec<u32> {
        let mut ids = Vec::new();
        for vehicle in vehicles {
            let uncertain = VehicleCertainty::Uncertain(vehicle.clone().to_string());
            if let Some(val) = self.map.get(&uncertain).cloned()
                && vehicle.is_certain()
            {
                self.map.remove(&uncertain);
                self.map.insert(vehicle, val);
                ids.push(val);
            } else if !vehicle.is_unknown() {
                let val = self
                    .map
                    .entry(vehicle)
                    .or_insert_with(|| {
                        self.counter += 1;
                        self.counter
                    })
                    .clone();
                ids.push(val);
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
