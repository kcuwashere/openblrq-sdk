//! Parsing and basic validation of `openblrq.toml`.
//!
//! This crate checks that a manifest is well formed and within the published limits. The
//! platform runs the same checks on upload, plus others that need the rest of the bundle.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod bundle;

/// Interface versions this crate knows about.
pub const SUPPORTED_INTERFACES: &[&str] = &["0.1.0"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub experience: Experience,
    pub interface: Interface,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// What the client components need on the player's device. Server budgets aren't here: they're
    /// a setting in the creator dashboard.
    #[serde(default)]
    pub client: Option<ClientRequirements>,
    #[serde(default)]
    pub components: BTreeMap<String, ComponentSchema>,
    #[serde(default)]
    pub input: Input,
    pub players: Players,
    pub content: ContentAnswers,
    /// Every file in the bundle, by path, with its hash. Filled in by `obq build`.
    #[serde(default)]
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experience {
    pub id: String,
    pub version: semver::Version,
    pub name: String,
    pub dimension: Dimension,
    #[serde(default = "default_tick_rate")]
    pub tick_rate: u32,
}

fn default_tick_rate() -> u32 {
    60
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dimension {
    #[serde(rename = "2d")]
    TwoD,
    #[serde(rename = "3d")]
    ThreeD,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub server: String,
    pub client: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Physics,
    Storage,
}

/// Recommended budgets for the experience's client components. Players whose settings allow less
/// are told before they join.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientRequirements {
    /// Memory per client component, in MiB.
    pub memory_mib: u32,
    /// Work per frame, in the same units as the platform's CPU budget (instructions, roughly).
    pub fuel_per_frame: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentSchema {
    pub fields: BTreeMap<String, FieldType>,
    /// Whether clients receive this component. Defaults to true.
    #[serde(default = "yes")]
    pub replicated: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldType {
    Boolean,
    Integer,
    Number,
    Point2,
    Point3,
    Rotation,
    Tint,
    Text,
    Blob,
    Target,
    AssetRef,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    #[serde(default)]
    pub actions: BTreeMap<String, Action>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub kind: ActionKind,
    /// Suggested default bindings, such as "key:Space" or "pad:South". Players can rebind.
    #[serde(default)]
    pub default: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    Button,
    Axis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Players {
    pub max: u32,
}

/// The creator's answers to the content questionnaire. Review checks them against the
/// experience and sets the age rating from them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentAnswers {
    pub violence: Level,
    pub fear: Level,
    pub language: Level,
    /// Whether players can type text that other players see.
    pub player_text: bool,
    /// Whether players can build or upload things other players see.
    pub player_creations: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    None,
    Mild,
    Moderate,
    Strong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    /// "blake3:" followed by 64 lowercase hex digits.
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("openblrq.toml is not valid: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("{field}: {problem}")]
    Invalid { field: String, problem: String },
}

fn invalid(field: impl Into<String>, problem: impl Into<String>) -> Error {
    Error::Invalid {
        field: field.into(),
        problem: problem.into(),
    }
}

pub const MAX_PLAYERS: u32 = 100;
pub const MAX_NAME_LEN: usize = 50;
pub const MAX_COMPONENTS: usize = 256;
pub const MAX_FIELDS_PER_COMPONENT: usize = 32;
pub const MAX_ACTIONS: usize = 64;
/// Client ceilings: a recommendation can't ask for more than any player could allow.
pub const MAX_CLIENT_MEMORY_MIB: u32 = 128;
pub const MAX_CLIENT_FUEL_PER_FRAME: u64 = 80_000_000;

impl Manifest {
    /// Checks that a built bundle's manifest lists a server component. A source manifest,
    /// before `obq build`, has no `[files]` yet.
    pub fn check_built(&self) -> Result<(), Error> {
        if !self.files.contains_key(bundle::SERVER_FILE) {
            return Err(invalid("files", "a bundle needs server.wasm"));
        }
        Ok(())
    }

    pub fn parse(text: &str) -> Result<Self, Error> {
        let manifest: Manifest = toml::from_str(text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Checks that don't need the rest of the bundle.
    pub fn validate(&self) -> Result<(), Error> {
        let e = &self.experience;
        if !is_slug(&e.id) {
            return Err(invalid(
                "experience.id",
                "use 3 to 40 lowercase letters, digits and hyphens, starting with a letter",
            ));
        }
        let name_len = e.name.chars().count();
        if name_len == 0 || name_len > MAX_NAME_LEN {
            return Err(invalid(
                "experience.name",
                format!("must be 1 to {MAX_NAME_LEN} characters"),
            ));
        }
        if !(20..=60).contains(&e.tick_rate) {
            return Err(invalid("experience.tick_rate", "must be between 20 and 60"));
        }
        if !SUPPORTED_INTERFACES.contains(&self.interface.server.as_str()) {
            return Err(invalid(
                "interface.server",
                format!("{} is not supported", self.interface.server),
            ));
        }
        if let Some(client) = &self.interface.client
            && !SUPPORTED_INTERFACES.contains(&client.as_str())
        {
            return Err(invalid(
                "interface.client",
                format!("{client} is not supported"),
            ));
        }
        if self.players.max == 0 || self.players.max > MAX_PLAYERS {
            return Err(invalid(
                "players.max",
                format!("must be 1 to {MAX_PLAYERS}"),
            ));
        }
        if self.components.len() > MAX_COMPONENTS {
            return Err(invalid(
                "components",
                format!("at most {MAX_COMPONENTS} components"),
            ));
        }
        for (name, schema) in &self.components {
            if !is_slug(name) {
                return Err(invalid(
                    format!("components.{name}"),
                    "name must be a lowercase slug",
                ));
            }
            if schema.fields.is_empty() || schema.fields.len() > MAX_FIELDS_PER_COMPONENT {
                return Err(invalid(
                    format!("components.{name}.fields"),
                    format!("must have 1 to {MAX_FIELDS_PER_COMPONENT} fields"),
                ));
            }
        }
        if self.input.actions.len() > MAX_ACTIONS {
            return Err(invalid(
                "input.actions",
                format!("at most {MAX_ACTIONS} actions"),
            ));
        }
        if let Some(client) = &self.client {
            if client.memory_mib == 0 || client.memory_mib > MAX_CLIENT_MEMORY_MIB {
                return Err(invalid(
                    "client.memory_mib",
                    format!("must be 1 to {MAX_CLIENT_MEMORY_MIB}"),
                ));
            }
            if client.fuel_per_frame == 0 || client.fuel_per_frame > MAX_CLIENT_FUEL_PER_FRAME {
                return Err(invalid(
                    "client.fuel_per_frame",
                    format!("must be 1 to {MAX_CLIENT_FUEL_PER_FRAME}"),
                ));
            }
        }
        if self.files.len() > bundle::MAX_FILES {
            return Err(invalid(
                "files",
                format!("at most {} files", bundle::MAX_FILES),
            ));
        }
        let mut total = 0u64;
        for (path, entry) in &self.files {
            let kind = bundle::file_kind(path)?;
            if !is_blake3(&entry.hash) {
                return Err(invalid(
                    format!("files.{path}.hash"),
                    "expected blake3:<64 hex digits>",
                ));
            }
            if entry.size > kind.max_bytes() {
                return Err(invalid(
                    format!("files.{path}.size"),
                    format!("at most {} bytes for this kind of file", kind.max_bytes()),
                ));
            }
            total = total.saturating_add(entry.size);
        }
        if total > bundle::MAX_BUNDLE_BYTES {
            return Err(invalid(
                "files",
                format!("the bundle is at most {} bytes", bundle::MAX_BUNDLE_BYTES),
            ));
        }
        Ok(())
    }
}

fn is_slug(s: &str) -> bool {
    let len = s.len();
    (3..=40).contains(&len)
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.ends_with('-')
}

fn is_blake3(s: &str) -> bool {
    s.strip_prefix("blake3:").is_some_and(|hex| {
        hex.len() == 64 && hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../examples/hello-server/openblrq.toml");

    #[test]
    fn example_manifest_is_valid() {
        let m = Manifest::parse(EXAMPLE).unwrap();
        assert_eq!(m.experience.id, "hello-server");
        assert_eq!(m.experience.dimension, Dimension::ThreeD);
    }

    #[test]
    fn rejects_path_escape() {
        let text = format!(
            "{EXAMPLE}\n[files.\"../secret\"]\nhash = \"blake3:{}\"\nsize = 1\n",
            "0".repeat(64)
        );
        assert!(Manifest::parse(&text).is_err());
    }

    #[test]
    fn client_recommendations_stay_under_the_ceilings() {
        let ok = format!("{EXAMPLE}\n[client]\nmemory_mib = 48\nfuel_per_frame = 30000000\n");
        let m = Manifest::parse(&ok).unwrap();
        assert_eq!(m.client.unwrap().memory_mib, 48);
        let too_much = format!("{EXAMPLE}\n[client]\nmemory_mib = 512\nfuel_per_frame = 1\n");
        assert!(Manifest::parse(&too_much).is_err());
        // Server budgets are a dashboard setting, so the old section is refused.
        let old = format!("{EXAMPLE}\n[limits.server_memory]\nvalue = 1\nreason = \"x\"\n");
        assert!(Manifest::parse(&old).is_err());
    }

    #[test]
    fn rejects_bad_slug() {
        assert!(!is_slug("Hello"));
        assert!(!is_slug("a-"));
        assert!(is_slug("tower-run-2"));
    }
}
