mod ide;
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
    #[arg(long, num_args=0..=1, default_missing_value="", value_name="KEYWORD")]
    lang: Option<String>,
    #[arg(long,default_value="text",value_parser=["text","json"])]
    format: String,
    #[arg(value_name = "WORKSPACE")]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Initialize a documentation workspace and optionally connect an existing source project.
    Init {
        #[arg(default_value = ".")]
        workspace: PathBuf,
        #[arg(long)]
        target: Option<String>,
    },
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
    /// Retrieve compact context. DSL: in:depends X, out:export X, neighbors X, path A -> B.
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
        #[arg(long, value_parser = ["ignore", "track"])]
        artifacts: Option<String>,
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
    /// Open the IDE (desktop by default; use --server for a browser host), or build distributions.
    Ide(IdeArgs),
    /// Print the authoritative Core language dictionary.
    Lang {
        keyword: Option<String>,
        #[arg(long,default_value="text",value_parser=["text","json"])]
        format: String,
    },
}
#[derive(clap::Args)]
struct IdeArgs {
    /// Omit to open Projects; use . or a path to open a workspace directly.
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    action: Option<IdeAction>,
    #[arg(long, conflicts_with_all=["desktop","android"])]
    server: bool,
    #[arg(long, conflicts_with = "android")]
    desktop: bool,
    #[arg(long)]
    android: bool,
    /// Compatibility alias: server mode always stays in this terminal.
    #[arg(long, conflicts_with_all=["desktop","android"])]
    foreground: bool,
    #[arg(long)]
    no_open: bool,
    #[arg(long, default_value_t = 0)]
    port: u16,
}
#[derive(Subcommand)]
enum IdeAction {
    /// Build embedded server/desktop distributions from the Luvyn source repository.
    Build {
        #[arg(long,conflicts_with_all=["desktop","android"])]
        server: bool,
        #[arg(long, conflicts_with = "android")]
        desktop: bool,
        #[arg(long)]
        android: bool,
        #[arg(long, default_value = ".")]
        source: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
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
    let result = project.build_with_progress(|line| eprintln!("{line}"));
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
    if let Some(keyword) = cli.lang {
        return print_language(
            (!keyword.is_empty()).then_some(keyword.as_str()),
            &cli.format,
        );
    }
    match cli.command {
        None => start_ide(
            cli.workspace
                .unwrap_or(luvyn_core::projects::launcher_directory()?),
            ide::host::Target::Desktop,
            false,
            0,
        ),
        Some(Command::Lang { keyword, format }) => print_language(keyword.as_deref(), &format),
        Some(Command::Ide(args)) => match args.action {
            Some(IdeAction::Build {
                server: _,
                desktop,
                android,
                source,
                output,
            }) => ide::host::build(target(desktop, android), &source, output.as_deref()),
            None if args.foreground => launch_ide(
                args.workspace
                    .unwrap_or(luvyn_core::projects::launcher_directory()?),
                args.no_open,
                args.port,
            ),
            None => start_ide(
                args.workspace
                    .unwrap_or(luvyn_core::projects::launcher_directory()?),
                ide_target(args.server, args.desktop, args.android),
                args.no_open,
                args.port,
            ),
        },
        Some(Command::Init { workspace, target }) => {
            let root = workspace.canonicalize()?;
            let path = luvyn_core::workspace::safe_path(&root, "luvyn.toml")?;
            if path.exists() {
                return Err(Error::Message(
                    "luvyn.toml already exists; edit project.target to connect a target".into(),
                ));
            }
            if let Some(target) = &target
                && !root.join(target).is_dir()
            {
                return Err(Error::Message(format!(
                    "Target project does not exist: {target}"
                )));
            }
            let mut config = String::from(
                "sources = [\".\"]\noutput = \".luvyn/project.lu\"\n\n[project]\nartifacts = \"ignore\"\n",
            );
            if let Some(target) = target {
                config.push_str(&format!(
                    "target = {}\n",
                    serde_json::to_string(&target).map_err(|e| Error::Message(e.to_string()))?
                ));
            }
            use std::io::Write;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?
                .write_all(config.as_bytes())?;
            let project = Project::open(&root)?;
            println!(
                "Documentation: {}\nTarget: {}",
                project.root.display(),
                project.target_project_root.display()
            );
            Ok(())
        }
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
            if rebuild || !project.output_path()?.exists() {
                compile(&mut project)?;
            }
            let mut artifact = luvyn_core::binary::Artifact::open(&project.output_path()?)?;
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
        Some(Command::Git {
            workspace,
            artifacts,
        }) => {
            let mut project = Project::open(&workspace)?;
            if let Some(policy) = artifacts {
                project.config.project.artifacts = policy;
            }
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
fn target(desktop: bool, android: bool) -> ide::host::Target {
    if android {
        ide::host::Target::Android
    } else if desktop {
        ide::host::Target::Desktop
    } else {
        ide::host::Target::Server
    }
}
fn ide_target(server: bool, desktop: bool, android: bool) -> ide::host::Target {
    if android {
        ide::host::Target::Android
    } else if desktop {
        ide::host::Target::Desktop
    } else if server {
        ide::host::Target::Server
    } else {
        ide::host::Target::Desktop
    }
}
fn print_language(keyword: Option<&str>, format: &str) -> Result<()> {
    if format == "json" {
        let entries: Vec<_> = match keyword {
            Some(k) => vec![
                luvyn_core::language::lookup(k)
                    .ok_or_else(|| Error::Message(format!("Unknown language entry: {k}")))?,
            ],
            None => luvyn_core::language::dictionary().iter().collect(),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&entries).map_err(|e| Error::Message(e.to_string()))?
        );
    } else {
        println!("{}", luvyn_core::language::summary(keyword)?);
    }
    Ok(())
}
fn start_ide(
    workspace: PathBuf,
    target: ide::host::Target,
    no_open: bool,
    port: u16,
) -> Result<()> {
    if target == ide::host::Target::Desktop {
        target.available()?;
        let project = Project::open(&workspace)?;
        ide::host::remember_workspace(&project)?;
        #[cfg(feature = "desktop")]
        return ide::desktop::run(project.root, port);
        #[cfg(not(feature = "desktop"))]
        return Err(Error::Message(
            "Desktop host is not included in this executable; run `luvyn ide build --desktop`"
                .into(),
        ));
    }
    target.available()?;
    let project = Project::open(&workspace)?;
    ide::host::remember_workspace(&project)?;
    launch_ide(project.root, no_open, port)
}

#[cfg(test)]
mod project_cli_tests {
    use super::*;
    #[test]
    fn ide_projects_accepts_optional_workspace_for_both_hosts() {
        for input in [
            vec!["luvyn", "ide"],
            vec!["luvyn", "ide", "--desktop"],
            vec!["luvyn", "ide", ".", "--desktop"],
            vec!["luvyn", "ide", "--server"],
            vec!["luvyn", "ide", ".", "--server"],
            vec!["luvyn", "ide", "C:/project", "--desktop"],
        ] {
            let cli = Cli::try_parse_from(&input).unwrap();
            if let Some(Command::Ide(args)) = cli.command {
                assert_eq!(
                    args.workspace.is_some(),
                    input.iter().any(|v| *v == "." || *v == "C:/project")
                );
            } else {
                panic!("Expected IDE command");
            }
        }
    }

    #[test]
    fn ide_target_defaults_to_the_compiled_host_and_honors_explicit_server() {
        let default = ide_target(false, false, false);
        assert_eq!(default, ide::host::Target::Desktop);
        assert_eq!(ide_target(true, false, false), ide::host::Target::Server);
        assert_eq!(ide_target(false, true, false), ide::host::Target::Desktop);
    }
}
