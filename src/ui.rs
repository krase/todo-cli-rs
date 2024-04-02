use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::style::{Color, Print, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use crossterm::{queue, ExecutableCommand, QueueableCommand};
use std::cmp;
use std::io::{self, stderr, stdout, BufRead, Write};
use std::ops::{Add, Div, Mul, Sub};

use crate::screen_buf::{apply_patches, VirtualScreen};

#[derive(Default, Copy, Clone)]
pub struct Vec2 {
    x: i32,
    y: i32,
}

impl Add for Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Mul for Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Vec2 {
        Vec2 {
            x: self.x * rhs.x,
            y: self.y * rhs.y,
        }
    }
}

impl Vec2 {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn null() -> Self {
        Self { x: 0, y: 0 }
    }
}

pub enum LayoutKind {
    Vert,
    Horz,
}

pub struct Layout {
    kind: LayoutKind,
    pos: Vec2,
    size: Vec2,
}

impl Layout {
    fn available_pos(&self) -> Vec2 {
        use LayoutKind::*;
        match self.kind {
            Horz => self.pos + self.size * Vec2::new(1, 0),
            Vert => self.pos + self.size * Vec2::new(0, 1),
        }
    }

    fn add_widget(&mut self, size: Vec2) {
        use LayoutKind::*;
        match self.kind {
            Horz => {
                self.size.x += size.x;
                self.size.y = cmp::max(self.size.y, size.y);
            }
            Vert => {
                self.size.x = cmp::max(self.size.x, size.x);
                self.size.y += size.y;
            }
        }
    }
}

#[derive(Default)]
pub struct Ui {
    layouts: Vec<Layout>,
    pub screen: VirtualScreen,
    //key: Option<i32>,
}

impl Ui {
    pub fn new(width: usize, height: usize) -> Self {
        let ret = Self {
            screen: VirtualScreen::new(width, height),
            layouts: Vec::default(),
        };
        ret.screen.flush(&mut stdout()).unwrap();
        ret
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen.resize(width, height);
        self.screen.flush(&mut stdout()).unwrap();
    }

    pub fn begin(&mut self, pos: Vec2, kind: LayoutKind) {
        assert!(self.layouts.is_empty());
        self.layouts.push(Layout {
            kind,
            pos,
            size: Vec2::null(),
        })
    }

    pub fn begin_layout(&mut self, kind: LayoutKind) {
        let layout = self
            .layouts
            .last()
            .expect("Can't create a layout outside of Ui::begin() and Ui::end()");
        let pos = layout.available_pos();
        self.layouts.push(Layout {
            kind,
            pos,
            size: Vec2::null(),
        });
    }

    pub fn end_layout(&mut self) {
        let layout = self
            .layouts
            .pop()
            .expect("Unbalanced Ui::begin_layout() and Ui::end_layout() calls.");
        self.layouts
            .last_mut()
            .expect("Unbalanced Ui::begin_layout() and Ui::end_layout() calls.")
            .add_widget(layout.size);
    }

    pub fn label_fixed_width(&mut self, text: &str, width: i32, fg: Color, bg: Color) -> Vec2 {
        // TODO(#17): Ui::label_fixed_width() does not elide the text when width < text.len()
        let layout = self
            .layouts
            .last_mut()
            .expect("Trying to render label outside of any layout");

        let pos = layout.available_pos();

        self.screen.put_cells(pos.x as usize, pos.y as usize, text, fg, bg);
        let fill = std::iter::repeat(" ").take((layout.size.x-width-4).abs() as usize).collect::<String>();
        self.screen.put_cells((pos.x + width) as usize, pos.y as usize, &fill, fg, bg);
        
        layout.add_widget(Vec2::new(width, 1));

        pos
    }

    pub fn label_edit(&mut self, text: &str, fg: Color, bg: Color, edit: bool) {
        let len = text.chars().count();
        let pos = self.label_fixed_width(text, len as i32, fg, bg);
        if edit {
            self.screen.put_cell(pos.x as usize + len, pos.y as usize, ' ', fg, fg);
            self.screen.put_cell(pos.x as usize + len + 1, pos.y as usize, ' ', bg, bg);
        } else {
            self.screen.put_cell(pos.x as usize + len, pos.y as usize, ' ', bg, bg);
        }
    }

    #[allow(dead_code)]
    pub fn label(&mut self, text: &str, fg: Color, bg: Color) {
        self.label_fixed_width(text, text.len() as i32, fg, bg);
    }

    pub fn end(&mut self) {
        self.layouts
            .pop()
            .expect("Unbalanced Ui::begin() and Ui::end() calls.");

        let mut stdout = stdout();
        apply_patches(&mut stdout, &self.screen.diff()).unwrap();

        self.screen.swap();
        stdout.flush().unwrap();
    }
}
