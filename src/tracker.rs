use fsdparser::{ATCPosition, PilotPosition};
use std::{collections::{HashMap, HashSet}};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn get_time() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn convert_lat_to_miles(lat: f32) -> f32 {
    return lat * 69.0
}

fn convert_lon_to_miles(lon: f32) -> f32 {
    return lon * 54.6
}

struct LatLonPos {
    lat: f32,
    lon: f32
}

impl LatLonPos {
    pub fn new(lat: f32, lon: f32) -> Self {
        Self {
            lat, lon
        }
    }

    pub fn distance_from_in_miles(&self, other: Self) -> f32 {
        let lat_diff_miles = convert_lat_to_miles((other.lat-self.lat).abs());
        let lon_diff_miles = convert_lon_to_miles((other.lon-self.lon).abs());
        return ((lat_diff_miles.powi(2)) + (lon_diff_miles.powi(2))).sqrt();
    }
}

pub struct Tracker {
    callsign: String,
    callsign_last_updated: Instant,

    current_atc_position: Option<ATCPosition>,
    current_pilot_position: Option<PilotPosition>,
    position: Option<LatLonPos>,

    pilots_seen: HashSet<String>,
    // Regarding handoffs/tracks
    tracked_aircraft: HashSet<String>,
    // Regarding squawks/strips
    counter_set: HashMap<String, i64>,

    // Counters
    number_squawks: u32,
    number_strips: u32,
    number_handoffs: u32
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            callsign: "".to_string(),
            callsign_last_updated: Instant::now(),

            current_atc_position: None,
            current_pilot_position: None,
            position: None,

            pilots_seen: HashSet::new(),
            tracked_aircraft: HashSet::new(),
            counter_set: HashMap::new(),

            number_squawks: 0,
            number_strips: 0,
            number_handoffs: 0
        }
    }

    fn can_increment_counter(&self, callsign: &String) -> bool {
        if let Some(last_time) = self.counter_set.get(callsign) {
            let now = get_time();
            return now-last_time > 60;
        } else {
            return true;
        }
    }

    fn set_cooldown(&mut self, callsign: &String) {
        self.counter_set.insert(callsign.to_string(), get_time());
    }

    fn set_position(&mut self, lat: f32, lon: f32) {
        self.position = Some(LatLonPos::new(lat, lon));
    }

    // Returns true if the callsign didn't match the current callsign
    pub fn update_callsign(&mut self, new_callsign: String) -> bool {
        let was_updated = new_callsign.ne(&self.callsign);

        self.callsign = new_callsign;
        self.callsign_last_updated = Instant::now();

        return was_updated;
    }

    pub fn update_atc_position(&mut self, position: ATCPosition) -> bool {
        let did_update = self.update_callsign(position.callsign.to_string());
        self.set_position(position.lat, position.lon);
        self.current_atc_position = Some(position);
        self.current_pilot_position = None;
        return did_update;
    }

    pub fn update_pilot_position(&mut self, position: PilotPosition) -> bool {
        let did_update = self.update_callsign(position.callsign.to_string());
        self.set_position(position.lat, position.lon);
        self.current_pilot_position = Some(position);
        self.current_atc_position = None;
        return did_update;
    }

    pub fn tracked(&mut self, callsign: &String) {
        self.tracked_aircraft.insert(callsign.to_string());
    }

    pub fn drop_tracked(&mut self, callsign: &String) {
        self.tracked_aircraft.remove(callsign);
    }

    pub fn assigned_squawk(&mut self, callsign: &String) {
        if !self.can_increment_counter(callsign) {return}
        self.set_cooldown(callsign);
        self.number_squawks += 1
    }

    pub fn pushed_strip(&mut self, callsign: &String) {
        if !self.can_increment_counter(callsign) {return}
        self.set_cooldown(callsign);
        self.number_strips += 1
    }

    pub fn handoff(&mut self, callsign: &String) {
        if !self.can_increment_counter(callsign) {return}
        self.tracked(callsign);
        self.set_cooldown(callsign);
        self.number_handoffs += 1
    }

    pub fn add_pilot(&mut self, position: &PilotPosition) {
        if let Some(pos) = self.position.as_ref() {
            if let Some(atc_pos) = self.current_atc_position.as_ref() {
                if pos.distance_from_in_miles(LatLonPos::new(position.lat, position.lon)) < atc_pos.vis_range as f32 {
                    self.pilots_seen.insert(position.callsign.to_string());
                }
                else {
                    self.remove_pilot(&position.callsign);
                    return
                }
            } else {
                self.pilots_seen.insert(position.callsign.to_string());
            }
        }
    }

    pub fn remove_pilot(&mut self, callsign: &String) {
        self.pilots_seen.remove(callsign);
        self.tracked_aircraft.remove(callsign);
    }

    // Getters
    pub fn get_number_seen(&self) -> usize {
        return self.pilots_seen.len()
    }

    pub fn get_number_squawks(&self) -> u32 {
        return self.number_squawks
    }

    pub fn get_number_strips(&self) -> u32 {
        return self.number_strips
    }

    pub fn get_number_tracked(&self) -> usize {
        return self.tracked_aircraft.len()
    }

    pub fn get_number_handoffs(&self) -> u32 {
        return self.number_handoffs
    }

    pub fn is_atc(&self) -> bool {
        return self.current_atc_position.is_some();
    }

    pub fn is_connected(&self) -> bool {
        return self.current_atc_position.is_some() || self.current_pilot_position.is_some()
    }

    pub fn get_atc_position(&self) -> Option<&ATCPosition> {
        return self.current_atc_position.as_ref();
    }

    pub fn get_current_callsign(&self) -> &String {
        return &self.callsign;
    }

    pub fn get_secs_since_last_callsign(&self) -> u64 {
        return self.callsign_last_updated.elapsed().as_secs();
    }

    pub fn reset(&mut self) {
        self.current_atc_position = None;
        self.current_pilot_position = None;
        self.number_squawks = 0;
        self.number_strips = 0;
        self.tracked_aircraft.clear();
        self.pilots_seen.clear();
        self.counter_set.clear();
    }
}