use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
use openblrq_manifest::Manifest;
use openblrq_manifest::bundle::{self, MANIFEST_FILE, SERVER_FILE, SIGNATURE_FILE};

#[derive(Parser)]
#[command(
    name = "obq",
    version,
    about = "Build and publish OpenBLRQ experiences"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check openblrq.toml in the given directory.
    Check {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Print the BLAKE3 hash of a file, in the form the manifest uses.
    Hash { file: PathBuf },
    /// Put a bundle together: copy the components and assets into OUT and write the manifest
    /// with every file's hash and size.
    Build {
        /// The project directory with openblrq.toml.
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// The server component.
        #[arg(long)]
        server: PathBuf,
        /// A client component, as NAME=PATH. It goes in client/NAME.wasm.
        #[arg(long = "client", value_name = "NAME=PATH")]
        clients: Vec<String>,
        /// A directory of assets, copied to assets/.
        #[arg(long)]
        assets: Option<PathBuf>,
        /// Where to write the bundle. Must not exist yet, or be empty.
        #[arg(long)]
        out: PathBuf,
    },
    /// Manage your creator key.
    Keys {
        #[command(subcommand)]
        command: KeysCommand,
    },
    /// Sign a built bundle with your creator key. Uploading for review isn't open yet; the
    /// signed bundle stays where it is.
    Publish {
        #[arg(default_value = ".")]
        bundle: PathBuf,
        /// Your creator key. Defaults to the one `obq keys new` made.
        #[arg(long)]
        key: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum KeysCommand {
    /// Make a new creator key and print its public half, which you register with your account.
    New {
        /// Where to keep it. Defaults to ~/.config/openblrq/creator.key.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Print the public half of a creator key.
    Show {
        #[arg(long)]
        key: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Check { dir } => check(&dir),
        Command::Hash { file } => hash_file(&file).map(|(hash, _)| println!("{hash}")),
        Command::Build {
            dir,
            server,
            clients,
            assets,
            out,
        } => build(&dir, &server, &clients, assets.as_deref(), &out),
        Command::Keys {
            command: KeysCommand::New { out },
        } => keys_new(out),
        Command::Keys {
            command: KeysCommand::Show { key },
        } => load_key(key).map(|k| println!("{}", public_key(&k))),
        Command::Publish { bundle, key } => publish(&bundle, key),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

type Result<T> = std::result::Result<T, String>;

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))
}

fn check(dir: &Path) -> Result<()> {
    let path = dir.join(MANIFEST_FILE);
    let m = Manifest::parse(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{} {} looks fine", m.experience.id, m.experience.version);
    Ok(())
}

/// The manifest form of a file's hash, and its size. Reads in chunks, so large assets don't
/// have to fit in memory.
fn hash_file(path: &Path) -> Result<(String, u64)> {
    let mut file =
        fs::File::open(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("can't read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((format!("blake3:{}", hasher.finalize().to_hex()), size))
}

fn copy_into(
    from: &Path,
    out: &Path,
    rel: &str,
    files: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    bundle::file_kind(rel).map_err(|e| e.to_string())?;
    let to = out.join(rel);
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::copy(from, &to).map_err(|e| format!("can't copy {}: {e}", from.display()))?;
    files.insert(rel.to_owned(), to);
    Ok(())
}

fn copy_assets(
    root: &Path,
    dir: &Path,
    out: &Path,
    files: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("can't read {}: {e}", dir.display()))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| format!("can't read {}: {e}", dir.display()))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            copy_assets(root, &path, out, files)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|_| format!("{} is outside the assets directory", path.display()))?
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        copy_into(&path, out, &format!("assets/{rel}"), files)?;
    }
    Ok(())
}

fn build(
    dir: &Path,
    server: &Path,
    clients: &[String],
    assets: Option<&Path>,
    out: &Path,
) -> Result<()> {
    let source_path = dir.join(MANIFEST_FILE);
    let source = read(&source_path)?;
    let parsed = Manifest::parse(&source).map_err(|e| format!("{}: {e}", source_path.display()))?;
    if !parsed.files.is_empty() {
        return Err(format!(
            "{} already has a [files] table. Remove it; obq build writes it.",
            source_path.display()
        ));
    }
    if out.exists()
        && fs::read_dir(out)
            .map_err(|e| format!("{}: {e}", out.display()))?
            .next()
            .is_some()
    {
        return Err(format!("{} isn't empty", out.display()));
    }
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;

    let mut files = BTreeMap::new();
    copy_into(server, out, SERVER_FILE, &mut files)?;
    for c in clients {
        let (name, path) = c
            .split_once('=')
            .ok_or_else(|| format!("--client takes NAME=PATH, not {c}"))?;
        copy_into(
            Path::new(path),
            out,
            &format!("client/{name}.wasm"),
            &mut files,
        )?;
    }
    if let Some(assets) = assets {
        copy_assets(assets, assets, out, &mut files)?;
    }

    let mut manifest = source.trim_end().to_owned();
    manifest.push_str("\n\n# Written by obq build. Don't edit by hand.\n");
    for (rel, path) in &files {
        let (hash, size) = hash_file(path)?;
        manifest.push_str(&format!(
            "\n[files.\"{rel}\"]\nhash = \"{hash}\"\nsize = {size}\n"
        ));
    }
    let built = Manifest::parse(&manifest).map_err(|e| e.to_string())?;
    built.check_built().map_err(|e| e.to_string())?;
    fs::write(out.join(MANIFEST_FILE), &manifest).map_err(|e| format!("{}: {e}", out.display()))?;
    let digest = bundle::digest(&manifest).map_err(|e| e.to_string())?;
    println!(
        "Built {} {} in {} ({} {})\n{digest}",
        built.experience.id,
        built.experience.version,
        out.display(),
        files.len(),
        if files.len() == 1 { "file" } else { "files" }
    );
    Ok(())
}

fn default_key_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or("can't find your home directory; pass --out or --key")?;
    Ok(PathBuf::from(home).join(".config/openblrq/creator.key"))
}

fn public_key(key: &SigningKey) -> String {
    format!("ed25519:{}", hex(key.verifying_key().as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn keys_new(out: Option<PathBuf>) -> Result<()> {
    let path = match out {
        Some(p) => p,
        None => default_key_path()?,
    };
    if path.exists() {
        return Err(format!(
            "{} already exists. Keep using it, or move it away first.",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("no randomness available: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    write_private(&path, &format!("{}\n", hex(&seed)))?;
    println!(
        "Your creator key is in {}. Keep it private and backed up.\nRegister this public key with your account:\n{}",
        path.display(),
        public_key(&key)
    );
    Ok(())
}

fn write_private(path: &Path, contents: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))
    }
    #[cfg(not(unix))]
    fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

fn load_key(path: Option<PathBuf>) -> Result<SigningKey> {
    let path = match path {
        Some(p) => p,
        None => default_key_path()?,
    };
    let text = read(&path)?;
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{} isn't a creator key", path.display()));
    }
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| format!("{} isn't a creator key", path.display()))?;
    }
    Ok(SigningKey::from_bytes(&seed))
}

fn publish(dir: &Path, key: Option<PathBuf>) -> Result<()> {
    let text = read(&dir.join(MANIFEST_FILE))?;
    let manifest = Manifest::parse(&text).map_err(|e| e.to_string())?;
    manifest.check_built().map_err(|e| e.to_string())?;
    for (rel, entry) in &manifest.files {
        let (hash, size) = hash_file(&dir.join(rel))?;
        if hash != entry.hash || size != entry.size {
            return Err(format!(
                "{rel} changed since obq build. Build the bundle again."
            ));
        }
    }
    let key = load_key(key)?;
    let digest = bundle::digest(&text).map_err(|e| e.to_string())?;
    let signature = key.sign(&bundle::signing_message(&digest));
    let sig = serde_json::json!({
        "format": 1,
        "digest": digest,
        "key": public_key(&key),
        "signature": hex(&signature.to_bytes()),
    });
    fs::write(
        dir.join(SIGNATURE_FILE),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&sig).map_err(|e| e.to_string())?
        ),
    )
    .map_err(|e| format!("{}: {e}", dir.display()))?;
    println!(
        "Signed {} {} ({digest}).\nUploading for review isn't open yet. The signed bundle is in {}.",
        manifest.experience.id,
        manifest.experience.version,
        dir.display()
    );
    Ok(())
}
