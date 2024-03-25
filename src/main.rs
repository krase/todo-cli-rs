#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]

use std::fs::File;
use std::io::{self, stderr, stdout, BufRead, Write};
use std::ops::{BitXor, BitXorAssign};
use std::time::{Duration, SystemTime};
use std::{env, process, thread};

use anyhow::Result;
use crossterm::cursor::{DisableBlinking, Hide, MoveTo, SetCursorStyle, Show};
use crossterm::event::{poll, read, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::style::{Color, Print, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue, ExecutableCommand, QueueableCommand};
use screen_buf::{apply_patches, Buffer, VirtualScreen};
use unicode_segmentation::UnicodeSegmentation;

mod ui;
mod screen_buf;

use ui::{Layout, LayoutKind, Ui, Vec2};

type Item = String;

struct ScreenState;

impl ScreenState {
    fn enable() -> io::Result<Self> {
        execute!(
            stdout(),
            EnterAlternateScreen,
            SetCursorStyle::SteadyBlock,
            Hide
        )?;
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for ScreenState {
    fn drop(&mut self) {
        let _ =
            terminal::disable_raw_mode().map_err(|err| eprintln!("ERROR: disable raw mode: {err}"));
        let _ = execute!(stdout(), LeaveAlternateScreen)
            .map_err(|err| eprintln!("ERROR: leave alternate screen: {err}"));
    }
}

#[derive(PartialEq, Default, Debug, Clone, Copy)]
#[repr(usize)]
enum Status {
    #[default]
    Todo = 0,
    Done = 1,
}

impl BitXorAssign<usize> for Status {
    fn bitxor_assign(&mut self, _: usize) {
        if *self == Status::Todo {
            *self = Status::Done
        } else {
            *self = Status::Todo
        }
    }
}

impl BitXor<usize> for Status {
    type Output = Status;
    fn bitxor(self, _: usize) -> Status {
        if self == Status::Todo {
            Status::Done
        } else {
            Status::Todo
        }
    }
}

#[derive(Default)]
struct App {
    quit: bool,
    //w: u16,
    //h: u16,
    active_status: Status,
    edit_mode: bool,
    edit_cursor: usize,
    // at start it is list.len()
    lists: [ItemList; 2],
}

impl App {
    fn new() -> Self {
        Self::default()
    }

    fn cursor_up(&mut self) {
        self.active_list_mut().cursor_up();
    }
    fn cursor_down(&mut self) {
        self.active_list_mut().cursor_down();
    }
    fn cursor_to_top(&mut self) {
        self.active_list_mut().cursor_to_top();
    }
    fn cursor_to_bottom(&mut self) {
        self.active_list_mut().cursor_to_bottom()
    }
    fn drag_up(&mut self) {
        self.active_list_mut().list_drag_up();
    }
    fn drag_down(&mut self) {
        self.active_list_mut().list_drag_down();
    }

    fn edit_add_char(&mut self, c: char) {
        let cursor = self.active_cursor();
        let edit_cursor = self.edit_cursor;
        self.active_items_mut()[cursor].push(c);
        //let tmp = tmp.chars() + c;
        //tmp.
        //self.active_items_mut()[cursor].insert(edit_cursor, c);
        self.edit_cursor_right();
    }

    fn backspace(&mut self) {
        let cursor = self.active_cursor();
        let edit_cursor = self.edit_cursor;
        let mut chars = self.active_items()[cursor].chars();
        chars.next_back();
        
        self.active_items_mut()[cursor] = chars.as_str().to_owned();

        //let len = UnicodeSegmentation::graphemes(tmp, true).count();
        self.edit_cursor_left();    
    }

    fn edit_cursor_left(&mut self) {
        if self.edit_cursor > 0 {
            self.edit_cursor -= 1;
        }
    }
    fn edit_cursor_right(&mut self) {
        let cursor = self.active_cursor();
        let tmp = self.active_items()[cursor].as_str();
        let len = UnicodeSegmentation::graphemes(tmp, true).count();
        if self.edit_cursor < len {
            self.edit_cursor += 1;
        }
    }

    fn edit_cursor_begin(&mut self) {
        self.edit_cursor = 0;
    }

    fn edit_cursor_end(&mut self) {
        let cursor = self.active_cursor();
        self.edit_cursor = self.active_items()[cursor].len();
    }

    fn set_edit(&mut self, edit_active: bool) {
        if !self.edit_mode && edit_active {
            self.edit_cursor_end();
        }
        /* else if self.edit_mode && !edit_active {
            let _ = execute!(stdout(), Hide, DisableBlinking);
        }*/
        self.edit_mode = edit_active;
    }

    fn list_transfer(&mut self) {
        let active_list = self.active_status;
        let active_cursor = self.active_list().cursor;

        if active_cursor < self.active_items().len() {
            let tmp = self.active_items_mut().remove(active_cursor);
            self.lists[(active_list ^ 1) as usize].items.push(tmp);
            if active_cursor >= self.active_items().len() && !self.active_items().is_empty() {
                self.active_list_mut().cursor -= 1;
            }
        }
    }

    fn list_delete(&mut self) {
        //let active_list = self.active_status;
        let active_cursor = self.active_cursor().clone();
        if self.active_cursor() < self.active_items().len() {
            self.active_items_mut().remove(active_cursor);
            if self.active_cursor() >= self.active_items().len() && !self.active_items().is_empty()
            {
                self.active_list_mut().cursor -= 1;
            }
        }
    }

    fn active_cursor(&self) -> usize {
        self.lists[self.active_status as usize].cursor
    }

    fn active_list_mut(&mut self) -> &mut ItemList {
        &mut self.lists[self.active_status as usize]
    }
    fn active_list(&self) -> &ItemList {
        &self.lists[self.active_status as usize]
    }

    fn active_items_mut(&mut self) -> &mut Vec<Item> {
        &mut self.lists[self.active_status as usize].items
    }
    fn active_items(&self) -> &Vec<Item> {
        &self.lists[self.active_status as usize].items
    }

    fn new_item(&mut self) {
        let active_cursor = self.active_cursor();
        self.active_items_mut().insert(active_cursor, String::new());
    }

    fn load_state(&mut self, file_path: &str) -> Result<()> {
        let file = File::open(file_path)?;
        for (index, line) in io::BufReader::new(file).lines().enumerate() {
            let line: String = line?.as_str().trim().to_string();

            if line.is_empty() {
                continue;
            }

            match parse_item(line.as_str()) {
                Some((Status::Todo, title)) => self.lists[Status::Todo as usize]
                    .items
                    .push(title.trim_end().to_string()),
                Some((Status::Done, title)) => self.lists[Status::Done as usize]
                    .items
                    .push(title.trim_end().to_string()),
                None => {
                    eprintln!("{}:{}: ERROR: ill-formed item line", file_path, index + 1);
                    process::exit(1);
                }
            }
        }
        Ok(())
    }

    fn save_state(&mut self, file_path: &str) -> Result<()> {
        let mut file = File::create(file_path)?;
        for (index, line) in self.lists[Status::Todo as usize].items.iter().enumerate() {
            file.write(b"TODO: ")?;
            file.write(line.as_bytes())?;
            file.write(b"\n")?;
        }
        for (index, line) in self.lists[Status::Done as usize].items.iter().enumerate() {
            file.write(b"DONE: ")?;
            file.write(line.as_bytes())?;
            file.write(b"\n")?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct ItemList {
    items: Vec<Item>,
    cursor: usize,
}

impl ItemList {
    fn new() -> Self {
        Self::default()
    }

    fn list_drag_up(&mut self) {
        if self.cursor > 0 {
            self.items.swap(self.cursor, self.cursor - 1);
            self.cursor -= 1;
        }
    }

    fn list_drag_down(&mut self) {
        if self.cursor + 1 < self.items.len() {
            self.items.swap(self.cursor, self.cursor + 1);
            self.cursor += 1;
        }
    }

    fn cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1
        }
    }

    fn cursor_down(&mut self) {
        if self.cursor + 1 < self.items.len() {
            self.cursor += 1;
        }
    }

    fn cursor_to_top(&mut self) {
        self.cursor = 0;
    }

    fn cursor_to_bottom(&mut self) {
        if !self.items.is_empty() {
            self.cursor = self.items.len() - 1;
        }
    }
}

fn parse_item(line: &str) -> Option<(Status, &str)> {
    let todo_item = line
        .strip_prefix("TODO: ")
        .map(|title| (Status::Todo, title));
    let done_item = line
        .strip_prefix("DONE: ")
        .map(|title| (Status::Done, title));
    todo_item.or(done_item)
}

fn get_file_argument(file_path: &mut String) {
    let mut args = env::args();
    args.next().unwrap();
    *file_path = match args.next() {
        Some(file_path) => file_path,
        None => {
            eprintln!("Usage: todo-rs <file-path>");
            eprintln!("ERROR: file path is not provided");
            process::exit(1);
        }
    };
}

fn poll_events(app: &mut App, ui: &mut ui::Ui) -> Result<()> {
    while poll(Duration::from_millis(33))? {
        match read()? {
            Event::Resize(nw, nh) => {
                ui.resize(nw as usize, nw as usize);
            }
            Event::Paste(data) => {
                for c in data.chars() {
                    app.edit_add_char(c);
                }
            }
            Event::Key(event) => {
                if event.kind == KeyEventKind::Press {
                    if app.edit_mode {
                        match event.code {
                            KeyCode::Char(x) => {
                                app.edit_add_char(x);
                            }
                            KeyCode::Left => app.edit_cursor_left(),
                            KeyCode::Right => app.edit_cursor_right(),
                            KeyCode::Home => app.edit_cursor_begin(),
                            KeyCode::End => app.edit_cursor_end(),
                            KeyCode::Backspace => app.backspace(),
                            KeyCode::Esc | KeyCode::Enter => {
                                app.set_edit(false);
                            }
                            _ => {}
                        }
                    } else {
                        match event.code {
                            KeyCode::Char(x) => {
                                if x == 'c' && event.modifiers.contains(KeyModifiers::CONTROL) {
                                    app.quit = true;
                                }
                            }
                            KeyCode::Esc => {
                                app.quit = true;
                            }
                            KeyCode::Enter => app.set_edit(true),
                            KeyCode::Tab => {
                                app.active_status ^= 1;
                            }
                            KeyCode::Up => {
                                if event.modifiers.contains(KeyModifiers::CONTROL) {
                                    app.drag_up();
                                } else {
                                    app.cursor_up();
                                }
                            }
                            KeyCode::Down => {
                                if event.modifiers.contains(KeyModifiers::CONTROL) {
                                    app.drag_down();
                                } else {
                                    app.cursor_down();
                                }
                            }
                            KeyCode::Left => {
                                if app.active_status == Status::Done {
                                    app.list_transfer();
                                }
                            }
                            KeyCode::Right => {
                                if app.active_status == Status::Todo {
                                    app.list_transfer();
                                }
                            }
                            KeyCode::Delete => {
                                app.list_delete();
                            }
                            KeyCode::Insert => {
                                app.new_item();
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// https://github.com/tsoding/4at/blob/main/src/client.rs

fn main() -> Result<()> {
    env::set_var("RUST_BACKTRACE", "full");
    let _screen_state = ScreenState::enable()?;
    let mut app = App::new();
    let (w, h) = terminal::size()?;

    let mut file_path = String::new();
    get_file_argument(&mut file_path);
    app.load_state(&file_path)?;

    let mut last_time = SystemTime::now();

    let mut ui = ui::Ui::new(w as usize, h as usize);
    
    while !app.quit {
        poll_events(&mut app, &mut ui)?;

        if app.edit_mode {
            let now = SystemTime::now();
            if now - Duration::from_millis(300) > last_time {
//                cursor_on ^= true;
                last_time = now
            }
        } else {
  //          cursor_on = false;
        }

        ui.begin(Vec2::null(), LayoutKind::Vert);
        {
            ui.begin_layout(LayoutKind::Horz);
            {
                ui.begin_layout(LayoutKind::Vert);
                {
                    ui.label_fixed_width("TODO", (w / 2).into(), Color::Cyan, Color::Black);
                    for (index, todo) in app.lists[Status::Todo as usize].items.iter().enumerate() {
                        let color = if index == app.active_cursor()
                            && app.active_status == Status::Todo
                            && !app.edit_mode
                        {
                            (Color::Black, Color::White)
                        } else {
                            (Color::White, Color::Black)
                        };
                        ui.label(&format!("[ ] {}", todo), color.0, color.1);
                    }
                }
                ui.end_layout();
                ui.begin_layout(LayoutKind::Vert);
                {
                    ui.label_fixed_width("DONE", (w / 2) as i32, Color::Cyan, Color::Black);
                    for (index, todo) in app.lists[Status::Done as usize].items.iter().enumerate() {
                        let color = if index == app.active_cursor()
                            && app.active_status == Status::Done
                            && !app.edit_mode
                        {
                            (Color::Black, Color::White)
                        } else {
                            (Color::White, Color::Black)
                        };
                        ui.label(&format!("[x] {}", todo), color.0, color.1);
                    }
                }
                ui.end_layout();

            }
            ui.end_layout();
        }

        let edit_state = if app.edit_mode { "Edit" } else { "View" };
        let prompt = format!("{}: {:?}", edit_state, app.active_status);
        let prompt = format!("{:width$}", prompt, width=w as usize);
        //let prompt = format!("{edit_state}: {:?}", app.active_status);
        ui.screen.put_cells(0, h as usize, &prompt, Color::Black, Color::White);

        ui.end();
    }

    app.save_state(&file_path)?;

    Ok(())
}
