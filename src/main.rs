#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{env, process, thread};
use std::fs::File;
use std::io::{self, BufRead, stdout, Write};
use std::ops::{BitXor, BitXorAssign};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use crossterm::{ExecutableCommand, execute, queue, QueueableCommand};
use crossterm::cursor::{DisableBlinking, Hide, MoveTo, SetCursorStyle, Show};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, poll, read};
use crossterm::style::{Color, Print, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};

type Item = String;

struct ScreenState;

impl ScreenState {
    fn enable() -> io::Result<Self> {
        execute!(stdout(), EnterAlternateScreen, SetCursorStyle::SteadyBlock)?;
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for ScreenState {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode().map_err(|err| {
            eprintln!("ERROR: disable raw mode: {err}")
        });
        let _ = execute!(stdout(), LeaveAlternateScreen).map_err(|err| {
            eprintln!("ERROR: leave alternate screen: {err}")
        });
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
    w: u16,
    h: u16,
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
        self.active_items_mut()[cursor].insert(edit_cursor, c);
        self.edit_cursor += 1;
    }
    fn edit_cursor_left(&mut self) {
        self.edit_cursor -= 1;
    }
    fn edit_cursor_right(&mut self) {
        self.edit_cursor += 1;
    }
    fn edit_cursor_begin(&mut self) {
        self.edit_cursor = 0;
    }
    fn edit_cursor_end(&mut self) {
        let cursor = self.active_cursor();
        self.edit_cursor = self.active_items()[cursor].len();
    }

    fn backspace(&mut self) {
        let cursor = self.active_cursor();
        self.active_items_mut()[cursor].pop();
        self.edit_cursor -= 1;
    }

    fn set_edit(&mut self, edit_active: bool) {
        if !self.edit_mode && edit_active {
            self.edit_cursor_end();
        } else if self.edit_mode && !edit_active {
            let _ = execute!(stdout(), Hide,DisableBlinking);
        }
        self.edit_mode = edit_active;
    }

    fn list_transfer(&mut self)
    {
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
            if self.active_cursor() >= self.active_items().len() && !self.active_items().is_empty() {
                self.active_list_mut().cursor -= 1;
            }
        }
    }

    fn active_cursor(&self) -> usize
    {
        self.lists[self.active_status as usize].cursor
    }

    fn active_list_mut(&mut self) -> &mut ItemList
    {
        &mut self.lists[self.active_status as usize]
    }
    fn active_list(&self) -> &ItemList
    {
        &self.lists[self.active_status as usize]
    }

    fn active_items_mut(&mut self) -> &mut Vec<Item>
    {
        &mut self.lists[self.active_status as usize].items
    }
    fn active_items(&self) -> &Vec<Item>
    {
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
                Some((Status::Todo, title)) => self.lists[Status::Todo as usize].items.push(title.to_string()),
                Some((Status::Done, title)) => self.lists[Status::Done as usize].items.push(title.to_string()),
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

fn poll_events(app: &mut App) -> Result<()> {
    while poll(Duration::ZERO)? {
        match read()? {
            Event::Resize(nw, nh) => {
                app.w = nw;
                app.h = nh;
            }
            Event::Paste(data) => {
                for c in data.chars() {
                    app.edit_add_char(c);
                }
            }
            Event::Key(event) => if event.kind == KeyEventKind::Press {
                if app.edit_mode {
                    match event.code {
                        KeyCode::Char(x) => {
                            app.edit_add_char(x);
                        }
                        KeyCode::Left => { app.edit_cursor_left() }
                        KeyCode::Right => { app.edit_cursor_right() }
                        KeyCode::Home => { app.edit_cursor_begin() }
                        KeyCode::End => { app.edit_cursor_end() }
                        KeyCode::Backspace => {
                            app.backspace()
                        }
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
                        KeyCode::Enter => {
                            app.set_edit(true)
                        }
                        KeyCode::Tab => {
                            queue!(stdout(), Print("tab"))?;
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
            _ => {}
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut stdout = stdout();
    let _screen_state = ScreenState::enable()?;
    let mut app = App::new();
    (app.w, app.h) = terminal::size()?;
    //let mut prompt = String::new();

    let mut file_path = String::new();
    get_file_argument(&mut file_path);
    app.load_state(&file_path)?;

    let mut last_time = SystemTime::now();
    let mut cursor_hidden = true;
    let _ = execute!(stdout, Hide, DisableBlinking);

    while !app.quit {
        poll_events(&mut app)?;
        queue!(stdout, Clear(ClearType::All))?;

        for (index, todo) in app.lists[app.active_status as usize].items.iter().enumerate() {
            queue!(stdout, MoveTo(0, index as u16 + 1))?;
            if index == app.active_cursor() && !app.edit_mode {
                queue!(stdout, SetBackgroundColor(Color::White))?;
                queue!(stdout, SetForegroundColor(Color::Black))?;
            } else {
                queue!(stdout, SetBackgroundColor(Color::Black))?;
                queue!(stdout, SetForegroundColor(Color::White))?;
            }
            let cross = if app.active_status == Status::Todo { " " } else { "X" };
            queue!(stdout, Print(&format!("- [{cross}] {todo}")))?;
            queue!(stdout, SetForegroundColor(Color::White))?;
            queue!(stdout, SetBackgroundColor(Color::Black))?;
        }

        let edit_state = if app.edit_mode { "Edit" } else { "View" };
        let prompt = format!("{edit_state}: {:?}", app.active_status);

        queue!(stdout, MoveTo(0, app.h-1))?;
        queue!(stdout, Print(&prompt))?;

        if app.edit_mode {
            let cursor = app.active_cursor();
            queue!(stdout,
                    MoveTo(6 + app.edit_cursor as u16, (cursor + 1) as  u16),
                    SetCursorStyle::SteadyUnderScore,
                )?;
            let now = SystemTime::now();

            if now - Duration::from_millis(400) > last_time
            {
                if cursor_hidden {
                    execute!(stdout, Show)?;
                } else {
                    execute!(stdout, Hide)?;
                }
                cursor_hidden ^= true;
                last_time = now
            }
        } else {
            cursor_hidden = true;
        }

        stdout.flush()?;
        thread::sleep(Duration::from_millis(33));
    }

    app.save_state(&file_path)?;

    Ok(())
}

