#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

mod std;

const KEY_ENTER: u8 = 0x0A;
const KEY_BACKSPACE: u8 = 0x08;
const MAX_CMD_LEN: usize = 128;

fn put_char(ch: u8) {
    let buf = [ch];
    unsafe {
        std::print(core::str::from_utf8_unchecked(&buf));
    }
}

fn print(s: &str) {
    std::print(s);
}

fn println(s: &str) {
    std::print(s);
    std::print("\n");
}

fn print_u64(mut val: u64) {
    if val == 0 {
        put_char(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while val > 0 {
        i -= 1;
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    unsafe {
        std::print(core::str::from_utf8_unchecked(
            core::slice::from_raw_parts(buf.as_ptr().add(i), 20 - i),
        ));
    }
}

#[inline(never)]
fn cmd_help() {
    println("Commands:");
    println("  help              Show this help");
    println("  ls                List files");
    println("  cat <file>        Show file contents");
    println("  write <file> <msg> Write msg to a file");
    println("  rm <file>         Delete a file");
    println("  exec <file> [args] Execute a KEF binary");
    println("  a | b             Pipe a's output into b");
    println("  cmd > f, >> f, < f Redirect output (replace/append) or input");
    println("  clear             Clear screen");
    println("  shutdown          Shut down");
}

// ── Pipelines ──────────────────────────────────────────────────────────────
//
// A command line is up to MAX_STAGES commands joined by `|`. Each command may
// take `< file`, `> file` or `>> file`; an explicit redirection wins over the
// pipe on that side. Arguments are passed on joined by single spaces.

const MAX_STAGES: usize = 8;
const MAX_WORDS: usize = 16;

#[derive(Clone, Copy)]
struct Stage<'a> {
    words: [&'a str; MAX_WORDS],
    word_count: usize,
    input: Option<&'a str>,
    output: Option<&'a str>,
    append: bool,
}

const EMPTY_STAGE: Stage<'static> = Stage {
    words: [""; MAX_WORDS],
    word_count: 0,
    input: None,
    output: None,
    append: false,
};

fn parse_stage(text: &str) -> Result<Stage<'_>, &'static str> {
    let mut stage = EMPTY_STAGE;
    let mut tokens = text.split_ascii_whitespace();
    while let Some(token) = tokens.next() {
        let (kind, rest) = if let Some(rest) = token.strip_prefix(">>") {
            (b'a', rest)
        } else if let Some(rest) = token.strip_prefix('>') {
            (b'>', rest)
        } else if let Some(rest) = token.strip_prefix('<') {
            (b'<', rest)
        } else {
            if stage.word_count == MAX_WORDS {
                return Err("too many arguments");
            }
            stage.words[stage.word_count] = token;
            stage.word_count += 1;
            continue;
        };
        let file = if rest.is_empty() {
            tokens.next().ok_or("missing file name after redirection")?
        } else {
            rest
        };
        if kind == b'<' {
            stage.input = Some(file);
        } else {
            stage.output = Some(file);
            stage.append = kind == b'a';
        }
    }
    if stage.word_count == 0 {
        return Err("missing command");
    }
    Ok(stage)
}

/// The KEF to run for a stage and the index of its first argument word.
fn resolve<'a>(stage: &Stage<'a>) -> Option<(&'a str, usize)> {
    let program = match stage.words[0] {
        "ls" => "ls.kef",
        "cat" => "cat.kef",
        "write" => "write.kef",
        "rm" => "rm.kef",
        "ping" => "ping.kef",
        "exec" if stage.word_count > 1 => return Some((stage.words[1], 2)),
        _ => return None,
    };
    Some((program, 1))
}

fn join_args<'b>(words: &[&str], buf: &'b mut [u8; MAX_CMD_LEN]) -> &'b str {
    let mut len = 0;
    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            buf[len] = b' ';
            len += 1;
        }
        buf[len..len + word.len()].copy_from_slice(word.as_bytes());
        len += word.len();
    }
    unsafe { core::str::from_utf8_unchecked(&buf[..len]) }
}

fn open_or_report(name: &str, flags: usize) -> Option<usize> {
    let fd = std::open(name, flags);
    if fd < 0 {
        print("Error: cannot open ");
        println(name);
        return None;
    }
    Some(fd as usize)
}

fn close_owned(fd: Option<usize>) {
    if let Some(fd) = fd {
        if fd > std::STDERR {
            std::close(fd);
        }
    }
}

fn run_builtin(name: &str) -> bool {
    match name {
        "help" => cmd_help(),
        "clear" => std::clear(),
        "shutdown" => {
            println("Goodbye!");
            std::shutdown();
        }
        _ => return false,
    }
    true
}

#[inline(never)]
fn process_command(line: &str) {
    let mut stages = [EMPTY_STAGE; MAX_STAGES];
    let mut count = 0;
    for part in line.split('|') {
        if count == MAX_STAGES {
            println("Error: too many pipeline stages");
            return;
        }
        match parse_stage(part) {
            Ok(stage) => stages[count] = stage,
            Err(message) => {
                print("Error: ");
                println(message);
                return;
            }
        }
        count += 1;
    }
    let stages = &stages[..count];

    let simple = count == 1 && stages[0].input.is_none() && stages[0].output.is_none();
    if simple && run_builtin(stages[0].words[0]) {
        return;
    }
    for stage in stages {
        if resolve(stage).is_none() {
            if stage.words[0] == "exec" {
                println("Usage: exec <file> [args]");
            } else if matches!(stage.words[0], "help" | "clear" | "shutdown") {
                print(stage.words[0]);
                println(" cannot be piped or redirected");
            } else {
                print("Unknown: ");
                println(stage.words[0]);
                println("Type 'help' for commands.");
            }
            return;
        }
    }

    let mut tasks = [usize::MAX; MAX_STAGES];
    let mut prev_read: Option<usize> = None;
    for (i, stage) in stages.iter().enumerate() {
        let (program, first_arg) = resolve(stage).unwrap();
        let mut args_buf = [0u8; MAX_CMD_LEN];
        let args = join_args(&stage.words[first_arg..stage.word_count], &mut args_buf);

        let pipe = if i + 1 < count {
            let pipe = std::pipe();
            if pipe.is_none() {
                println("Error: cannot create pipe");
                close_owned(prev_read);
                break;
            }
            pipe
        } else {
            None
        };
        let pipe_write = pipe.map(|(_, write_fd)| write_fd);

        let stdin = match stage.input {
            Some(name) => open_or_report(name, std::O_READ),
            None => Some(prev_read.unwrap_or(std::STDIN)),
        };
        let stdout = match stage.output {
            Some(name) => {
                let mode = if stage.append { std::O_APPEND } else { std::O_TRUNC };
                open_or_report(name, std::O_WRITE | std::O_CREATE | mode)
            }
            None => Some(pipe_write.unwrap_or(std::STDOUT)),
        };
        if let (Some(stdin), Some(stdout)) = (stdin, stdout) {
            tasks[i] = std::spawn(program, args, [stdin, stdout, std::STDERR]);
            if tasks[i] == usize::MAX {
                print("Error: failed to execute ");
                println(program);
            }
        }

        // The child holds its own copies; closing ours lets EOF and broken
        // pipes reach the neighbouring stages.
        close_owned(stdin);
        if prev_read != stdin {
            close_owned(prev_read);
        }
        close_owned(stdout);
        if pipe_write != stdout {
            close_owned(pipe_write);
        }
        prev_read = pipe.map(|(read_fd, _)| read_fd);
    }

    for &task in &tasks[..count] {
        if task != usize::MAX {
            std::wait_task(task);
        }
    }
}

// ── Entry point ────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn _start(_args_ptr: *const u8, _args_len: usize) -> ! {
    println("");
    println("  ============================");
    println("    kaguyaOS v0.1.0");
    println("    Type 'help' for commands");
    println("  ============================");
    println("");

    let mut cmd_buf = [0u8; MAX_CMD_LEN];
    let mut cmd_len: usize;

    loop {
        std::print("kaguya> ");
        cmd_len = 0;

        loop {
            let mut key = [0u8; 1];
            if std::read(std::STDIN, &mut key) != 1 {
                continue;
            }
            let key = key[0];

            if key == KEY_ENTER {
                std::print("\n");
                break;
            }

            if key == KEY_BACKSPACE {
                if cmd_len > 0 {
                    cmd_len -= 1;
                    std::print("\x08 \x08");
                }
                continue;
            }

            if key >= 0x20 && key < 0x7F && cmd_len < MAX_CMD_LEN - 1 {
                cmd_buf[cmd_len] = key;
                cmd_len += 1;
                let ch = [key];
                unsafe {
                    std::print(core::str::from_utf8_unchecked(&ch));
                }
            }
        }

        if cmd_len > 0 {
            // Only printable ASCII is stored, so the line is valid UTF-8.
            process_command(unsafe { core::str::from_utf8_unchecked(&cmd_buf[..cmd_len]) });
        }
    }
}
