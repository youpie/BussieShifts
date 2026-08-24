<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://brainmade.org/white-logo.svg">
  <source media="(prefers-color-scheme: light)" srcset="https://brainmade.org/black-logo.svg">
  <img align="right" alt="Brain mark." src="https://brainmade.org/black-logo.svg">
</picture>

# BussieShift
Parse en serve Transdev dienstbriefjes

# How to add dienstbriefjes
1. create a folder `Dienstboek`
2. Add all dienstbriefjes to the folders
    * It is recommended to seperate every timetable to their own folder
3. _Optional:_ Add valid_on.txt to timetables (More details in down below)
4. Visit [0.0.0.0:8080/refresh](0.0.0.0:8080/refresh) to load everything
5. party

# How to use valid_on.txt
Sometimes a timetable is valid again after another timetable was valid, in this case valid_on.txt can be used to tell BussieShift that a timetable is valid multiple times. If a file is started with a `.` it means the valid on date included on the PDF itself should be ignored. Every line after that should include a date in `DD-MM-YYYY` format. On that day the shift will be treated as new

**Example**
```
.
12-12-2026
01-02-2027
...
```

# How to use locations.txt

If looking for deadheads, multiple destinations actually lead to the same destination. In this file that can be specified. The file can be found at `./locations.toml`.
This file also defines the Concession that are supported by Bussie

**Example**
```
# Concession name
> Location name
Destination name 1
Destination name 2
...
```