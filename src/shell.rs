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

            self.add_history(line.as_bytes());
            self.eval(line);
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
                    print("Commands: help, echo, history, clear, shutdown, load, fsformat, fsls, fswrite, fsread, fsrm\n");
                    print("  help                 - show this help menu\n");
                    print("  echo [args...]       - print the arguments to the screen\n");
                    print("  history              - show the command history\n");
                    print("  clear                - clear the screen\n");
                    print("  shutdown             - shut down the machine\n");
                    print("  load <file>          - load a file and run it as a process using cc (.c) or tinyasm (.asm/.s)\n");
                    print("  fsformat             - format the NVMe drive with SimpleFS\n");
                    print("  fsls                 - list files in the filesystem\n");
                    print("  fswrite <file> <msg> - write a file with text message (inline)\n");
                    print("  fswrite <file>       - write a file in multi-line mode\n");
                    print("  fsread <file>        - read and display a file's contents\n");
                    print("  fsrm <file>          - delete a file from the filesystem\n");
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
                                                let compile_res = if filename.ends_with(".c") || filename.ends_with(".C") {
                                                    compile_c_to_bytes(content_str)
                                                } else if filename.ends_with(".asm") || filename.ends_with(".s") || filename.ends_with(".ASM") || filename.ends_with(".S") {
                                                    assemble_to_bytes(content_str)
                                                } else {
                                                    Err(alloc::string::String::from("Unsupported file extension. Only .c, .asm, and .s files are supported."))
                                                };

                                                match compile_res {
                                                    Ok(code) => {
                                                        match run_as_process(&code) {
                                                            Ok(_) => {}
                                                            Err(e) => {
                                                                let msg = alloc::format!("Process run error: {}\n", e);
                                                                print(&msg);
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        let msg = alloc::format!("Compile/Assemble error: {}\n", e);
                                                        print(&msg);
                                                    }
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

fn compile_c_to_bytes(src: &str) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
    let tokens = crate::cc::lexer::lex(src)?;
    let functions = crate::cc::parser::parse_functions(&tokens)?;
    let code = crate::cc::codegen::compile_program(&functions)?;
    Ok(code)
}

fn assemble_to_bytes(asm_str: &str) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
    use crate::tinyasm::encoder::assemble;
    use crate::tinyasm::parser::parse_asm_line;

    let lines: alloc::vec::Vec<_> = asm_str
        .split(|c| c == ';' || c == '\n')
        .filter_map(|part| parse_asm_line(part.trim()))
        .collect();

    if lines.is_empty() {
        return Err(alloc::string::String::from("No valid instructions found."));
    }

    assemble(&lines).map_err(|e| alloc::format!("Encoding error: {}", e))
}

#[unsafe(naked)]
extern "sysv64" fn user_exit_trampoline() -> ! {
    unsafe {
        core::arch::naked_asm!(
            "mov rdi, rax",
            "jmp {exit_process_fn}",
            exit_process_fn = sym crate::std::exit_process
        );
    }
}

fn run_as_process(code: &[u8]) -> Result<(), alloc::string::String> {
    use crate::tinyasm::jit::JitMemory;
    use crate::std::{spawn_process, yield_process, get_process_status, get_process_exit_code, sys_alloc, sys_free};

    let mut jit = JitMemory::new(code.len())?;
    jit.write(code)?;
    jit.make_executable()?;

    let stack_size = 16384;
    let stack_bottom = unsafe { sys_alloc(stack_size, 16) };
    if stack_bottom.is_null() {
        return Err(alloc::string::String::from("Failed to allocate process stack"));
    }

    let stack_top = unsafe { stack_bottom.add(stack_size) };
    let initial_rsp = unsafe { stack_top.sub(8) };
    unsafe {
        *(initial_rsp as *mut u64) = user_exit_trampoline as *const () as u64;
    }

    let entry_point = unsafe { jit.as_fn_u64() as *const () as u64 };

    let pid = spawn_process(entry_point as usize, initial_rsp as usize);
    let msg = alloc::format!("Spawned process PID={}\n", pid);
    print(&msg);

    loop {
        yield_process();
        let status = get_process_status(pid);
        if status == 2 { // Terminated
            break;
        } else if status == 3 { // Not found
            break;
        }
    }

    let exit_code = get_process_exit_code(pid);
    let msg = alloc::format!("Process finished with exit code: {}\n", exit_code);
    print(&msg);

    unsafe {
        sys_free(stack_bottom);
    }

    Ok(())
}

pub fn shell() {
    let mut shell = Shell::new();
    shell.run();
}
