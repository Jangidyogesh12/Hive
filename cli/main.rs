use hive_core::db::hive_db::HiveDb;
use hive_core::query::executor::Executor;
use hive_core::query::parser;
use hive_core::query::planner;
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
    let mut query = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let value = args.next().ok_or("missing value for --db")?;
                db_path = PathBuf::from(value);
            }
            "-e" | "--execute" => {
                query = Some(args.next().ok_or("missing value for --execute")?);
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

    if let Some(query) = query {
        execute_query(&mut db, &query)?;
        return Ok(());
    }

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
            _ => {
                if let Err(error) = execute_query(db, input) {
                    println!("error: {error}");
                }
            }
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
        "  ██╗  ██╗ ██╗ ██╗   ██╗ ███████╗  ",
        "  ██║  ██║ ██║ ██║   ██║ ██╔════╝  ",
        "  ███████║ ██║ ██║   ██║ █████╗    ",
        "  ██╔══██║ ██║ ╚██╗ ██╔╝ ██╔══╝    ",
        "  ██║  ██║ ██║  ╚████╔╝  ███████╗  ",
        "  ╚═╝  ╚═╝ ╚═╝   ╚═══╝   ╚══════╝  ",
    ];

    for line in logo {
        println!("{YELLOW}{line}{RESET}");
    }
}

fn execute_query(db: &mut HiveDb, input: &str) -> Result<(), String> {
    let statement = parser::parse(input).map_err(|e| e.to_string())?;
    let plan = planner::plan(statement).map_err(|error| error.to_string())?;
    let result = Executor::new(db)
        .execute(plan)
        .map_err(|error| error.to_string())?;

    if !result.columns.is_empty() {
        println!("{result}");
    }

    Ok(())
}

fn print_help() {
    print_cli_help();
    println!();
    print_repl_help(None);
}

fn print_cli_help() {
    println!("Usage: cargo run -p hive_cli -- [--db <path>] [-e <query>]");
    println!("       cargo run -p hive_cli -- [path-to-db-directory]");
    println!();
    println!("Startup options:");
    println!("  --db <path>        Open or create a database directory");
    println!("  -e, --execute      Run one query and exit");
    println!("  -h, --help         Show this help text");
}

fn print_repl_help(topic: Option<&str>) {
    match topic {
        None => {
            println!("Help topics:");
            println!("  .help commands   Show REPL commands");
            println!("  .help queries    Show supported query forms");
            println!("  .help examples   Show example queries");
            println!("  .help path       Show database path usage");
        }
        Some("commands") => {
            println!("REPL commands:");
            println!("  .help              Show help topics");
            println!("  .help commands     Show REPL commands");
            println!("  .help queries      Show supported query forms");
            println!("  .help examples     Show example queries");
            println!("  .help path         Show database path usage");
            println!("  .open <path>       Open another database directory");
            println!("  .status            Print the current database path");
            println!("  .quit              Exit the CLI");
            println!("  .exit              Exit the CLI");
        }
        Some("queries") => {
            println!("Supported query forms:");
            println!("  CREATE (n:Person {{name: \"Alice\", age: 30}})");
            println!("  CREATE (a:Person)-[:KNOWS]->(b:Person)");
            println!("  MERGE (n:Person {{name: \"Alice\"}})");
            println!("  MATCH (n:Person) RETURN n");
            println!("  MATCH (n:Person) WHERE n.age >= 18 RETURN n.name AS name");
            println!("  MATCH (a)-[:KNOWS]->(b) RETURN a, b");
            println!("  MATCH (a)<-[:KNOWS]-(b) RETURN a, b");
            println!("  MATCH (a)-[:KNOWS*1..3]->(b) RETURN b");
            println!("  SET n.age = 31");
            println!("  DELETE n");
            println!();
            println!("Notes:");
            println!("  - String values must use double quotes");
            println!("  - Supported value types: integers, floats, booleans, strings");
            println!("  - MATCH requires a RETURN clause");
        }
        Some("examples") => {
            println!("Example queries:");
            println!("  CREATE (n:Person {{name: \"Alice\", age: 30}})");
            println!("  CREATE (a:Person)-[:KNOWS]->(b:Person)");
            println!("  MERGE (n:Person {{name: \"Alice\"}})");
            println!("  MATCH (n:Person) RETURN n");
            println!("  MATCH (n:Person) WHERE n.age >= 18 RETURN n.name AS name");
            println!("  MATCH (a)-[:KNOWS*1..3]->(b) RETURN b");
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
            println!("Try: .help commands, .help queries, .help examples, .help path");
        }
    }
}
