use hive_core::db::hive_db::HiveDb;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::env;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mut db_path = PathBuf::from(".hive");

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let value = args.next().ok_or("missing value for --db")?;
                db_path = PathBuf::from(value);
            }
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            _ => {
                db_path = PathBuf::from(arg);
            }
        }
    }

    let mut db = HiveDb::open(&db_path).map_err(|error| error.to_string())?;

    repl(&mut db, db_path)
}

fn repl(db: &mut HiveDb, mut db_path: PathBuf) -> Result<(), String> {
    print_banner();
    println!("Connected to {}", db_path.display());
    println!("Use .help for commands.");

    let mut editor = DefaultEditor::new().map_err(|error| error.to_string())?;

    loop {
        let line = match editor.readline("hive> ") {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => {
                println!();
                return Ok(());
            }
            Err(error) => return Err(error.to_string()),
        };

        let input = line.trim();

        if input.is_empty() {
            continue;
        }

        let _ = editor.add_history_entry(input);

        match input {
            ".exit" | ".quit" => return Ok(()),
            ".help" => print_repl_help(None),
            ".status" => {
                println!("Connected to {}", db_path.display());
            }
            _ if input.starts_with(".help ") => {
                let topic = input.trim_start_matches(".help ").trim();
                print_repl_help(Some(topic));
            }
            _ if input.starts_with(".open ") => {
                let next_path = input.trim_start_matches(".open ").trim();
                if next_path.is_empty() {
                    println!("usage: .open <path>");
                    continue;
                }

                let next_db_path = PathBuf::from(next_path);
                *db = HiveDb::open(&next_db_path).map_err(|error| error.to_string())?;
                db_path = next_db_path;
                println!("Connected to {}", db_path.display());
            }
            _ if input.starts_with(".explain ") => {
                let query = input.trim_start_matches(".explain ").trim();
                if query.is_empty() {
                    println!("usage: .explain <query>");
                    continue;
                }
                match db.explain(query) {
                    Ok(plan) => println!("{plan}"),
                    Err(error) => println!("{error}"),
                }
            }
            _ if input.starts_with(".inspect ") => {
                let arg = input.trim_start_matches(".inspect ").trim();
                match arg.parse::<u32>() {
                    Ok(page_id) => match db.inspect_page(page_id) {
                        Ok(report) => println!("{report}"),
                        Err(error) => println!("{error}"),
                    },
                    Err(_) => println!("usage: .inspect <page_id>"),
                }
            }
            _ if input.starts_with(".wal") => {
                let arg = input.trim_start_matches(".wal").trim();
                let limit = if arg.is_empty() {
                    None
                } else {
                    match arg.parse::<usize>() {
                        Ok(n) => Some(n),
                        Err(_) => {
                            println!("usage: .wal [limit]");
                            continue;
                        }
                    }
                };
                match db.inspect_wal(limit) {
                    Ok(entries) => {
                        if entries.is_empty() {
                            println!("(empty WAL)");
                        }
                        for entry in &entries {
                            println!("{entry}");
                        }
                    }
                    Err(error) => println!("{error}"),
                }
            }
            ".stats" => match db.stats() {
                Ok(stats) => println!("{stats}"),
                Err(error) => println!("{error}"),
            },
            ".check" => {
                let mut problems = Vec::new();
                match db.check_integrity() {
                    Ok(found) => problems.extend(found),
                    Err(error) => println!("{error}"),
                }
                match db.check_index_consistency() {
                    Ok(found) => problems.extend(found),
                    Err(error) => println!("{error}"),
                }
                if problems.is_empty() {
                    println!("OK: storage and indexes are consistent");
                } else {
                    for problem in &problems {
                        println!("- {problem}");
                    }
                }
            }
            _ => match db.execute_timed(input) {
                Ok((result, metrics)) => {
                    println!("{result}");
                    println!("{metrics}");
                }
                Err(error) => println!("{error}"),
            },
        }
    }
}

fn print_banner() {
    print_unicode_logo();
    println!();
}

fn print_unicode_logo() {
    const YELLOW: &str = "\x1b[38;2;245;214;0m";
    const RESET: &str = "\x1b[0m";
    let logo = [
        "██╗  ██╗██╗██╗   ██╗███████╗",
        "██║  ██║██║██║   ██║██╔════╝",
        "███████║██║██║   ██║█████╗  ",
        "██╔══██║██║╚██╗ ██╔╝██╔══╝  ",
        "██║  ██║██║ ╚████╔╝ ███████╗",
        "╚═╝  ╚═╝╚═╝  ╚═══╝  ╚══════╝",
    ];

    for line in logo {
        println!("{YELLOW}{line}{RESET}");
    }
}

fn print_help() {
    print_cli_help();
    println!();
    print_repl_help(None);
}

fn print_cli_help() {
    println!("Usage: cargo run -p hive_cli -- [--db <path>]");
    println!("       cargo run -p hive_cli -- [path-to-db-directory]");
    println!();
    println!("Startup options:");
    println!("  --db <path>        Open or create a database directory");
    println!("  -h, --help         Show this help text");
}

fn print_repl_help(topic: Option<&str>) {
    match topic {
        None => {
            println!("Help topics:");
            println!("  .help commands   Show REPL commands");
            println!("  .help path       Show database path usage");
        }
        Some("commands") => {
            println!("REPL commands:");
            println!("  .help              Show help topics");
            println!("  .help commands     Show REPL commands");
            println!("  .help path         Show database path usage");
            println!("  .open <path>       Open another database directory");
            println!("  .status            Print the current database path");
            println!("  .explain <query>   Print the query plan without running it");
            println!("  .stats             Print database statistics");
            println!("  .check             Run storage and index consistency checks");
            println!("  .inspect <page>    Dump a byte-level page report");
            println!("  .wal [limit]       List WAL entries, oldest first (recent N with limit)");
            println!("  .quit              Exit the CLI");
            println!("  .exit              Exit the CLI");
        }
        Some("path") => {
            println!("Database path usage:");
            println!("  cargo run -p hive_cli -- --db ./.hive");
            println!("  cargo run -p hive_cli -- ./.hive");
            println!("  hive> .open ./another-db");
            println!("  hive> .status");
        }
        Some(_) => {
            println!("Unknown help topic.");
            println!("Try: .help commands, .help path");
        }
    }
}
