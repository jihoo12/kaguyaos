/// KaguyaOS interactive shell.
///
/// The shell is split across several submodules for clarity:
///
/// - [`commands`] — Individual command handlers (one method per command).
/// - [`compiler`] — Source compilation, assembly, and process execution.
/// - [`editor`]   — Multiline text editor for `fswrite`.
/// - [`fs_utils`] — Shared filesystem helpers.

mod commands;
mod compiler;
mod editor;
mod fs_utils;

use crate::std::stdio::{input, print};

const MAX_CMD_LEN: usize = 64;
const HISTORY_SIZE: usize = 10;

// ---------------------------------------------------------------------------
// Shell struct & core loop
// ---------------------------------------------------------------------------

struct Shell {
    history: [[u8; MAX_CMD_LEN]; HISTORY_SIZE],
    history_len: [usize; HISTORY_SIZE],
    history_count: usize,
    history_start: usize,
}

impl Shell {
    fn new() -> Self {
        Self {
            history: [[0; MAX_CMD_LEN]; HISTORY_SIZE],
            history_len: [0; HISTORY_SIZE],
            history_count: 0,
            history_start: 0,
        }
    }

    fn run(&mut self) {
        print("\n=== Interactive Shell (Fixed Buffer) ===\n");
        print("Type 'help' for commands. Use Arrow Keys for history.\n");

        loop {
            print("kaguya> ");
            let line = input();
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            self.add_history(line.as_bytes());
            self.eval(line);
        }
    }

    // -- History ------------------------------------------------------------

    fn add_history(&mut self, cmd: &[u8]) {
        let idx = (self.history_start + self.history_count) % HISTORY_SIZE;
        let len = cmd.len().min(MAX_CMD_LEN);
        self.history[idx][..len].copy_from_slice(&cmd[..len]);
        self.history_len[idx] = len;

        if self.history_count < HISTORY_SIZE {
            self.history_count += 1;
        } else {
            self.history_start = (self.history_start + 1) % HISTORY_SIZE;
        }
    }

    fn get_history(&self, offset_from_newest: usize) -> Option<&[u8]> {
        if offset_from_newest >= self.history_count {
            return None;
        }
        let end_idx = self.history_start + self.history_count;
        let target_idx = (end_idx - 1 - offset_from_newest) % HISTORY_SIZE;
        Some(&self.history[target_idx][..self.history_len[target_idx]])
    }

    // -- Command dispatch ---------------------------------------------------

    fn eval(&self, line: &str) {
        let mut parts = line.split_whitespace();
        let cmd = match parts.next() {
            Some(c) => c,
            None => return,
        };

        match cmd {
            "help"     => self.cmd_help(),
            "echo"     => self.cmd_echo(parts),
            "history"  => self.cmd_history(),
            "shutdown" => self.cmd_shutdown(),
            "clear"    => self.cmd_clear(),
            "fsformat" => self.cmd_fsformat(),
            "fsls"     => self.cmd_fsls(),
            "fswrite"  => self.cmd_fswrite(parts),
            "fsread"   => self.cmd_fsread(parts),
            "fsrm"     => self.cmd_fsrm(parts),
            "compile"  => self.cmd_compile(parts),
            "load"     => self.cmd_load(parts),
            _ => {
                print("Unknown command: ");
                print(cmd);
                print(". Type 'help' for available commands.\n");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn shell() {
    let mut shell = Shell::new();
    shell.run();
}
