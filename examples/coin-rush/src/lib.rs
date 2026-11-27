//! Coin Rush: coins appear around the spawn point, players collect them by walking over them, and
//! whoever has the most when the 90-second round ends wins.
//!
//! Each player gets an entity with a `runner` component (name and score) that the engine moves
//! with their character. Coins are entities with a `coin` component. Clients receive both, so a
//! client component can draw the scoreboard. Scores also go out as plain text messages once a
//! second, which is enough to follow the game without a client component.

use std::cell::RefCell;
use std::collections::BTreeMap;

use openblrq::server::exports::openblrq::engine::server_events::Guest;
use openblrq::server::openblrq::engine::players::{self, PlayerInfo};
use openblrq::server::openblrq::engine::types::{
    Entity, Field, HostError, LogLevel, Player, Quat, Transform, Value, Vec3,
};
use openblrq::server::openblrq::engine::{entities, log, messaging, time, timers};

const COINS: usize = 8;
/// How close a character has to get to pick up a coin, in metres.
const PICKUP_RADIUS: f32 = 1.2;
/// Coins appear within this distance of the spawn point.
const FIELD_RADIUS: f32 = 12.0;
const ROUND_SECONDS: u32 = 90;
const RESPAWN_SECONDS: u32 = 2;

struct Runner {
    entity: Entity,
    name: String,
    score: i64,
}

#[derive(Default)]
struct Game {
    runners: BTreeMap<Player, Runner>,
    coins: Vec<Entity>,
    /// Timer id to the number of coins it brings back.
    respawns: BTreeMap<u64, usize>,
    round_timer: Option<u64>,
    round: u32,
    /// State of the small generator that places coins.
    rng: u64,
}

thread_local! {
    static GAME: RefCell<Game> = RefCell::new(Game::default());
}

fn transform(x: f32, y: f32, z: f32) -> Transform {
    Transform {
        translation: Vec3 { x, y, z },
        rotation: Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        scale: Vec3 {
            x: 0.5,
            y: 0.5,
            z: 0.5,
        },
    }
}

impl Game {
    /// A position on the field. An xorshift generator is plenty for coin placement, and keeps
    /// the game the same from one run to the next.
    fn next_spot(&mut self) -> (f32, f32) {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        let angle = (x & 0xffff) as f32 / 65536.0 * std::f32::consts::TAU;
        let distance = 3.0 + ((x >> 16) & 0xffff) as f32 / 65536.0 * (FIELD_RADIUS - 3.0);
        (angle.cos() * distance, angle.sin() * distance)
    }

    fn spawn_coin(&mut self) {
        let (x, z) = self.next_spot();
        let Ok(coin) = entities::spawn(transform(x, 1.0, z)) else {
            log::write(LogLevel::Warn, "couldn't spawn a coin");
            return;
        };
        let _ = entities::set_component(
            coin,
            "coin",
            &[Field {
                name: "value".into(),
                value: Value::Integer(1),
            }],
        );
        self.coins.push(coin);
    }

    fn start_round(&mut self) {
        self.round += 1;
        for runner in self.runners.values_mut() {
            runner.score = 0;
            set_runner(runner);
        }
        self.round_timer = timers::schedule(ROUND_SECONDS * time::tick_rate()).ok();
        log::write(LogLevel::Info, &format!("round {} started", self.round));
        let _ = messaging::broadcast(format!("Round {} has started.", self.round).as_bytes());
    }

    fn end_round(&mut self) {
        let best = self
            .runners
            .values()
            .max_by_key(|r| r.score)
            .filter(|r| r.score > 0);
        let text = match best {
            Some(r) => format!(
                "Round {} is over. {} wins with {}.",
                self.round, r.name, r.score
            ),
            None => format!("Round {} is over. Nobody picked up a coin.", self.round),
        };
        log::write(LogLevel::Info, &text);
        let _ = messaging::broadcast(text.as_bytes());
        self.start_round();
    }

    fn collect(&mut self) {
        let mut picked = 0;
        for runner in self.runners.values_mut() {
            let Ok(at) = entities::get_transform(runner.entity) else {
                continue;
            };
            let mut i = 0;
            while i < self.coins.len() {
                let coin = self.coins[i];
                let near = entities::get_transform(coin).is_ok_and(|c| {
                    let dx = c.translation.x - at.translation.x;
                    let dz = c.translation.z - at.translation.z;
                    dx * dx + dz * dz < PICKUP_RADIUS * PICKUP_RADIUS
                });
                if near {
                    let _ = entities::despawn(coin);
                    self.coins.swap_remove(i);
                    runner.score += 1;
                    set_runner(runner);
                    picked += 1;
                } else {
                    i += 1;
                }
            }
        }
        if picked > 0
            && let Ok(id) = timers::schedule(RESPAWN_SECONDS * time::tick_rate())
        {
            self.respawns.insert(id, picked);
        }
    }

    fn scoreboard(&self) -> String {
        let mut runners: Vec<&Runner> = self.runners.values().collect();
        runners.sort_by(|a, b| b.score.cmp(&a.score).then(a.name.cmp(&b.name)));
        let parts: Vec<String> = runners
            .iter()
            .map(|r| format!("{} {}", r.name, r.score))
            .collect();
        format!("Scores: {}", parts.join(", "))
    }
}

fn set_runner(runner: &Runner) {
    let _ = entities::set_component(
        runner.entity,
        "runner",
        &[
            Field {
                name: "name".into(),
                value: Value::Text(runner.name.clone()),
            },
            Field {
                name: "score".into(),
                value: Value::Integer(runner.score),
            },
        ],
    );
}

struct CoinRush;

impl Guest for CoinRush {
    fn init(_config: String) {
        GAME.with_borrow_mut(|g| {
            g.rng = 0x9e37_79b9_7f4a_7c15;
            for _ in 0..COINS {
                g.spawn_coin();
            }
            g.start_round();
        });
    }

    fn tick() {
        GAME.with_borrow_mut(|g| {
            g.collect();
            if time::tick() % u64::from(time::tick_rate()) == 0 && !g.runners.is_empty() {
                let _ = messaging::broadcast(g.scoreboard().as_bytes());
            }
        });
    }

    fn player_joined(p: PlayerInfo) {
        GAME.with_borrow_mut(|g| {
            let offset = g.runners.len() as f32 * 1.5;
            let Ok(entity) = entities::spawn(transform(offset, 2.0, 0.0)) else {
                let _ = players::kick(p.handle, "The game is full.");
                return;
            };
            let runner = Runner {
                entity,
                name: p.display_name.clone(),
                score: 0,
            };
            set_runner(&runner);
            let _ = players::set_character(p.handle, Some(entity));
            g.runners.insert(p.handle, runner);
            let _ = messaging::send(
                p.handle,
                format!(
                    "Welcome, {}. Walk over coins to pick them up.",
                    p.display_name
                )
                .as_bytes(),
            );
        });
    }

    fn player_left(p: Player) {
        GAME.with_borrow_mut(|g| {
            if let Some(runner) = g.runners.remove(&p) {
                let _ = entities::despawn(runner.entity);
            }
        });
    }

    fn client_message(_sender: Player, _payload: Vec<u8>) {}

    fn timer_fired(id: u64) {
        GAME.with_borrow_mut(|g| {
            if g.round_timer == Some(id) {
                g.end_round();
            } else if let Some(n) = g.respawns.remove(&id) {
                for _ in 0..n {
                    g.spawn_coin();
                }
            }
        });
    }

    fn storage_updated(_request: u64, _outcome: Result<u64, HostError>) {}

    fn shutdown() {}
}

openblrq::export_server!(CoinRush);
