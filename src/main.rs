mod atom;
mod easy_cons;
mod env;
mod lisp_eval;
mod lisp_parsing;
mod sexpr;
mod types;

use std::cell::RefCell;

use rustyline::DefaultEditor;
use rustyline::Result as RlResult;

use env::{current_env_count, env_id, Env};
use lisp_eval::eval;
use lisp_parsing::parse;

thread_local! {
    static REPL_ENV: RefCell<Option<std::sync::Arc<Env>>> = RefCell::new(None);
}

fn init_repl_env() {
    REPL_ENV.with(|env| {
        *env.borrow_mut() = Some(std::sync::Arc::new(Env::root()));
    });
}

fn with_repl_env<F, R>(f: F) -> R
where
    F: FnOnce(&mut std::sync::Arc<Env>) -> R,
{
    REPL_ENV.with(|env| {
        let mut env_ref = env.borrow_mut();
        let env = env_ref.as_mut().unwrap();
        f(env)
    })
}

fn print_welcome() {
    println!("╔══════════════════════════════════════════╗");
    println!("║          Lisp REPL - Type :help          ║");
    println!("║          (Press Ctrl+D to exit)          ║");
    println!("╚══════════════════════════════════════════╝");
    println!();
}

fn print_help() {
    println!(
        "\
Commands:
  :help, :h           Show this help message
  :quit, :q, Ctrl+D   Exit the REPL
  :env, :e            Show all defined variables
  :info, :i           Show REPL info and stats
  :trace, :t          Toggle env creation tracing
  :clear, :cl         Clear the screen

Lisp Features:
  Numbers:     42, 3.14, -7
  Strings:     \"hello world\"
  Symbols:     foo, my-var, add
  Lists:       (1 2 3), (add x y)
  Quote:       (quote (a b c)) or '(a b c)
  Lambda:      (lambda (x) (add x 1))
  If:          (if condition then-expr else-expr)
  Def:         (def name value)
  Labels:      (labels ((f (x) body)) (f arg)) - local recursive functions
  Set!:        (set! name value) - modify existing variable

Builtins:
  Arithmetic:  add, sub, mul, div
  Lists:       list, cons, car, cdr
  Comparison:  eq
  Control:     apply, funcall

Examples:
  > (add 1 2 3)
  => 6
  > (def x 42)
  => 42
  > (labels ((double (x) (mul x 2))) (double 21))
  => 42
  > (labels ((fact (n) (if (eq n 0) 1 (mul n (fact (sub n 1)))))) (fact 5))
  => 120
  > (labels ((fib (n) (if (eq n 0) 0 (if (eq n 1) 1 (add (fib (sub n 1)) (fib (sub n 2))))))) (fib 10))
  => 55"
    );
}

fn print_env() {
    with_repl_env(|env| {
        println!("Defined variables:");
        let mut bindings: Vec<_> = env.local.iter().collect();
        bindings.sort_by(|a, b| a.0.cmp(b.0));
        for (name, value) in bindings {
            println!("  {} => {:?}", name, value);
        }
    });
}

fn print_info() {
    println!("REPL Info:");
    println!("  Version: 0.1.0");
    println!("  Active Envs: {}", current_env_count());
    with_repl_env(|env| {
        println!("  REPL Env ID: #{}", env_id(env));
        println!("  Variables: {}", env.local.len());
    });
}

fn handle_command(line: &str) -> Option<bool> {
    let mut parts = line.split_whitespace();
    let cmd = parts.next()?.trim_start_matches(':');

    match cmd {
        "q" | "quit" => Some(true),
        "h" | "help" => {
            print_help();
            None
        }
        "e" | "env" => {
            print_env();
            None
        }
        "i" | "info" => {
            print_info();
            None
        }
        "t" | "trace" => {
            if env::env_trace_enabled() {
                env::env_trace_disable();
                println!("Env tracing disabled");
            } else {
                env::env_trace_enable();
                println!("Env tracing enabled");
            }
            None
        }
        "cl" | "clear" => {
            print!("\x1B[2J\x1B[H");
            None
        }
        _ => {
            eprintln!("Unknown command: :{cmd} (try :help)");
            None
        }
    }
}

fn eval_and_print(line: &str) {
    let parsed = parse(line);
    with_repl_env(|env| match eval(parsed.clone().into(), env) {
        Ok(val) => {
            println!("=> {:?}", val.as_ref());
        }
        Err(e) => {
            eprintln!("!> Error: {e}");
        }
    });
}

fn run_repl() -> RlResult<()> {
    init_repl_env();

    let hist_path = ".lisp_repl_history";
    let mut rl = DefaultEditor::new()?;

    if rl.load_history(hist_path).is_err() {
        println!("(Starting fresh history)");
    }

    print_welcome();

    let mut input_buffer = String::new();
    let mut paren_depth: i32 = 0;

    loop {
        let prompt = if input_buffer.is_empty() {
            "lisp> "
        } else {
            "..> "
        };

        match rl.readline(prompt) {
            Ok(line) => {
                let line = line.trim();

                if line.is_empty() {
                    if input_buffer.is_empty() {
                        continue;
                    } else {
                        continue;
                    }
                }

                rl.add_history_entry(line)?;

                if line.starts_with(':') && input_buffer.is_empty() {
                    if let Some(should_quit) = handle_command(line) {
                        if should_quit {
                            break;
                        }
                    }
                    continue;
                }

                let mut this_line_depth: i32 = 0;
                for c in line.chars() {
                    if c == '(' {
                        this_line_depth += 1;
                    } else if c == ')' {
                        this_line_depth -= 1;
                    }
                }
                paren_depth += this_line_depth;

                if !input_buffer.is_empty() {
                    input_buffer.push(' ');
                }
                input_buffer.push_str(line);

                if paren_depth <= 0 && !input_buffer.trim().is_empty() {
                    let full_input = input_buffer.trim().to_string();
                    input_buffer.clear();
                    paren_depth = 0;
                    eval_and_print(&full_input);
                }
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                if !input_buffer.is_empty() {
                    let full_input = input_buffer.trim().to_string();
                    eval_and_print(&full_input);
                }
                println!("\nGoodbye!");
                break;
            }
            Err(err) => {
                eprintln!("Readline error: {:?}", err);
                break;
            }
        }
    }

    rl.save_history(hist_path).ok();
    Ok(())
}

fn main() {
    if let Err(e) = run_repl() {
        eprintln!("REPL error: {:?}", e);
    }
}
