use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ktrx_core::{ToolPackage, validate_arguments, validate_manifest};
use ktrx_runtime::Runtime;
use std::{
    fs::File,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "ktrx",
    version,
    about = "KTRX — portable WebAssembly tool runner",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a packaged WebAssembly tool.
    Run {
        /// Approve requested network and file-read access for this run without
        /// prompting. Pass before the tool path, e.g. `ktrx run -y <tool> ...`.
        /// Equivalent to answering "y" at the interactive prompt; access is
        /// still limited to permissions the manifest declares.
        #[arg(short = 'y', long = "assume-yes")]
        assume_yes: bool,
        tool: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        #[arg(long, global = true)]
        json: bool,
        /// Stream progress and the final result as newline-delimited JSON events.
        #[arg(long, conflicts_with = "json")]
        events: bool,
    },
    /// Show manifest details.
    Info { tool: PathBuf },
    /// Print the manifest as JSON.
    Describe {
        tool: PathBuf,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    /// Validate the archive, manifest, and WebAssembly component.
    Validate { tool: PathBuf },
    /// Package manifest.toml and module.wasm from a tool directory.
    Pack {
        directory: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() {
    let raw_args: Vec<_> = std::env::args_os().collect();
    if raw_args.len() == 4
        && raw_args[1].to_str() == Some("run")
        && raw_args[3].to_str() == Some("--help")
    {
        let result = ToolPackage::open(&raw_args[2])
            .map_err(anyhow::Error::from)
            .and_then(|package| print_tool_help(&package.manifest));
        if let Err(err) = result {
            eprintln!("ktrx: {err:#}");
            std::process::exit(2);
        }
        return;
    }
    if let Err(err) = execute(Cli::parse()) {
        eprintln!("ktrx: {err:#}");
        std::process::exit(2);
    }
}

fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Run {
            assume_yes,
            tool,
            args,
            json,
            events,
        } => {
            let package =
                ToolPackage::open(&tool).with_context(|| format!("opening {}", tool.display()))?;
            let values = validate_arguments(&package.manifest, &args)?;
            let network_approved = approve(
                &package.manifest.name,
                &package.manifest.permissions,
                ktrx_core::Permission::Network,
                assume_yes,
            )?;
            let file_read_approved = approve(
                &package.manifest.name,
                &package.manifest.permissions,
                ktrx_core::Permission::FileRead,
                assume_yes,
            )?;
            let runtime = std::sync::Arc::new(Runtime::new()?);
            let session = runtime.start_run(
                package,
                serde_json::to_string(&values)?,
                network_approved,
                file_read_approved,
            );
            let (interrupt_tx, interrupt_rx) = std::sync::mpsc::channel();
            ctrlc::set_handler(move || {
                let _ = interrupt_tx.send(());
            })
            .context("installing Ctrl+C handler")?;
            let mut output_event = None;
            loop {
                if interrupt_rx.try_recv().is_ok() {
                    session.cancel();
                }
                match session
                    .events
                    .recv_timeout(std::time::Duration::from_millis(50))
                {
                    Ok(event) => {
                        if events {
                            println!("{}", serde_json::to_string(&event)?);
                        }
                        if let ktrx_runtime::RunEvent::Finished { output, .. } = event {
                            output_event = output;
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            let output = session.join()?;
            let output = output_event.unwrap_or(output);
            if events {
                // The final result was already included in the Finished event.
            } else if json {
                println!("{output}");
            } else if let Ok(value) = serde_json::from_str::<serde_json::Value>(&output) {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("{output}");
            }
        }
        Command::Info { tool } => {
            let p = ToolPackage::open(tool)?;
            println!("{}  {}", p.manifest.name, p.manifest.version);
            println!("{}", p.manifest.description);
            println!(
                "\nID       {}\nAuthor   {}\nKTRX API {}\nPerms    {}",
                p.manifest.id,
                p.manifest.author,
                p.manifest.api_version,
                if p.manifest.permissions.is_empty() {
                    "none".into()
                } else {
                    p.manifest
                        .permissions
                        .iter()
                        .map(|x| match x {
                            ktrx_core::Permission::Network => "network",
                            ktrx_core::Permission::FileRead => "file-read",
                        })
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
            if !p.manifest.examples.is_empty() {
                println!("\nExamples");
                for ex in p.manifest.examples {
                    println!("  {ex}");
                }
            }
        }
        Command::Describe { tool, .. } => {
            let p = ToolPackage::open(tool)?;
            println!("{}", serde_json::to_string_pretty(&p.manifest)?);
        }
        Command::Validate { tool } => {
            let p = ToolPackage::open(&tool)?;
            Runtime::new()?.validate(&p)?;
            println!("Valid: {} ({})", p.manifest.name, p.manifest.id);
        }
        Command::Pack { directory, output } => pack(&directory, output.as_deref())?,
    }
    Ok(())
}

fn approve(
    name: &str,
    permissions: &[ktrx_core::Permission],
    permission: ktrx_core::Permission,
    assume_yes: bool,
) -> Result<bool> {
    if !permissions
        .iter()
        .any(|p| std::mem::discriminant(p) == std::mem::discriminant(&permission))
    {
        return Ok(false);
    }
    if assume_yes {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    let label = match permission {
        ktrx_core::Permission::Network => "network access",
        ktrx_core::Permission::FileRead => "file access",
    };
    eprint!("{} requests {} for this run. Allow? [y/N] ", name, label);
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn print_tool_help(m: &ktrx_core::Manifest) -> Result<()> {
    println!("{} — {}\n\n{}", m.name, m.version, m.description);
    println!("\nUsage: ktrx run <file.tool> [arguments]");
    for a in &m.arguments {
        let form = if a.positional {
            format!("<{}>", a.name)
        } else {
            format!("--{}", a.name)
        };
        println!(
            "  {:24} {}{}",
            form,
            a.description,
            if a.required { " (required)" } else { "" }
        );
    }
    if !m.examples.is_empty() {
        println!("\nExamples");
        for e in &m.examples {
            println!("  {e}");
        }
    }
    Ok(())
}

fn pack(directory: &Path, output: Option<&Path>) -> Result<()> {
    let manifest = directory.join("manifest.toml");
    let module = directory.join("module.wasm");
    let manifest_data =
        std::fs::read(&manifest).with_context(|| format!("reading {}", manifest.display()))?;
    let manifest: ktrx_core::Manifest = toml::from_str(std::str::from_utf8(&manifest_data)?)?;
    validate_manifest(&manifest)?;
    let module_data =
        std::fs::read(&module).with_context(|| format!("reading {}", module.display()))?;
    anyhow::ensure!(
        module_data.starts_with(b"\0asm"),
        "module.wasm is not a WebAssembly binary"
    );
    let output = output.map(Path::to_path_buf).unwrap_or_else(|| {
        directory.join(format!(
            "{}.tool",
            directory.file_name().unwrap_or_default().to_string_lossy()
        ))
    });
    let file = File::create(&output)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("manifest.toml", options)?;
    zip.write_all(&manifest_data)?;
    zip.start_file("module.wasm", options)?;
    zip.write_all(&module_data)?;
    zip.finish()?;
    println!("Created {}", output.display());
    Ok(())
}
