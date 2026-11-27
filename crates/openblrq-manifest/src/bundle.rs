//! Bundles: what files may be in one, and the digest that identifies it.
//!
//! A bundle is a directory with `openblrq.toml`, `server.wasm`, optional client components in
//! `client/`, and assets in `assets/`. The manifest's `[files]` table lists every other file
//! with its hash and size. The bundle digest is the BLAKE3 hash of the manifest in canonical
//! JSON (RFC 8785), so it covers every file through their hashes. A creator signs the digest to
//! show who uploaded the bundle; that signature doesn't make anything playable.

use crate::{Error, invalid};

pub const MANIFEST_FILE: &str = "openblrq.toml";
pub const SIGNATURE_FILE: &str = "openblrq.sig";
pub const SERVER_FILE: &str = "server.wasm";

pub const MAX_SERVER_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_CLIENT_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_BUNDLE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_FILES: usize = 10_000;

/// Asset types the platform accepts, by extension.
pub const ASSET_EXTENSIONS: &[&str] = &["glb", "png", "ktx2", "ogg", "opus", "otf", "ttf"];

/// What a file in a bundle is, from its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Server,
    Client,
    Asset,
}

impl FileKind {
    pub fn max_bytes(self) -> u64 {
        match self {
            FileKind::Server => MAX_SERVER_BYTES,
            FileKind::Client => MAX_CLIENT_BYTES,
            FileKind::Asset => MAX_ASSET_BYTES,
        }
    }
}

fn plain_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// The kind of a bundle path, or an error saying why it isn't allowed. Paths use `/`, are
/// relative, and only contain letters, digits, `-`, `_` and `.`.
pub fn file_kind(path: &str) -> Result<FileKind, Error> {
    let field = || format!("files.{path}");
    let parts: Vec<&str> = path.split('/').collect();
    if !parts.iter().all(|p| plain_segment(p)) {
        return Err(invalid(
            field(),
            "must be a relative path of letters, digits, '-', '_' and '.'",
        ));
    }
    match parts.as_slice() {
        [SERVER_FILE] => Ok(FileKind::Server),
        ["client", name] if name.ends_with(".wasm") => Ok(FileKind::Client),
        ["assets", .., last] if parts.len() >= 2 => {
            let ext = last.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
            if ASSET_EXTENSIONS.contains(&ext) {
                Ok(FileKind::Asset)
            } else {
                Err(invalid(
                    field(),
                    format!("assets must be one of: {}", ASSET_EXTENSIONS.join(", ")),
                ))
            }
        }
        _ => Err(invalid(
            field(),
            "only server.wasm, client/*.wasm and assets/** belong in a bundle",
        )),
    }
}

/// The bundle digest: `blake3:` and the hash of the manifest text in canonical JSON. Computed
/// from the TOML as written, not from the parsed structure, so the same file always gives the
/// same digest whichever version of this crate reads it.
pub fn digest(manifest_text: &str) -> Result<String, Error> {
    let value: toml::Value = toml::from_str(manifest_text)?;
    let mut out = String::new();
    canonical(&value, &mut out)?;
    Ok(format!("blake3:{}", blake3::hash(out.as_bytes()).to_hex()))
}

/// The bytes a creator key signs for a bundle. The prefix keeps a bundle signature from being
/// valid for anything else.
pub fn signing_message(digest: &str) -> Vec<u8> {
    format!("openblrq bundle v1\n{digest}").into_bytes()
}

/// RFC 8785 for the values a manifest can hold. Floats and dates aren't part of the manifest
/// format, so they're refused rather than formatted.
fn canonical(value: &toml::Value, out: &mut String) -> Result<(), Error> {
    match value {
        toml::Value::String(s) => quote(s, out),
        toml::Value::Integer(i) => {
            // JSON numbers are doubles; larger integers wouldn't survive a round trip.
            if i.unsigned_abs() > (1 << 53) {
                return Err(invalid("manifest", "integers must fit in 53 bits"));
            }
            out.push_str(&i.to_string());
        }
        toml::Value::Boolean(b) => out.push_str(if *b { "true" } else { "false" }),
        toml::Value::Float(_) => {
            return Err(invalid(
                "manifest",
                "decimal numbers aren't used in manifests",
            ));
        }
        toml::Value::Datetime(_) => {
            return Err(invalid("manifest", "dates aren't used in manifests"));
        }
        toml::Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out)?;
            }
            out.push(']');
        }
        toml::Value::Table(table) => {
            // RFC 8785 orders keys by their UTF-16 code units.
            let mut keys: Vec<&String> = table.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                quote(key, out);
                out.push(':');
                canonical(&table[key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_ignores_formatting_but_not_content() {
        let a = "[experience]\nid = \"x\"\nname = \"X\"\n\n[players]\nmax = 4\n";
        let b = "# a comment\n[players]\nmax=4\n[experience]\nname=\"X\"\nid=\"x\"\n";
        let c = "[experience]\nid = \"x\"\nname = \"Y\"\n\n[players]\nmax = 4\n";
        assert_eq!(digest(a).unwrap(), digest(b).unwrap());
        assert_ne!(digest(a).unwrap(), digest(c).unwrap());
    }

    #[test]
    fn canonical_json_matches_rfc_8785() {
        let v: toml::Value = toml::from_str(
            "z = 1\na = \"tab\\there \\\"q\\\" é\"\n\"\u{e9}\" = true\nlist = [3, -2]\n[t]\nk = \"\\u0001\"\n",
        )
        .unwrap();
        let mut out = String::new();
        canonical(&v, &mut out).unwrap();
        assert_eq!(
            out,
            "{\"a\":\"tab\\there \\\"q\\\" é\",\"list\":[3,-2],\"t\":{\"k\":\"\\u0001\"},\"z\":1,\"é\":true}"
        );
    }

    #[test]
    fn only_known_paths_belong_in_a_bundle() {
        assert_eq!(file_kind("server.wasm").unwrap(), FileKind::Server);
        assert_eq!(file_kind("client/hud.wasm").unwrap(), FileKind::Client);
        assert_eq!(
            file_kind("assets/models/crate.glb").unwrap(),
            FileKind::Asset
        );
        for bad in [
            "../server.wasm",
            "/server.wasm",
            "assets/../server.wasm",
            "notes.txt",
            "assets/run.exe",
            "client/sub/x.wasm",
            "assets/a b.png",
            "assets//x.png",
        ] {
            assert!(file_kind(bad).is_err(), "{bad}");
        }
    }
}
