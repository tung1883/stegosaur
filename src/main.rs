use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use stegosaur::analysis::analyze;
use stegosaur::container::{
    deniable_capacity, embed_deniable, embed_single, extract_deniable, extract_single, single_capacity,
};
use stegosaur::distribute::{distribute, recover};
use stegosaur::corpora::Style;
use stegosaur::linguistic::{decode, encode, NgramModel};
use stegosaur::{AnyCarrier, Config, Error, ImageCarrier, Mode, Result};

#[derive(Parser)]
#[command(name = "stegosaur", version, about = "Encrypted steganography with two-passphrase hiding, threshold sharing and linguistic cover text")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum StyleArg {
    /// English sentences
    Prose,
    /// Linux syslog / auth.log
    Syslog,
    /// PostgreSQL server log
    Postgres,
    /// nginx access log
    Nginx,
    /// .env-style config with secret-looking values
    Env,
}

impl From<StyleArg> for Style {
    fn from(s: StyleArg) -> Self {
        match s {
            StyleArg::Prose => Style::Prose,
            StyleArg::Syslog => Style::Syslog,
            StyleArg::Postgres => Style::Postgres,
            StyleArg::Nginx => Style::Nginx,
            StyleArg::Env => Style::Env,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    /// LSB matching (+-1): much harder for classical detectors to see
    Match,
    /// LSB replacement: fast, easily detected
    Replace,
}

#[derive(Args)]
struct Common {
    /// Embedding mode
    #[arg(long, value_enum, default_value = "match")]
    mode: ModeArg,
    /// Reed-Solomon check bytes per 255-byte block (0 = off); extraction must use the same value
    #[arg(long, default_value_t = 16)]
    parity: usize,
}

#[derive(Subcommand)]
enum Cmd {
    /// Pick a natural file name (camera style for images, recorder style for audio) that carries a tag only your passphrase can recognise
    TagName {
        /// Folder the new file will live in
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// File extension, e.g. png or wav
        #[arg(long, default_value = "png")]
        ext: String,
        /// single, hidden or share
        #[arg(long, default_value = "single")]
        kind: String,
        #[arg(long, env = "STEGOSAUR_PASS", hide_env_values = true)]
        pass: String,
    },
    /// Find and recover everything in a folder that the given passphrases open
    Recover {
        folder: PathBuf,
        /// Text file with one passphrase per line (STEGOSAUR_PASS adds one more)
        #[arg(long)]
        pass_file: Option<PathBuf>,
        /// Where recovered files go (default: <folder>/recovered)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Also look inside sub-folders
        #[arg(long)]
        recursive: bool,
        /// Only try a passphrase on files whose name carries its tag (faster)
        #[arg(long)]
        names_only: bool,
    },
    /// Say which of the given files were named with this passphrase (and what they are)
    TagCheck {
        files: Vec<PathBuf>,
        #[arg(long, env = "STEGOSAUR_PASS", hide_env_values = true)]
        pass: String,
    },
    /// Hide a file in a PNG/BMP/WAV carrier
    Embed {
        carrier: PathBuf,
        out: PathBuf,
        #[arg(long)]
        data: PathBuf,
        /// Passphrase (or set STEGOSAUR_PASS)
        #[arg(long, env = "STEGOSAUR_PASS", hide_env_values = true)]
        pass: String,
        #[command(flatten)]
        common: Common,
    },
    /// Recover a file hidden with `embed`
    Extract {
        carrier: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, env = "STEGOSAUR_PASS", hide_env_values = true)]
        pass: String,
        #[arg(long, default_value_t = 16)]
        parity: usize,
    },
    /// Hide a public file and (optionally) a hidden file under two passphrases
    #[command(alias = "deniable-embed")]
    HiddenEmbed {
        carrier: PathBuf,
        out: PathBuf,
        /// File revealed by the first passphrase
        #[arg(long, alias = "decoy-data")]
        public_data: PathBuf,
        #[arg(long, alias = "decoy-pass")]
        public_pass: String,
        #[arg(long, requires = "hidden_pass")]
        hidden_data: Option<PathBuf>,
        #[arg(long, requires = "hidden_data")]
        hidden_pass: Option<String>,
        /// Fraction of each half's samples to use (bits per sample)
        #[arg(long, default_value_t = 0.1)]
        rate: f64,
        #[command(flatten)]
        common: Common,
    },
    /// Recover whichever payload the passphrase unlocks
    #[command(alias = "deniable-extract")]
    HiddenExtract {
        carrier: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        pass: String,
        #[arg(long, default_value_t = 0.1)]
        rate: f64,
        #[arg(long, default_value_t = 16)]
        parity: usize,
    },
    /// Estimate how detectable an embedding is (chi-square, RS, SPA)
    Analyze { image: PathBuf },
    /// Split a secret across carriers; any K of them recover it
    Split {
        #[arg(long)]
        secret: PathBuf,
        #[arg(long)]
        k: u8,
        #[arg(long, env = "STEGOSAUR_PASS", hide_env_values = true)]
        pass: String,
        /// Output directory for the modified carriers
        #[arg(long)]
        out_dir: PathBuf,
        /// One carrier per share
        #[arg(required = true)]
        carriers: Vec<PathBuf>,
        #[command(flatten)]
        common: Common,
    },
    /// Recover a split secret from carriers
    Join {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, env = "STEGOSAUR_PASS", hide_env_values = true)]
        pass: String,
        #[arg(long, default_value_t = 16)]
        parity: usize,
        #[arg(required = true)]
        carriers: Vec<PathBuf>,
    },
    /// Hide a file in generated English-like cover text
    TextEmbed {
        #[arg(long)]
        data: PathBuf,
        #[arg(long, env = "STEGOSAUR_PASS", hide_env_values = true)]
        pass: String,
        /// Training text for the language model (a longer, more natural corpus gives better cover text)
        #[arg(long)]
        corpus: Option<PathBuf>,
        /// Cover-text style; extraction must use the same style and top-k
        #[arg(long, value_enum, default_value = "prose")]
        style: StyleArg,
        /// Candidate words per step (more bits per word, less natural text)
        #[arg(long)]
        top_k: Option<usize>,
        /// Words of context the model conditions on (higher = more realistic lines, less capacity)
        #[arg(long)]
        order: Option<usize>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Recover a file from cover text
    TextExtract {
        text: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, env = "STEGOSAUR_PASS", hide_env_values = true)]
        pass: String,
        #[arg(long)]
        corpus: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "prose")]
        style: StyleArg,
        #[arg(long)]
        top_k: Option<usize>,
        #[arg(long)]
        order: Option<usize>,
    },
}

fn config(common: &Common, rate: f64) -> Config {
    let mode = match common.mode {
        ModeArg::Match => Mode::Match,
        ModeArg::Replace => Mode::Replace,
    };
    Config { mode, rate, parity: common.parity, ..Config::default() }
}

fn model(style: StyleArg, corpus: &Option<PathBuf>, top_k: Option<usize>, order: Option<usize>) -> Result<NgramModel> {
    let text = match corpus {
        Some(p) => Some(std::fs::read_to_string(p)?),
        None => None,
    };
    Ok(Style::from(style).model(text.as_deref(), top_k, order))
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Embed { carrier, out, data, pass, common } => {
            let mut c = AnyCarrier::load(&carrier)?;
            let bytes = std::fs::read(data)?;
            embed_single(&mut c, pass.as_bytes(), &bytes, &config(&common, 0.1))?;
            c.save(&out)?;
            println!("hid {} bytes of {} available in {}", bytes.len(), single_capacity(&c, &config(&common, 0.1)), out.display());
        }
        Cmd::Extract { carrier, out, pass, parity } => {
            let c = AnyCarrier::load(&carrier)?;
            let bytes = extract_single(&c, pass.as_bytes(), &Config { parity, ..Config::default() })?;
            std::fs::write(&out, &bytes)?;
            println!("recovered {} bytes", bytes.len());
        }
        Cmd::HiddenEmbed { carrier, out, public_data, public_pass, hidden_data, hidden_pass, rate, common } => {
            let cfg = config(&common, rate);
            let mut c = AnyCarrier::load(&carrier)?;
            let public = std::fs::read(public_data)?;
            let hidden = match (hidden_data, hidden_pass) {
                (Some(d), Some(p)) => Some((std::fs::read(d)?, p)),
                _ => None,
            };
            embed_deniable(
                &mut c,
                (public_pass.as_bytes(), &public),
                hidden.as_ref().map(|(d, p)| (p.as_bytes(), d.as_slice())),
                &cfg,
            )?;
            c.save(&out)?;
            println!("wrote {} (each slot holds up to {} bytes)", out.display(), deniable_capacity(&c, &cfg));
        }
        Cmd::HiddenExtract { carrier, out, pass, rate, parity } => {
            let c = AnyCarrier::load(&carrier)?;
            let cfg = Config { rate, parity, ..Config::default() };
            let bytes = extract_deniable(&c, pass.as_bytes(), &cfg)?;
            std::fs::write(&out, &bytes)?;
            println!("recovered {} bytes", bytes.len());
        }
        Cmd::Recover { folder, pass_file, out, recursive, names_only } => {
            use stegosaur::app::bulk::{list_files, run as bulk_run, BulkOptions, Found};
            let mut passes: Vec<stegosaur::app::Secret> = Vec::new();
            if let Some(f) = pass_file {
                for line in std::fs::read_to_string(&f)?.lines() {
                    let line = line.trim_end_matches('\r');
                    if !line.is_empty() {
                        passes.push(stegosaur::app::Secret::new(line));
                    }
                }
            }
            if let Ok(p) = std::env::var("STEGOSAUR_PASS") {
                if !p.is_empty() {
                    passes.push(stegosaur::app::Secret::new(p));
                }
            }
            let out_dir = out.unwrap_or_else(|| folder.join("recovered"));
            let files = list_files(&folder, recursive, Some(&out_dir))?;
            let opts = BulkOptions { passes, out_dir: out_dir.clone(), cfg: stegosaur::app::AppConfig::default(), names_only, text: None };
            let total = files.len();
            let report = bulk_run(&files, &opts, &mut |done, _, name| eprintln!("[{done}/{total}] {name}"))?;
            println!();
            for e in &report.entries {
                let name = e.file.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if let Some(why) = &e.skipped {
                    println!("{name:<34} skipped: {why}");
                } else if e.found.is_empty() {
                    println!("{name:<34} nothing found");
                }
                for f in &e.found {
                    match f {
                        Found::File { layout, path, bytes, .. } => println!("{name:<34} {layout}: {bytes} bytes -> {}", path.display()),
                        Found::Share { set, k, n, .. } => println!("{name:<34} share of split {set} (needs {k} of {n})"),
                    }
                }
            }
            for s in &report.sets {
                match &s.output {
                    Some(p) => println!("split {}: rebuilt from {} of {} shares: {} bytes -> {}", s.set, s.found, s.n, s.bytes, p.display()),
                    None => println!("split {}: only {} of the {} shares needed; {} more required", s.set, s.found, s.k, s.missing),
                }
            }
            println!("\n{} item(s) recovered into {}", report.recovered_files(), out_dir.display());
        }
        Cmd::TagName { dir, ext, kind, pass } => {
            let kind = match kind.as_str() {
                "single" => stegosaur::tag::Kind::Single,
                "hidden" => stegosaur::tag::Kind::Hidden,
                "share" => stegosaur::tag::Kind::Share,
                other => return Err(stegosaur::Error::Invalid(format!("kind must be single, hidden or share, not {other}")).into()),
            };
            let audio = stegosaur::carrier::is_audio_ext(&ext.to_ascii_lowercase());
            let name = stegosaur::tag::tagged_name(pass.as_bytes(), kind, &dir, &ext, audio, stegosaur::crypto::KdfParams::default())?;
            println!("{name}");
        }
        Cmd::TagCheck { files, pass } => {
            for f in &files {
                let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                let kinds = stegosaur::tag::check_name(pass.as_bytes(), name, stegosaur::crypto::KdfParams::default())?;
                let what: Vec<&str> = kinds.iter().map(|k| match k {
                    stegosaur::tag::Kind::Single => "one file",
                    stegosaur::tag::Kind::Hidden => "two-passphrase layout",
                    stegosaur::tag::Kind::Share => "share of a split secret",
                }).collect();
                println!("{:<40} {}", name, if what.is_empty() { "not tagged for this passphrase".to_string() } else { what.join(" / ") });
            }
        }
        Cmd::Analyze { image } => {
            let is_jpeg = stegosaur::carrier::ext_of(&image).is_some_and(|e| stegosaur::carrier::is_jpeg_ext(&e));
            if is_jpeg {
                let j = stegosaur::JpegCarrier::load(&image)?;
                let r = stegosaur::analysis::analyze_jpeg(&j)
                    .ok_or_else(|| stegosaur::Error::Unsupported("this JPEG is too small for the coefficient detector".into()))?;
                println!("altered-coefficient estimate {:>7.3}  (fraction of non-zero AC coefficients changed)", r.rs);
                println!("detectability      {:>6.1}/100  [{:?}]", r.score, r.risk);
                println!("note: calibrated-histogram detector, reliable for heavy use; small payloads stay inside its noise. Not proof of safety");
                return Ok(());
            }
            let img = ImageCarrier::load(&image)?;
            let r = analyze(&img);
            println!("RS estimate        {:>7.3}  (fraction of samples carrying a message)", r.rs);
            println!("SPA estimate       {:>7.3}", r.spa);
            println!("chi-square p       {:>7.3}  (unreliable on smooth histograms)", r.chi_p);
            println!("detectability      {:>6.1}/100  [{:?}]", r.score, r.risk);
            println!("note: classical detectors only; a low score is not proof of safety");
        }
        Cmd::Split { secret, k, pass, out_dir, carriers, common } => {
            let bytes = std::fs::read(secret)?;
            let mut loaded: Vec<AnyCarrier> = carriers.iter().map(|p| AnyCarrier::load(p)).collect::<Result<_>>()?;
            distribute(&bytes, pass.as_bytes(), k, &mut loaded, &config(&common, 0.1))?;
            std::fs::create_dir_all(&out_dir)?;
            for (path, c) in carriers.iter().zip(&loaded) {
                let name = path.file_name().ok_or_else(|| Error::Invalid("carrier has no file name".into()))?;
                c.save(&out_dir.join(name))?;
            }
            println!("split into {} shares, any {} recover it", loaded.len(), k);
        }
        Cmd::Join { out, pass, parity, carriers } => {
            let loaded: Vec<AnyCarrier> = carriers
                .iter()
                .filter_map(|p| AnyCarrier::load(p).ok())
                .collect();
            let bytes = recover(pass.as_bytes(), &loaded, &Config { parity, ..Config::default() })?;
            std::fs::write(&out, &bytes)?;
            println!("recovered {} bytes from {} carriers", bytes.len(), loaded.len());
        }
        Cmd::TextEmbed { data, pass, corpus, style, top_k, order, out } => {
            let m = model(style, &corpus, top_k, order)?;
            let text = encode(&m, pass.as_bytes(), &std::fs::read(data)?, Default::default())?;
            std::fs::write(&out, &text)?;
            println!("wrote {} words to {}", text.split_whitespace().count(), out.display());
        }
        Cmd::TextExtract { text, out, pass, corpus, style, top_k, order } => {
            let m = model(style, &corpus, top_k, order)?;
            let bytes = decode(&m, pass.as_bytes(), &std::fs::read_to_string(text)?, Default::default())?;
            std::fs::write(&out, &bytes)?;
            println!("recovered {} bytes", bytes.len());
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
