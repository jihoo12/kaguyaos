use crate::std::stdio::{input, print};
use crate::std::syscall;
use alloc::string::String;

const MAX_CMD_LEN: usize = 64;
const HISTORY_SIZE: usize = 10;

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

            // Check if this is the start of a multi-line asm block
            if line == "asm" || line.starts_with("asm ") || line.starts_with("asm\t") {
                self.add_history(line.as_bytes());

                // Inline asm (single line): asm <instruction>
                let rest = line["asm".len()..].trim();
                if !rest.is_empty() {
                    self.eval_asm(rest);
                } else {
                    // Multi-line asm mode
                    self.run_multiline_asm();
                }
            } else if line == "c" || line.starts_with("c ") || line.starts_with("c\t") {
                self.add_history(line.as_bytes());

                let rest = line["c".len()..].trim();
                if !rest.is_empty() {
                    self.eval_c(rest);
                } else {
                    self.run_multiline_c();
                }
            } else {
                self.add_history(line.as_bytes());
                self.eval(line);
            }
        }
    }

    fn run_multiline_asm(&mut self) {
        print("Entering multi-line asm mode. Type instructions line by line.\n");
        print("Type 'done' on its own line to assemble and run.\n");
        print("Type 'cancel' to abort.\n");

        let mut lines: alloc::vec::Vec<String> = alloc::vec::Vec::new();

        loop {
            print("asm> ");
            let line = input();
            let line = line.trim();

            match line {
                "done" => {
                    if lines.is_empty() {
                        print("No instructions entered.\n");
                    } else {
                        let combined = lines.join(";");
                        self.eval_asm(&combined);
                    }
                    break;
                }
                "cancel" => {
                    print("Asm cancelled.\n");
                    break;
                }
                "" => {
                    // Skip blank lines
                }
                _ => {
                    lines.push(String::from(line));
                }
            }
        }
    }

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

    fn eval(&self, line: &str) {
        let mut parts = line.split_whitespace();
        if let Some(cmd) = parts.next() {
            match cmd {
                "help" => {
                    print("Commands: help, echo, history, clear, shutdown, asm, c, load, fsformat, fsls, fswrite, fsread, fsrm\n");
                    print("  help                 - show this help menu\n");
                    print("  echo [args...]       - print the arguments to the screen\n");
                    print("  history              - show the command history\n");
                    print("  clear                - clear the screen\n");
                    print("  shutdown             - shut down the machine\n");
                    print("  asm <instr>          - assemble and run a single instruction\n");
                    print("  asm                  - enter multi-line asm mode\n");
                    print("  Use ';' to separate multiple instructions inline\n");
                    print("  c <code>             - JIT-compile and run a tiny C function\n");
                    print("  c                    - enter multi-line C mode\n");
                    print("  load <file>          - load a file and run it using cc (.c) or tinyasm (.asm/.s)\n");
                    print("  fsformat             - format the NVMe drive with SimpleFS\n");
                    print("  fsls                 - list files in the filesystem\n");
                    print("  fswrite <file> <msg> - write a file with text message (inline)\n");
                    print("  fswrite <file>       - write a file in multi-line mode\n");
                    print("  fsread <file>        - read and display a file's contents\n");
                    print("  fsrm <file>          - delete a file from the filesystem\n");
                    print("  Example: c uint64_t f() { return 42; }\n");
                }
                "echo" => {
                    let mut first = true;
                    for arg in parts {
                        if !first {
                            print(" ");
                        }
                        print(arg);
                        first = false;
                    }
                    print("\n");
                }
                "history" => {
                    if self.history_count == 0 {
                        print("No history.\n");
                        return;
                    }
                    for i in 0..self.history_count {
                        if let Some(h) = self.get_history(self.history_count - 1 - i) {
                            match core::str::from_utf8(h) {
                                Ok(s) => {
                                    print(s);
                                    print("\n");
                                }
                                Err(_) => {
                                    print("<invalid utf8>\n");
                                }
                            }
                        }
                    }
                }
                "shutdown" => {
                    print("Bye!\n");
                    unsafe { syscall(10, 0, 0, 0, 0, 0, 0) };
                }
                "clear" => {
                    unsafe { syscall(12, 0, 0, 0, 0, 0, 0) };
                }
                "fsformat" => {
                    match crate::std::fs_format() {
                        Ok(_) => print("Filesystem formatted successfully.\n"),
                        Err(e) => {
                            let msg = alloc::format!("Error formatting filesystem: {}\n", e);
                            print(&msg);
                        }
                    }
                }
                "fsls" => {
                    let mut buf = [crate::std::SyscallFileEntry {
                        name: [0; 47],
                        name_len: 0,
                        size: 0,
                        start_block: 0,
                    }; 128];
                    match crate::std::fs_list_files(&mut buf) {
                        Ok(count) => {
                            if count == 0 {
                                print("No files found.\n");
                            } else {
                                print("Name                           Size (Bytes)   Start Block\n");
                                print("---------------------------------------------------------\n");
                                for i in 0..count {
                                    let entry = &buf[i];
                                    let name_str = alloc::string::String::from_utf8_lossy(&entry.name[..entry.name_len as usize]).into_owned();
                                    let msg = alloc::format!("{:<30} {:<14} {}\n", name_str, entry.size, entry.start_block);
                                    print(&msg);
                                }
                            }
                        }
                        Err(e) => {
                            let msg = alloc::format!("Error listing files: {}\n", e);
                            print(&msg);
                        }
                    }
                }
                "fswrite" => {
                    let mut filename = "";
                    if let Some(name) = parts.next() {
                        filename = name;
                    }
                    if filename.is_empty() {
                        print("Usage:\n");
                        print("  fswrite <filename> <content> - write content inline\n");
                        print("  fswrite <filename>           - enter multi-line mode\n");
                    } else {
                        // Gather remaining parts as content
                        let mut content = alloc::vec::Vec::new();
                        for part in parts {
                            if !content.is_empty() {
                                content.push(b' ');
                            }
                            content.extend_from_slice(part.as_bytes());
                        }
                        if content.is_empty() {
                            self.run_multiline_write(filename);
                        } else {
                            match crate::std::fs_write(filename, &content) {
                                Ok(_) => print("File written successfully.\n"),
                                Err(e) => {
                                    let msg = alloc::format!("Error writing file: {}\n", e);
                                    print(&msg);
                                }
                            }
                        }
                    }
                }
                "fsread" => {
                    if let Some(filename) = parts.next() {
                        let mut size_buf = [];
                        match crate::std::fs_read(filename, &mut size_buf) {
                            Ok(size) => {
                                let mut data = alloc::vec![0u8; size];
                                match crate::std::fs_read(filename, &mut data) {
                                    Ok(_) => {
                                        match core::str::from_utf8(&data) {
                                            Ok(s) => {
                                                print(s);
                                                print("\n");
                                            }
                                            Err(_) => {
                                                let msg = alloc::format!("<binary data, {} bytes>\n", data.len());
                                                print(&msg);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let msg = alloc::format!("Error reading file: {}\n", e);
                                        print(&msg);
                                    }
                                }
                            }
                            Err(e) => {
                                let msg = alloc::format!("Error reading file: {}\n", e);
                                print(&msg);
                            }
                        }
                    } else {
                        print("Usage: fsread <filename>\n");
                    }
                }
                "fsrm" => {
                    if let Some(filename) = parts.next() {
                        match crate::std::fs_rm(filename) {
                            Ok(_) => print("File deleted successfully.\n"),
                            Err(e) => {
                                let msg = alloc::format!("Error deleting file: {}\n", e);
                                print(&msg);
                            }
                        }
                    } else {
                        print("Usage: fsrm <filename>\n");
                    }
                }
                "load" => {
                    if let Some(filename) = parts.next() {
                        let mut size_buf = [];
                        match crate::std::fs_read(filename, &mut size_buf) {
                            Ok(size) => {
                                let mut data = alloc::vec![0u8; size];
                                match crate::std::fs_read(filename, &mut data) {
                                    Ok(_) => {
                                        match core::str::from_utf8(&data) {
                                            Ok(content_str) => {
                                                if filename.ends_with(".c") || filename.ends_with(".C") {
                                                    self.eval_c(content_str);
                                                } else if filename.ends_with(".asm") || filename.ends_with(".s") || filename.ends_with(".ASM") || filename.ends_with(".S") {
                                                    self.eval_asm(content_str);
                                                } else {
                                                    print("Error: Unsupported file extension. Only .c, .asm, and .s files are supported.\n");
                                                }
                                            }
                                            Err(_) => {
                                                print("Error: File content is not valid UTF-8.\n");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let msg = alloc::format!("Error reading file: {}\n", e);
                                        print(&msg);
                                    }
                                }
                            }
                            Err(e) => {
                                let msg = alloc::format!("Error reading file: {}\n", e);
                                print(&msg);
                            }
                        }
                    } else {
                        print("Usage: load <filename>\n");
                    }
                }
                _ => {
                    print("Unknown command: ");
                    print(cmd);
                    print(". Type 'help' for available commands.\n");
                }
            }
        }
    }

    fn run_multiline_c(&mut self) {
        print("Entering multi-line C mode. Type your function line by line.\n");
        print("Type 'done' on its own line to compile and run.\n");
        print("Type 'cancel' to abort.\n");

        let mut lines: alloc::vec::Vec<String> = alloc::vec::Vec::new();

        loop {
            print("c> ");
            let line = input();
            let line = line.trim();

            match line {
                "done" => {
                    if lines.is_empty() {
                        print("No code entered.\n");
                    } else {
                        let combined = lines.join(" ");
                        self.eval_c(&combined);
                    }
                    break;
                }
                "cancel" => {
                    print("C cancelled.\n");
                    break;
                }
                "" => {}
                _ => {
                    lines.push(String::from(line));
                }
            }
        }
    }

    fn eval_c(&self, src: &str) {
        use crate::cc::compile_and_run;

        match compile_and_run(src) {
            Ok(result) => {
                let msg = alloc::format!("Result: {}\n", result);
                print(&msg);
            }
            Err(e) => {
                let msg = alloc::format!("C JIT error: {}\n", e);
                print(&msg);
            }
        }
    }

    fn eval_asm(&self, asm_str: &str) {
        use crate::tinyasm::encoder::assemble;
        use crate::tinyasm::jit::JitMemory;
        use crate::tinyasm::parser::parse_asm_line;

        let lines: alloc::vec::Vec<_> = asm_str
            .split(|c| c == ';' || c == '\n')
            .filter_map(|part| parse_asm_line(part.trim()))
            .collect();

        if lines.is_empty() {
            print("No valid instructions found.\n");
            return;
        }

        let machine_code = match assemble(&lines) {
            Ok(code) => code,
            Err(e) => {
                let msg = alloc::format!("Encoding error: {}\n", e);
                print(&msg);
                return;
            }
        };

        match JitMemory::new(4096) {
            Ok(mut jit) => {
                if jit.write(&machine_code).is_err() {
                    print("JIT write error.\n");
                    return;
                }
                if jit.make_executable().is_err() {
                    print("JIT make-executable error.\n");
                    return;
                }
                let result = unsafe { jit.as_fn_u64()() };
                let msg = alloc::format!("Result: {}\n", result);
                print(&msg);
            }
            Err(_) => {
                print("JIT allocation error.\n");
            }
        }
    }

    fn run_multiline_write(&self, filename: &str) {
        print("Entering multi-line write mode. Type your text line by line.\n");
        print("Type 'done' on its own line to write to file.\n");
        print("Type 'cancel' to abort.\n");

        let mut lines: alloc::vec::Vec<String> = alloc::vec::Vec::new();

        loop {
            print("write> ");
            let line = input();
            let line = line.trim();

            match line {
                "done" => {
                    let combined = lines.join("\n");
                    match crate::std::fs_write(filename, combined.as_bytes()) {
                        Ok(_) => print("File written successfully.\n"),
                        Err(e) => {
                            let msg = alloc::format!("Error writing file: {}\n", e);
                            print(&msg);
                        }
                    }
                    break;
                }
                "cancel" => {
                    print("Write cancelled.\n");
                    break;
                }
                _ => {
                    lines.push(String::from(line));
                }
            }
        }
    }
}

pub fn shell() {
    let mut shell = Shell::new();
    shell.run();
}
