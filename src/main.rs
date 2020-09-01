use discord_game_sdk::{Discord, Activity, CreateFlags};
use fsdparser::{Sniffer, PacketSource, PacketTypes, ClientQueryPayload, ATCManager, PilotManager, NetworkClientType, ATCPosition, PilotPosition, NetworkClient, NetworkFacility};
use pnet::datalink::NetworkInterface;
use std::{collections::{HashMap, HashSet}, sync::Arc, time::{SystemTime, Duration, self}};
use std::{io::{Write, Read}, sync::Mutex};
use std::fs::File;
use systray::Application;
use text_io::{self, read, };
use webview::*;

const config_filename: &str = "config.dat";

fn get_time() -> i64 {
    time::SystemTime::now().duration_since(time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn main() {
    let mut sniffer = Sniffer::new();
    
    // Create UI in taskbar
    let mut app = Application::new().unwrap();

    //Prompts user for the interface to use
    let mut interfaces = sniffer.get_available_interfaces();
    let mut selected_interface: Arc<Mutex<Option<NetworkInterface>>> = Arc::new(Mutex::new(None));

    // Read interface from config
    let mut last_interface: Option<NetworkInterface> = None;
    let mut match_interface: String = match File::open(config_filename) {
        Ok(mut file) => {
            let mut buf = vec![];
            file.read_to_end(&mut buf).expect("Could not read config file! Try deleting it.");
            String::from_utf8(buf).unwrap().trim().to_string()
        }
        Err(_) => "".to_string()
    };

    for (index, interface) in interfaces.iter().enumerate() {
        let selected_interface = selected_interface.clone();
        let desc = interface.description.to_string();
        let interface_object = interface.clone();

        if match_interface == interface.name {
            *selected_interface.lock().unwrap() = Some(interface.clone());
        }

        app.add_menu_item(&desc, move |_| -> Result<(), systray::Error> {
            let mut selected_interface=  selected_interface.lock().unwrap();
            *selected_interface = Some(interface_object.clone());
            Ok(())
        });
    }

    // Attempt to find callsign
    let mut last_callsign: String = "".to_string();
    let mut current_callsign: String = "".to_string();
    let mut current_atc_position: Option<ATCPosition> = None;
    let mut current_pilot_position: Option<PilotPosition> = None;

    let mut pilot_manager = PilotManager::new();
    let mut tracked_aircraft: HashSet<String> = HashSet::new();

    let mut counter_set: HashMap<String, SystemTime> = HashMap::new();

    // lower position counters
    let mut squawk_counter: usize = 0;
    let mut strip_counter: usize = 0;

    let mut tick = 0;
    let mut start_time: i64 = get_time();

    let mut client: Discord<()> = Discord::with_create_flags(748578379648991273, CreateFlags::Default).expect("Failed to connect to discord!");

    let mut check_aircraft = |target: &String| -> bool {
        let last_dur = counter_set.get(target);
        let now = time::SystemTime::now();
        if last_dur.is_none() || (now.duration_since(*last_dur.unwrap()).unwrap().as_secs() > 60) {
            counter_set.insert(target.to_string(), now);
            return true;
        }
        return false;
    };

    // Handle toolbar inputs
    std::thread::spawn(move || {
        app.wait_for_message().ok();
    });
    // Track handoffs
    loop {
        let selected_interface=  selected_interface.lock().unwrap();
        if last_interface != *selected_interface {
            let new_interface = selected_interface.as_ref().unwrap().clone();
            sniffer.set_user_interface(&new_interface);
            sniffer.start();

            match File::create(config_filename) {
                Ok(mut file) => {
                    file.write(new_interface.name.as_bytes());
                }
                Err(_) => {}
            }

            last_interface = Some(new_interface);
        }

        if last_interface.is_some() {
            match sniffer.next() {
                Some(PacketSource::Client(packet)) => match packet {
                    PacketTypes::ClientQuery(query) => match query.payload {
                        ClientQueryPayload::AcceptHandoff(aircraft, atc) => {tracked_aircraft.insert(aircraft);},
                        ClientQueryPayload::DropTrack(aircraft) => {tracked_aircraft.remove(&aircraft);},
                        ClientQueryPayload::InitiateTrack(aircraft) => {tracked_aircraft.insert(aircraft);},
                        ClientQueryPayload::SetBeaconCode(aircraft, _) => {
                            if check_aircraft(&aircraft) {
                                squawk_counter += 1;
                            }
                        }
                        _ => ()
                    },
                    PacketTypes::ATCPosition(position) => {
                        if position.callsign.find("ATIS").is_none() {
                            current_callsign = position.callsign.to_string();
                            current_atc_position = Some(position);
                        }
                    },
                    PacketTypes::PilotPosition(position) => {
                        current_callsign = position.callsign.to_string();
                        current_pilot_position = Some(position);
                    },
                    PacketTypes::FlightStrip(s) => {
                        if check_aircraft(&s.target) {
                            strip_counter += 1;
                        }
                    }
                    _ => ()
                }
                Some(PacketSource::Server(packet)) => match packet {
                    PacketTypes::PilotPosition(position) => {
                        pilot_manager.process_position(&position);
                    },
                    PacketTypes::DeleteClient(client) =>  {
                        pilot_manager.delete(&client.callsign);
                        tracked_aircraft.remove(&client.callsign);
                    }
                    _ => ()
                }
                _ => {}
            }
        }

        if last_callsign != current_callsign {start_time = get_time(); last_callsign = current_callsign.clone()}

        if tick % 100 == 0 {
            let details: String;
            let large_tooltip: String;
            let small_tooltip: String;

            if let Some(position) = current_atc_position.as_ref() {
                match position.facility {
                    NetworkFacility::OBS | NetworkFacility::DEL | NetworkFacility::GND | NetworkFacility::Undefined => {
                        details = format!("Seeing {} aircraft", pilot_manager.number_tracked());
                        small_tooltip = format!("{} Squawks {} Strips", squawk_counter, strip_counter);
                    },
                    _ => {
                        details = format!("Tracking {}/{} aircraft", tracked_aircraft.len(), pilot_manager.number_tracked());
                        small_tooltip = "".to_string()
                    }
                };
                large_tooltip = format!("{} {}", position.rating.to_string(), position.freq.text.to_string());
            } else {
                details = "Idling".to_string();
                large_tooltip = "".to_string();
                small_tooltip = "".to_string();
            }
                
            client.update_activity(
                &Activity::empty()
                    .with_details(&current_callsign)
                    .with_state(&details)
                    .with_start_time(start_time)
                    .with_large_image_key("radar")
                    .with_large_image_tooltip(&large_tooltip)
                    .with_small_image_key("vatsim1")
                    .with_small_image_tooltip(&small_tooltip),

                |discord: &Discord<()>, result| {
                if let Err(error) = result {
                    return eprintln!("failed to update activity: {}", error);
                }
            });
        }

        client.run_callbacks();
        tick += 1;
        std::thread::sleep(time::Duration::from_millis(50));
    }
}
