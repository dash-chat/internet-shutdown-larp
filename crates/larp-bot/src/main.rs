use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use larp_bot::{cast, config::BotConfig, identity::IdentityBundle, qr};

#[derive(Parser)]
#[command(
    name = "larp-bot",
    about = "Dash Chat character bot for the town-fire LARP"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a character's flashable identity bundle (run offline, once).
    Keygen {
        /// Character key, e.g. "mum".
        #[arg(long)]
        character: String,
        /// Where to write the bundle (default: ./larp-identity.toml).
        #[arg(long, default_value = "larp-identity.toml")]
        out: PathBuf,
        /// Name shown on the scanner's pending chat until the bot's real
        /// profile syncs. Max 16 bytes; defaults to the character key.
        #[arg(long)]
        profile_name: Option<String>,
    },
    /// Render a character's contact QR (for the wall posters) from its bundle.
    Qr {
        #[arg(long)]
        identity: PathBuf,
        /// Output PNG path (default: ./qr.png).
        #[arg(long, default_value = "qr.png")]
        out: PathBuf,
        /// Pixels per QR module.
        #[arg(long, default_value_t = 16)]
        module_px: u32,
        /// Also print the raw QR string (and verify it round-trips).
        #[arg(long)]
        print_string: bool,
    },
    /// Assemble the public cast.toml from one or more identity bundles.
    Cast {
        /// Identity bundle paths, one per character.
        #[arg(long, required = true, num_args = 1..)]
        identity: Vec<PathBuf>,
        #[arg(long, default_value = "cast.toml")]
        out: PathBuf,
    },
    /// Run the bot daemon.
    Run {
        #[arg(long, default_value = "/etc/larp-bot/config.toml")]
        config: PathBuf,
    },
    /// Run a spec-bot daemon: a scripted character with no scenario pack —
    /// the mayor (onboarding + the endgame trigger) or the anonymous
    /// informant. `anonymous` is kept as an alias for the old invocation.
    #[command(alias = "anonymous")]
    Spec {
        #[arg(long, default_value = "/etc/larp-bot/spec.toml")]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    match Cli::parse().command {
        Command::Keygen {
            character,
            out,
            profile_name,
        } => {
            anyhow::ensure!(
                !out.exists(),
                "{} already exists — refusing to overwrite an identity",
                out.display()
            );
            let mut bundle = IdentityBundle::generate(&character);
            bundle.profile_name = profile_name;
            bundle.save(&out)?;
            println!("wrote {}", out.display());
            println!("\n# cast.toml entry (public — safe to commit):");
            let entry = bundle.cast_entry()?;
            println!("[characters.{character}]");
            print!("{}", toml::to_string_pretty(&entry)?);
        }
        Command::Qr {
            identity,
            out,
            module_px,
            print_string,
        } => {
            let bundle = IdentityBundle::load(&identity)?;
            let code = bundle.contact_code()?;
            // Always verify against dashchat-node's own parser before the code
            // lands on paper: decode it, then re-encode through the upstream
            // `Display` and compare. Catches any wire-format drift.
            let decoded = qr::decode_contact_code(&code).context("QR round-trip check failed")?;
            anyhow::ensure!(
                decoded.device_pubkey == bundle.device_id()? && decoded.to_string() == code,
                "QR round-trip mismatch — the contact-code format has drifted"
            );
            // The QR carries the add-contact deep link, not the bare code:
            // since dash-chat 08dc85a3 the scan path only accepts the
            // https://dashchat.org/add-contact/{code} form (a bare code gets
            // the "invalid contact link" toast).
            let link = qr::contact_deep_link(&code);
            qr::render_png(&link, &out, module_px)?;
            println!("wrote {} ({})", out.display(), bundle.character);
            if print_string {
                println!("{link}");
            }
        }
        Command::Cast { identity, out } => {
            let mut cast = cast::Cast::default();
            for path in identity {
                let bundle = IdentityBundle::load(&path)?;
                let entry = bundle.cast_entry()?;
                if cast
                    .characters
                    .insert(bundle.character.clone(), entry)
                    .is_some()
                {
                    anyhow::bail!("duplicate character {:?}", bundle.character);
                }
            }
            cast.save(&out)?;
            println!(
                "wrote {} ({} characters)",
                out.display(),
                cast.characters.len()
            );
        }
        Command::Run { config } => {
            let config = BotConfig::load(&config)?;
            larp_bot::bot::run(config).await?;
        }
        Command::Spec { config } => {
            let config = larp_bot::spec::SpecConfig::load(&config)?;
            larp_bot::spec::run(config).await?;
        }
    }
    Ok(())
}
