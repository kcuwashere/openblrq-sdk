//! Spawns a box for every player who joins and counts how often each player has visited.

use std::cell::RefCell;
use std::collections::HashMap;

use openblrq::server::exports::openblrq::engine::server_events::Guest;
use openblrq::server::openblrq::engine::players::{self, PlayerInfo};
use openblrq::server::openblrq::engine::types::{
    Entity, Field, LogLevel, Player, Quat, Transform, Value, Vec3,
};
use openblrq::server::openblrq::engine::{entities, log};

thread_local! {
    static BOXES: RefCell<HashMap<Player, Entity>> = RefCell::new(HashMap::new());
}

struct Hello;

impl Guest for Hello {
    fn init(_config: String) {
        log::write(LogLevel::Info, "hello-server started");
    }

    fn tick() {}

    fn player_joined(p: PlayerInfo) {
        let spawn_at = Transform {
            translation: Vec3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            rotation: Quat {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
            scale: Vec3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        };
        let Ok(e) = entities::spawn(spawn_at) else {
            log::write(LogLevel::Warn, "couldn't spawn a box");
            return;
        };
        let _ = entities::set_component(
            e,
            "greeted",
            &[Field {
                name: "times".into(),
                value: Value::Integer(1),
            }],
        );
        let _ = players::set_character(p.handle, Some(e));
        BOXES.with_borrow_mut(|boxes| boxes.insert(p.handle, e));
        log::write(LogLevel::Info, &format!("{} joined", p.display_name));
    }

    fn player_left(p: Player) {
        if let Some(e) = BOXES.with_borrow_mut(|boxes| boxes.remove(&p)) {
            let _ = entities::despawn(e);
        }
    }

    fn client_message(_sender: Player, _payload: Vec<u8>) {}

    fn timer_fired(_id: u64) {}

    fn shutdown() {}
}

openblrq::export_server!(Hello);
