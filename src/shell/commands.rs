/// Individual command handlers for the shell.
///
/// Each public method corresponds to one shell command. This keeps the
/// main `eval` dispatch in `mod.rs` clean and allows each command's
/// logic to be read and modified independently.

use crate::std::stdio::print;
use crate::std::syscall;
use super::Shell;
use super::compiler;
use super::editor;
use super::fs_utils::fs_read_file;

impl Shell {
    pub(super) fn cmd_help(&self) {
        print("Commands: help, echo, history, clear, shutdown, compile, load, fsformat, fsls, fswrite, fsread, fsrm\n");
        print("  help                 - show this help menu\n");
        print("  echo [args...]       - print the arguments to the screen\n");
        print("  history              - show the command history\n");
        print("  clear                - clear the screen\n");
        print("  shutdown             - shut down the machine\n");
        print("  compile <src> <dest> - compile/assemble a source file (.c/.asm/.s) to machine code\n");
        print("  load <file...>       - load one or more files and run them as processes (compiles .c/.asm/.s, runs others as raw machine code)\n");
        print("  fsformat             - format the NVMe drive with SimpleFS\n");
        print("  fsls                 - list files in the filesystem\n");
        print("  fswrite <file> <msg> - write a file with text message (inline)\n");
        print("  fswrite <file>       - write a file in multi-line mode\n");
        print("  fsread <file>        - read and display a file's contents\n");
        print("  fsrm <file>          - delete a file from the filesystem\n");
    }

    pub(super) fn cmd_echo(&self, parts: core::str::SplitWhitespace<'_>) {
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

    pub(super) fn cmd_history(&self) {
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
                    Err(_) => print("<invalid utf8>\n"),
                }
            }
        }
    }

    pub(super) fn cmd_shutdown(&self) {
        print("Bye!\n");
        unsafe { syscall(10, 0, 0, 0, 0, 0, 0) };
    }

    pub(super) fn cmd_clear(&self) {
        unsafe { syscall(12, 0, 0, 0, 0, 0, 0) };
    }

    pub(super) fn cmd_fsformat(&self) {
        match crate::std::fs_format() {
            Ok(_) => print("Filesystem formatted successfully.\n"),
            Err(e) => print(&alloc::format!("Error formatting filesystem: {}\n", e)),
        }
    }

    pub(super) fn cmd_fsls(&self) {
        let mut buf = [crate::std::SyscallFileEntry {
            name: [0; 47],
            name_len: 0,
            size: 0,
            start_block: 0,
        }; 128];

        match crate::std::fs_list_files(&mut buf) {
            Ok(0) => print("No files found.\n"),
            Ok(count) => {
                print("Name                           Size (Bytes)   Start Block\n");
                print("---------------------------------------------------------\n");
                for entry in &buf[..count] {
                    let name = alloc::string::String::from_utf8_lossy(
                        &entry.name[..entry.name_len as usize],
                    );
                    print(&alloc::format!(
                        "{:<30} {:<14} {}\n",
                        name, entry.size, entry.start_block
                    ));
                }
            }
            Err(e) => print(&alloc::format!("Error listing files: {}\n", e)),
        }
    }

    pub(super) fn cmd_fswrite(&self, mut parts: core::str::SplitWhitespace<'_>) {
        let filename = match parts.next() {
            Some(name) if !name.is_empty() => name,
            _ => {
                print("Usage:\n");
                print("  fswrite <filename> <content> - write content inline\n");
                print("  fswrite <filename>           - enter multi-line mode\n");
                return;
            }
        };

        // Collect remaining arguments as inline content.
        let content: alloc::vec::Vec<u8> = {
            let mut buf = alloc::vec::Vec::new();
            for part in parts {
                if !buf.is_empty() {
                    buf.push(b' ');
                }
                buf.extend_from_slice(part.as_bytes());
            }
            buf
        };

        if content.is_empty() {
            editor::run(filename);
        } else {
            match crate::std::fs_write(filename, &content) {
                Ok(_) => print("File written successfully.\n"),
                Err(e) => print(&alloc::format!("Error writing file: {}\n", e)),
            }
        }
    }

    pub(super) fn cmd_fsread(&self, mut parts: core::str::SplitWhitespace<'_>) {
        let filename = match parts.next() {
            Some(f) => f,
            None => {
                print("Usage: fsread <filename>\n");
                return;
            }
        };

        match fs_read_file(filename) {
            Ok(data) => match core::str::from_utf8(&data) {
                Ok(s) => {
                    print(s);
                    print("\n");
                }
                Err(_) => print(&alloc::format!("<binary data, {} bytes>\n", data.len())),
            },
            Err(e) => print(&alloc::format!("{}\n", e)),
        }
    }

    pub(super) fn cmd_fsrm(&self, mut parts: core::str::SplitWhitespace<'_>) {
        let filename = match parts.next() {
            Some(f) => f,
            None => {
                print("Usage: fsrm <filename>\n");
                return;
            }
        };

        match crate::std::fs_rm(filename) {
            Ok(_) => print("File deleted successfully.\n"),
            Err(e) => print(&alloc::format!("Error deleting file: {}\n", e)),
        }
    }

    pub(super) fn cmd_compile(&self, mut parts: core::str::SplitWhitespace<'_>) {
        let src_file = parts.next().unwrap_or("");
        let dest_file = parts.next().unwrap_or("");

        if src_file.is_empty() || dest_file.is_empty() {
            print("Usage: compile <src_file> <dest_file>\n");
            return;
        }

        let data = match fs_read_file(src_file) {
            Ok(d) => d,
            Err(e) => {
                print(&alloc::format!("{}\n", e));
                return;
            }
        };

        match compiler::compile_by_extension(src_file, &data) {
            Ok(code) => match crate::std::fs_write(dest_file, &code) {
                Ok(_) => print(&alloc::format!(
                    "Compiled {} successfully to {}\n",
                    src_file, dest_file
                )),
                Err(e) => print(&alloc::format!("Error writing compiled output: {}\n", e)),
            },
            Err(e) => print(&alloc::format!("Compile/Assemble error: {}\n", e)),
        }
    }

    pub(super) fn cmd_load(&self, parts: core::str::SplitWhitespace<'_>) {
        let files: alloc::vec::Vec<&str> = parts.collect();
        if files.is_empty() {
            print("Usage: load <file1> [file2...]\n");
            return;
        }

        for filename in files {
            if let Err(e) = compiler::load_and_run(filename) {
                print(&alloc::format!("{}\n", e));
            }
        }
    }
}
