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

use ouroboros::self_referencing;

use crate::prelude::*;

type GeneralLocation<'a> = &'a str;
type SpecificLocation<'a> = &'a str;

#[self_referencing]
struct DeadheadLocations {
    locations_vec: Vec<(String, Vec<String>)>,
    #[borrows(locations_vec)]
    #[covariant]
    specific_to_general_map: HashMap<SpecificLocation<'this>, GeneralLocation<'this>>,
    #[borrows(locations_vec)]
    #[covariant]
    general_to_specific_map: HashMap<GeneralLocation<'this>, SpecificLocation<'this>>,
}

impl DeadheadLocations {
    fn load() -> Result<Self> {
        let path = PathBuf::from("locations.txt");
        let locations_file = fs::read_to_string(path)?;
        let mut locations_unit = Self {
            locations_vec: Self::parse_locations_file(locations_file),
            specific_to_general_map: HashMap::new(),
            general_to_specific_map: HashMap::new(),
        };
        locations_unit.locations_vec.iter().for_each(|g| {
            g.1.iter().for_each(|spec| {
                locations_unit
                    .specific_to_general_map
                    .insert(g.0.as_str(), spec.as_str());
                locations_unit
                    .general_to_specific_map
                    .insert(spec.as_str(), g.0.as_str());
            })
        });

        Ok(locations_unit)
    }

    fn parse_locations_file(file: String) -> Vec<(String, Vec<String>)> {
        let mut locations = Vec::new();
        let mut location: (String, Vec<String>) = (String::new(), Vec::new());
        let mut general_found = false;
        for line in file.lines() {
            if line.contains("> ") {
                let specific_location = line.replace("> ", "");
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
        locations
    }
}
