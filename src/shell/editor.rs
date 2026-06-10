/// Multiline text editor for the shell's `fswrite` command.
///
/// This is a simple vi-like editor with viewport scrolling, cursor movement,
/// and an ESC-menu to save or discard changes.

use alloc::string::String;
use alloc::vec::Vec;
use crate::std::stdio::{print, KEY_BACKSPACE, KEY_DOWN, KEY_ENTER, KEY_LEFT, KEY_RIGHT, KEY_UP};
use crate::std::syscall;
use super::fs_utils::fs_read_file;

const KEY_ESC: u8 = 0x1B;
const MAX_LINE_LENGTH: usize = 1024;
const MAX_DISPLAY_LINES: usize = 20;

/// Open the multiline editor for `filename`.
///
/// If the file already exists its contents are loaded; otherwise the editor
/// starts with a single empty line. The user can navigate, edit, and
/// ultimately save or discard via the ESC menu.
pub fn run(filename: &str) {
    let mut lines = load_initial_content(filename);

    let mut cur_line: usize = 0;
    let mut cursor: usize = 0;
    let mut viewport_top: usize = 0;

    loop {
        viewport_top = adjust_viewport(viewport_top, cur_line);
        draw_editor(filename, &lines, cur_line, cursor, viewport_top);

        match poll_key() {
            KEY_UP => {
                if cur_line > 0 {
                    cur_line -= 1;
                    cursor = cursor.min(lines[cur_line].len());
                }
            }
            KEY_DOWN => {
                if cur_line < lines.len() - 1 {
                    cur_line += 1;
                    cursor = cursor.min(lines[cur_line].len());
                }
            }
            KEY_LEFT => {
                if cursor > 0 {
                    cursor -= 1;
                } else if cur_line > 0 {
                    cur_line -= 1;
                    cursor = lines[cur_line].len();
                }
            }
            KEY_RIGHT => {
                if cursor < lines[cur_line].len() {
                    cursor += 1;
                } else if cur_line < lines.len() - 1 {
                    cur_line += 1;
                    cursor = 0;
                }
            }
            KEY_ENTER | 0x0D => {
                let safe = cursor.min(lines[cur_line].len());
                let right = lines[cur_line][safe..].to_vec();
                lines[cur_line].truncate(safe);
                lines.insert(cur_line + 1, right);
                cur_line += 1;
                cursor = 0;
            }
            KEY_BACKSPACE => {
                if cursor > 0 {
                    lines[cur_line].remove(cursor - 1);
                    cursor -= 1;
                } else if cur_line > 0 {
                    let removed = lines.remove(cur_line);
                    cur_line -= 1;
                    let prev_len = lines[cur_line].len();
                    lines[cur_line].extend(removed);
                    cursor = prev_len;
                }
            }
            KEY_ESC => {
                match show_save_menu(filename, &lines) {
                    MenuChoice::SaveAndExit | MenuChoice::DiscardAndExit => return,
                    MenuChoice::Resume => { /* redraw */ }
                }
            }
            key @ 0x20..=0x7E => {
                if lines[cur_line].len() < MAX_LINE_LENGTH {
                    let safe = cursor.min(lines[cur_line].len());
                    lines[cur_line].insert(safe, key as char);
                    cursor += 1;
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Load existing file content, or return a single empty line.
fn load_initial_content(filename: &str) -> Vec<Vec<char>> {
    if let Ok(data) = fs_read_file(filename) {
        if !data.is_empty() {
            if let Ok(s) = core::str::from_utf8(&data) {
                let mut lines: Vec<Vec<char>> = s.lines().map(|l| l.chars().collect()).collect();
                if lines.is_empty() {
                    lines.push(Vec::new());
                }
                return lines;
            }
        }
    }
    alloc::vec![Vec::new()]
}

/// Adjust the viewport so that `cur_line` is visible.
fn adjust_viewport(mut top: usize, cur_line: usize) -> usize {
    if cur_line < top {
        top = cur_line;
    } else if cur_line >= top + MAX_DISPLAY_LINES {
        top = cur_line - MAX_DISPLAY_LINES + 1;
    }
    top
}

/// Clear screen and redraw the full editor UI.
fn draw_editor(
    filename: &str,
    lines: &[Vec<char>],
    cur_line: usize,
    cursor: usize,
    viewport_top: usize,
) {
    clear_screen();

    // Header
    print(&alloc::format!(
        "=== KaguyaOS Text Editor === File: {} ===\n\
         Commands: [ESC] Save/Discard Menu | [Arrows] Navigate | [Enter] Insert Line\n\
         --------------------------------------------------------------------------------\n",
        filename
    ));

    // Visible lines
    let display_end = (viewport_top + MAX_DISPLAY_LINES).min(lines.len());
    for i in viewport_top..display_end {
        if i == cur_line {
            let safe = cursor.min(lines[i].len());
            let left: String = lines[i][..safe].iter().collect();
            let right: String = lines[i][safe..].iter().collect();
            print(&alloc::format!("{:3}> {}_{}\n", i + 1, left, right));
        } else {
            let text: String = lines[i].iter().collect();
            print(&alloc::format!("{:3}: {}\n", i + 1, text));
        }
    }

    // Fill remaining rows with '~'
    for _ in (display_end - viewport_top)..MAX_DISPLAY_LINES {
        print("~\n");
    }

    // Footer / status bar
    print("--------------------------------------------------------------------------------\n");
    print(&alloc::format!(
        "Line {}/{} | Col {} | Size: {} lines\n",
        cur_line + 1,
        lines.len(),
        cursor + 1,
        lines.len()
    ));
}

/// Block until a key is pressed and return the key code.
fn poll_key() -> u8 {
    loop {
        unsafe { syscall(9, 0, 0, 0, 0, 0, 0) };
        let val = unsafe { syscall(11, 0, 0, 0, 0, 0, 0) };
        if val != 0 {
            return val as u8;
        }
        // Yield to avoid 100% CPU
        unsafe { syscall(5, 0, 0, 0, 0, 0, 0) };
    }
}

fn clear_screen() {
    unsafe { syscall(12, 0, 0, 0, 0, 0, 0) };
}

// ---------------------------------------------------------------------------
// Save / discard menu
// ---------------------------------------------------------------------------

enum MenuChoice {
    SaveAndExit,
    DiscardAndExit,
    Resume,
}

fn show_save_menu(filename: &str, lines: &[Vec<char>]) -> MenuChoice {
    clear_screen();
    print("\n=== Save Changes? ===\n\n");
    print(&alloc::format!("File: {}\n\n", filename));
    print("  [Y] Save and Exit\n");
    print("  [N] Discard Changes and Exit\n");
    print("  [Esc/C] Cancel and Resume Editing\n\n");
    print("Choice: ");

    loop {
        let key = poll_key();
        match key {
            b'y' | b'Y' => {
                save_file(filename, lines);
                return MenuChoice::SaveAndExit;
            }
            b'n' | b'N' => {
                print("\nChanges discarded.\n");
                return MenuChoice::DiscardAndExit;
            }
            b'c' | b'C' | KEY_ESC => {
                return MenuChoice::Resume;
            }
            _ => {}
        }
    }
}

fn save_file(filename: &str, lines: &[Vec<char>]) {
    let mut combined = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            combined.push('\n');
        }
        for &c in line {
            combined.push(c);
        }
    }
    match crate::std::fs_write(filename, combined.as_bytes()) {
        Ok(_) => print("\nFile saved successfully.\n"),
        Err(e) => print(&alloc::format!("\nError writing file: {}\n", e)),
    }
}
