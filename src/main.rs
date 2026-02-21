mod atom;
mod easy_cons;
mod env;
mod lisp_eval;
mod lisp_parsing;
mod sexpr;

use std::{
    fs,
    process::exit,
    sync::Arc,
};

use atom::Atom;
use env::Env;
use lisp_eval::eval;
use lisp_parsing::parse;

use rustyline::{error::ReadlineError, DefaultEditor};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = ReplState {
        loaded_file: None,
        loaded_text: String::new(),
        env: Env::default(),
    };

    if let Some(input_file) = std::env::args().nth(1) {
        if let Err(e) = load_file(&input_file, &mut state) {
            eprintln!("error loading {input_file}: {e}");
            exit(1);
        }
        println!("Loaded file: {input_file}");
    }

    let mut rl = DefaultEditor::new()?; // enables line editing (Up/Down history, etc.)
    let hist_path = ".myrepl_history";

    // Persistent history across runs (feature is enabled by default in rustyline). :contentReference[oaicite:2]{index=2}
    let _ = rl.load_history(hist_path);

    loop {
        match rl.readline("> ") {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // store in history so Up/Down works immediately
                let _ = rl.add_history_entry(line);

                if line.starts_with(":") {
                    if handle_command(line, &mut state) {
                        break;
                    } else {
                        continue;
                    }
                }

                let input = parse(&line);
                let res = eval(input.into(), &mut state.env);
                match res {
                    Ok(atom) => println!("=> {:#?}", atom),
                    Err(err) => println!("!> {}", err),
                }
            }
            Err(ReadlineError::Interrupted) => {
                break;
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                eprintln!("readline error: {err:?}");
                break;
            }
        }
    }

    let _ = rl.save_history(hist_path);
    Ok(())
}

struct ReplState {
    loaded_file: Option<String>,
    loaded_text: String,
    env: Env,
}

fn load_file(path: &str, state: &mut ReplState) -> Result<Arc<Atom>, &'static str> {
    let contents = fs::read_to_string(path);
    state.loaded_file = Some(path.to_string());
    state.loaded_text = contents.or_else(|_| Err("Coundn't load input file"))?;
    eval(parse(&state.loaded_text).into(), &mut state.env)
}

// Return `true` to exit the REPL.
fn handle_command(cmdline: &str, state: &mut ReplState) -> bool {
    let mut parts = cmdline.split_whitespace();
    let cmd = parts.next().unwrap_or("");

    match cmd {
        ":q" | ":quit" => true,
        ":help" => {
            println!(
                "\
Commands:
  :help            Show this help
  :quit | :q       Exit
  :load <path>     Load a file into the REPL state
  :show            Print currently loaded file text (if any)
  :clear           Clear loaded file/text
Anything else is sent to eval()."
            );
            false
        }
        ":load" => {
            let path = match parts.next() {
                Some(p) => p,
                None => {
                    eprintln!("usage: :load <path>");
                    return false;
                }
            };
            match load_file(path, state) {
                Ok(r) => println!("Loaded file result:\n{r:#?}"),
                Err(e) => eprintln!("error loading {path}: {e}"),
            }
            false
        }
        ":show" => {
            if let Some(p) = &state.loaded_file {
                println!("--- {p} ---\n{}", state.loaded_text);
            } else {
                println!("(no file loaded)");
            }
            false
        }
        ":clear" => {
            state.loaded_file = None;
            state.loaded_text.clear();
            println!("Cleared.");
            false
        }
        _ => {
            eprintln!("unknown command: {cmd} (try :help)");
            false
        }
    }
}
