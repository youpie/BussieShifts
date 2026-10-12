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
            let vehicles: Vec<_> = omloop_index
                .get_all_omloop(weekday)
                .into_iter()
                .filter_map(|omloop| {
                    omloop_index
                        .get_omloop(omloop, weekday)
                        .map(|collection| {
                            collection.vehicle.map(|vehicle_type| Vehicle {
                                vehicle_type,
                                omloop,
                            })
                        })
                        .ok()
                        .flatten()
                })
                .collect();
            let shift_collection = ShiftOmlopen::new(timetable);
            let vehicle_collection = Self::new(weekday as u8, shift_collection, vehicles);
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
    vehicle_type: String,
}
