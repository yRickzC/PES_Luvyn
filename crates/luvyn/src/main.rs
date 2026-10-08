mod integration;
mod server;

use clap::{Parser, Subcommand};
use luvyn_core::{
    Error, Project, Result,
    query::{self, QueryOptions},
};
use std::{
    collections::BTreeMap,
    io::{self, Read},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    name = "luvyn",
    version,
    about = "Compile documentation into focused AI context. Open a workspace with: luvyn ."
)]
struct Cli {
    #[arg(value_name = "WORKSPACE")]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Compile .lyn documents into an indexed, versioned binary graph.
    Build {
        #[arg(default_value = ".")]
        workspace: PathBuf,
    },
    /// Validate syntax, imports, references and duplicate symbols.
    Check {
        #[arg(default_value = ".")]
        workspace: PathBuf,
        #[arg(long,default_value="text",value_parser=["text","json"])]
        format: String,
    },
    /// Retrieve compact context. DSL: in:depends X, out:exposes X, neighbors X, path A -> B.
    Get {
        query: Option<String>,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
        #[arg(long, default_value_t = 1)]
        depth: usize,
        #[arg(long, default_value_t = 12)]
        limit: usize,
        #[arg(long, default_value_t = 1800)]
        budget: usize,
        #[arg(long,default_value="compact",value_parser=["compact","text","markdown","json"])]
        format: String,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        rebuild: bool,
    },
    /// Export deterministic, compact Markdown documentation as ZIP.
    Export {
        #[arg(default_value = ".")]
        workspace: PathBuf,
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Idempotently ignore generated artifacts, preserving all .lyn sources.
    Git {
        #[arg(default_value = ".")]
        workspace: PathBuf,
    },
    /// Install the Codex context skill; falls back to .agents/skills in this workspace.
    Skill {
        #[arg(default_value = ".")]
        workspace: PathBuf,
        #[arg(long)]
        local: bool,
        #[arg(long)]
        directory: Option<PathBuf>,
    },
    /// Format .lyn documents. --stdin supports any editor integration.
    Fmt {
        #[arg(default_value = ".")]
        workspace: PathBuf,
        #[arg(long)]
        check: bool,
        #[arg(long)]
        stdin: bool,
    },
    /// Open the IDE. --no-open runs a local server without launching a browser.
    Ide {
        #[arg(default_value = ".")]
        workspace: PathBuf,
        #[arg(long)]
        no_open: bool,
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
}

fn diagnostics(project: &Project) {
    for d in &project.graph.diagnostics {
        eprintln!(
            "{}:{}:{}: {}[{}]: {}{}",
            d.location.file,
            d.location.line,
            d.location.column,
            d.severity,
            d.code,
            d.message,
            d.suggestion
                .as_ref()
                .map(|s| format!(" ({s})"))
                .unwrap_or_default()
        );
    }
}
fn compile(project: &mut Project) -> Result<()> {
    let result = project.build();
    diagnostics(project);
    result.map(|stats| {
        eprintln!(
            "Built {} files, {} symbols, {} edges ({} parsed, {} reused)",
            stats.files, stats.symbols, stats.edges, stats.parsed, stats.reused
        );
    })
}
fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(cli) {
        eprintln!("luvyn: {error}");
        std::process::exit(1);
    }
}
fn launch_ide(workspace: PathBuf, no_open: bool, port: u16) -> Result<()> {
    // CLI queries do not start an async runtime or a thread pool.
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(server::run(workspace, no_open, port))
}
fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => launch_ide(
            cli.workspace.unwrap_or_else(|| PathBuf::from(".")),
            false,
            0,
        ),
        Some(Command::Ide {
            workspace,
            no_open,
            port,
        }) => launch_ide(workspace, no_open, port),
        Some(Command::Build { workspace }) => compile(&mut Project::open(&workspace)?),
        Some(Command::Check { workspace, format }) => {
            let mut project = Project::open(&workspace)?;
            project.analyze(&BTreeMap::new())?;
            if format == "json" {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&project.graph.diagnostics)
                        .map_err(|e| Error::Message(e.to_string()))?
                );
            } else {
                diagnostics(&project);
                eprintln!(
                    "Checked {} files, {} symbols",
                    project.stats.files, project.stats.symbols
                );
            }
            if project.graph.has_errors() {
                Err(Error::Message("Check failed".into()))
            } else {
                Ok(())
            }
        }
        Some(Command::Get {
            query: input,
            workspace,
            depth,
            limit,
            budget,
            format,
            stdin,
            rebuild,
        }) => {
            let mut project = Project::open(&workspace)?;
            if rebuild || !project.safe_path(&project.config.output)?.exists() {
                compile(&mut project)?;
            }
            let mut artifact =
                luvyn_core::binary::Artifact::open(&project.safe_path(&project.config.output)?)?;
            // A compiled query is independent from source parsing and startup remains small.
            let input = if stdin {
                let mut text = String::new();
                io::stdin().take(16 * 1024).read_to_string(&mut text)?;
                text
            } else {
                input.ok_or_else(|| Error::Message("Provide a query or --stdin".into()))?
            };
            let result = artifact.query(
                input.trim(),
                &QueryOptions {
                    depth,
                    limit,
                    budget,
                },
            )?;
            println!("{}", query::render(&result, &format, budget)?);
            Ok(())
        }
        Some(Command::Export { workspace, output }) => {
            let mut project = Project::open(&workspace)?;
            compile(&mut project)?;
            let destination =
                project.safe_path(output.as_deref().unwrap_or(&project.config.export))?;
            luvyn_core::export::write(&destination, &project.graph)?;
            println!("{}", destination.display());
            Ok(())
        }
        Some(Command::Git { workspace }) => {
            let project = Project::open(&workspace)?;
            integration::git(&project)?;
            println!("Updated .gitignore");
            Ok(())
        }
        Some(Command::Skill {
            workspace,
            local,
            directory,
        }) => {
            let project = Project::open(&workspace)?;
            let destination = integration::skill(&project, local, directory)?;
            println!("{}", destination.display());
            Ok(())
        }
        Some(Command::Fmt {
            workspace,
            check,
            stdin,
        }) => {
            if stdin {
                let mut text = String::new();
                io::stdin()
                    .take(luvyn_core::workspace::MAX_SOURCE as u64 + 1)
                    .read_to_string(&mut text)?;
                if text.len() > luvyn_core::workspace::MAX_SOURCE {
                    return Err(Error::Message("Input exceeds 2 MiB".into()));
                }
                print!("{}", luvyn_core::formatter::format(&text));
                return Ok(());
            }
            let project = Project::open(&workspace)?;
            let mut changed = 0;
            for path in project.discover()? {
                let path = project.safe_path(&path)?;
                let text = luvyn_core::workspace::read_source(&path)?;
                let formatted = luvyn_core::formatter::format(&text);
                if text != formatted {
                    changed += 1;
                    if !check {
                        luvyn_core::workspace::atomic_write(&path, formatted.as_bytes())?;
                    }
                }
            }
            if check && changed > 0 {
                return Err(Error::Message(format!(
                    "{changed} documents require formatting"
                )));
            }
            eprintln!("{changed} documents formatted");
            Ok(())
        }
    }
}
