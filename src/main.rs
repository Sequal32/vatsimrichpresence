mod tracker;

use discord_game_sdk::{Discord, Activity, CreateFlags};
use fsdparser::{Sniffer, PacketSource, PacketTypes, ClientQueryPayload, NetworkFacility};
use pnet::datalink::NetworkInterface;
use tracker::Tracker;
use std::sync::Arc;
use std::{io::{Write, Read}, sync::Mutex};
use std::fs::File;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use systray::Application;

const CONFIG_FILENAME: &str = "config.dat";

fn get_time() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn main() {
    let mut sniffer = Sniffer::new();
    
    // Create UI in taskbar
    let mut app = Application::new().unwrap();

    //Prompts user for the interface to use
    let interfaces = sniffer.get_available_interfaces();
    let selected_interface: Arc<Mutex<Option<NetworkInterface>>> = Arc::new(Mutex::new(None));

    // Read interface from config
    let mut last_interface: Option<NetworkInterface> = None;
    let match_interface: String = match File::open(CONFIG_FILENAME) {
        Ok(mut file) => {
            let mut buf = vec![];
            file.read_to_end(&mut buf).expect("Could not read config file! Try deleting it.");
            String::from_utf8(buf).unwrap().trim().to_string()
        }
        Err(_) => "".to_string()
    };
    for (_, interface) in interfaces.iter().enumerate() {
        let selected_interface = selected_interface.clone();
        let desc = interface.description.to_string();
        let interface_object = interface.clone();
        // Match interface with config with available interfaces
        if match_interface == interface.name {
            *selected_interface.lock().unwrap() = Some(interface.clone());
        }
        // Append interface for selection in toolbar
        app.add_menu_item(&desc, move |_| -> Result<(), systray::Error> {
            let mut selected_interface=  selected_interface.lock().unwrap();
            *selected_interface = Some(interface_object.clone());
            Ok(())
        }).ok();
    }
    // Handle toolbar inputs
    std::thread::spawn(move || {
        app.wait_for_message().ok();
    });
    
    // Variables for rich presence timing
    let mut tick = 0;
    let mut start_time: i64 = get_time();

    let mut client: Discord<()> = Discord::with_create_flags(748578379648991273, CreateFlags::Default).expect("Failed to connect to discord!");
    let mut tracker = Tracker::new();

    loop {
        let selected_interface=  selected_interface.lock().unwrap();
        // On interface selected
        if last_interface != *selected_interface {
            let new_interface = selected_interface.as_ref().unwrap().clone();
            sniffer.set_user_interface(&new_interface);
            sniffer.start();

            match File::create(CONFIG_FILENAME) {
                Ok(mut file) => {
                    file.write(new_interface.name.as_bytes()).ok();
                }
                Err(_) => {}
            }

            last_interface = Some(new_interface);
        }

        if last_interface.is_some() {
            match sniffer.next() {
                Some(PacketSource::Client(packet)) => match packet {
                    PacketTypes::ClientQuery(query) => match query.payload {
                        ClientQueryPayload::AcceptHandoff(aircraft, _) => tracker.handoff(&aircraft),
                        ClientQueryPayload::InitiateTrack(aircraft) => tracker.tracked(&aircraft),
                        ClientQueryPayload::DropTrack(aircraft) => tracker.drop_tracked(&aircraft),
                        ClientQueryPayload::SetBeaconCode(aircraft, _) => tracker.assigned_squawk(&aircraft),
                        _ => ()
                    },
                    PacketTypes::ATCPosition(position) => {
                        // Do not capture vATIS traffic
                        if position.callsign.find("ATIS").is_none() {
                            // Update and detect if wasn't connected before
                            if tracker.update_atc_position(position) {
                                // Just started controlling
                                start_time = get_time();
                            }
                        }
                    },
                    PacketTypes::PilotPosition(position) => {tracker.update_pilot_position(position);},
                    PacketTypes::FlightStrip(s) => tracker.pushed_strip(&s.target),
                    _ => ()
                }
                Some(PacketSource::Server(packet)) => match packet {
                    PacketTypes::PilotPosition(position) => tracker.add_pilot(&position),
                    PacketTypes::DeleteClient(client) => tracker.remove_pilot(&client.callsign),
                    _ => ()
                }
                _ => {}
            }
        }

        if tick % 100 == 0 {
            let details: String;
            let large_tooltip: String;
            let small_tooltip: String;
            let callsign: String;

            if tracker.get_secs_since_last_callsign() > 60 {
                tracker.reset();
            }

            if let Some(position) = tracker.get_atc_position() {
                match position.facility {
                    NetworkFacility::OBS | NetworkFacility::DEL | NetworkFacility::GND | NetworkFacility::Undefined => {
                        details = format!("Seeing {} aircraft", tracker.get_number_seen());
                        small_tooltip = format!("{} Squawks {} Strips", tracker.get_number_squawks(), tracker.get_number_strips());
                    },
                    _ => {
                        details = format!("Tracking {}/{} aircraft", tracker.get_number_tracked(), tracker.get_number_seen());
                        small_tooltip = format!("{} Handoffs", tracker.get_number_handoffs());
                    }
                };
                callsign = position.callsign.to_string();
                large_tooltip = format!("{} {}", position.rating.to_string(), position.freq.text.to_string());
            } else {
                details = "Idling".to_string();
                large_tooltip = "".to_string();
                small_tooltip = "".to_string();
                callsign = "".to_string()
            }
                
            client.update_activity(
                &Activity::empty()
                    .with_details(&callsign)
                    .with_state(&details)
                    .with_start_time(start_time)
                    .with_large_image_key("radar")
                    .with_large_image_tooltip(&large_tooltip)
                    .with_small_image_key("vatsim1")
                    .with_small_image_tooltip(&small_tooltip),

                |_: &Discord<()>, result| {
                if let Err(error) = result {
                    return eprintln!("failed to update activity: {}", error);
                }
            });
        }

        client.run_callbacks().ok();
        tick += 1;
        std::thread::sleep(Duration::from_millis(50));
    }
}
