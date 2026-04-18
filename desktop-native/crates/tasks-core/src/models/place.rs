use rusqlite::Row;
use serde::{Deserialize, Serialize};

/// Mirrors `org.tasks.data.entity.Place` (table `places`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: i64,
    pub uid: Option<String>,
    pub name: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub url: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub color: i32,
    pub icon: Option<String>,
    pub order: i32,
    pub radius: i32,
}

impl Place {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Place {
            id: row.get("place_id")?,
            uid: row.get("uid")?,
            name: row.get("name")?,
            address: row.get("address")?,
            phone: row.get("phone")?,
            url: row.get("url")?,
            latitude: row.get("latitude")?,
            longitude: row.get("longitude")?,
            color: row.get("place_color")?,
            icon: row.get("place_icon")?,
            order: row.get("place_order")?,
            radius: row.get("radius")?,
        })
    }

    /// Mirrors Place.displayName: prefer name if not a raw coordinate pair,
    /// else address, else a formatted coordinate string.
    pub fn display_name(&self) -> String {
        if let Some(name) = self.name.as_deref().filter(|n| !n.is_empty() && !is_coord_string(n)) {
            return name.to_string();
        }
        if let Some(address) = self.address.as_deref().filter(|a| !a.is_empty()) {
            return address.to_string();
        }
        format!("{:.6}, {:.6}", self.latitude, self.longitude)
    }
}

fn is_coord_string(s: &str) -> bool {
    // Matches Place.COORDS: e.g. `37°46'26.4"N 122°25'51.6"W`. We don't need
    // character-perfect parity with the Android regex — a cheap prefix check
    // is enough to avoid rendering the coordinate form as the display name.
    s.contains('°') && (s.contains('"') || s.contains('\''))
}

/// Mirrors `org.tasks.data.entity.Geofence` (table `geofences`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geofence {
    pub id: i64,
    pub task: i64,
    pub place: Option<String>,
    pub arrival: bool,
    pub departure: bool,
}

impl Geofence {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Geofence {
            id: row.get("geofence_id")?,
            task: row.get("task")?,
            place: row.get("place")?,
            arrival: row.get::<_, i32>("arrival")? != 0,
            departure: row.get::<_, i32>("departure")? != 0,
        })
    }
}
