use std::{
    io::{self, Write},
    mem,
};

use crossterm::{
    cursor::MoveTo,
    style::{Color, Print, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use crossterm::{execute, queue, ExecutableCommand, QueueableCommand};

#[derive(Default)]
pub struct VirtualScreen {
    buf_curr: Buffer,
    buf_prev: Buffer,
}

impl VirtualScreen {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            buf_curr: Buffer::new(w, h),
            buf_prev: Buffer::new(w, h),
        }
    }

    pub fn flush(&self, qc: &mut impl Write) -> io::Result<()> {
        self.buf_prev.flush(qc)
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.buf_curr.resize(width, height);
        self.buf_prev.resize(width, height);
    }

    pub fn diff(&self) -> Vec<Patch> {
        self.buf_prev.diff(&self.buf_curr)
    }

    pub fn put_cell(&mut self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
        self.buf_curr.put_cell(x, y, ch, fg, bg)
    }

    pub fn put_cells(&mut self, x: usize, y: usize, chs: &str, fg: Color, bg: Color) {
        self.buf_curr.put_cells(x, y, chs, fg, bg)
    }

    pub fn swap(&mut self) {
        mem::swap(&mut self.buf_curr, &mut self.buf_prev);
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Cell {
    ch: char,
    fg: Color,
    bg: Color,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::White,
            bg: Color::Black,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Buffer {
    cells: Vec<Cell>,
    width: usize,
    height: usize,
}

pub struct Patch {
    cell: Cell,
    x: usize,
    y: usize,
}

impl Buffer {
    pub fn new(width: usize, height: usize) -> Self {
        let cells = vec![Cell::default(); width * height];
        Self {
            cells,
            width,
            height,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.cells.resize(width * height, Cell::default());
        self.cells.fill(Cell::default());
        self.width = width;
        self.height = height;
    }

    pub fn diff(&self, other: &Self) -> Vec<Patch> {
        assert!(self.width == other.width && self.height == other.height);
        self.cells
            .iter()
            .zip(other.cells.iter())
            .enumerate()
            .filter(|(_, (a, b))| *a != *b)
            .map(|(i, (_, cell))| {
                let x = i % self.width;
                let y = i / self.width;
                Patch {
                    cell: cell.clone(),
                    x,
                    y,
                }
            })
            .collect()
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
    }

    pub fn put_cell(&mut self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
        if let Some(cell) = self.cells.get_mut(y * self.width + x) {
            *cell = Cell { ch, fg, bg }
        }
    }

    pub fn put_cells(&mut self, x: usize, y: usize, chs: &str, fg: Color, bg: Color) {
        let start = y * self.width + x;
        for (offset, ch) in chs.chars().enumerate() {
            if let Some(cell) = self.cells.get_mut(start + offset) {
                *cell = Cell { ch, fg, bg };
            } else {
                break;
            }
        }
    }

    pub fn flush(&self, qc: &mut impl Write) -> io::Result<()> {
        let mut fg_curr = Color::White;
        let mut bg_curr = Color::Black;

        queue!(
            qc,
            Clear(ClearType::All),
            SetForegroundColor(fg_curr),
            SetBackgroundColor(bg_curr),
            MoveTo(0, 0),
        )?;

        for Cell { ch, fg, bg } in self.cells.iter() {
            if fg_curr != *fg {
                fg_curr = *fg;
                qc.queue(SetForegroundColor(fg_curr))?;
            }
            if bg_curr != *bg {
                bg_curr = *bg;
                qc.queue(SetBackgroundColor(bg_curr))?;
            }
            qc.queue(Print(ch))?;
        }

        qc.flush()?;
        Ok(())
    }
}


pub fn apply_patches(qc: &mut impl QueueableCommand, patches: &[Patch]) -> io::Result<()> {
    let mut fg_curr = Color::White;
    let mut bg_curr = Color::Black;
    let mut x_prev = 0;
    let mut y_prev = 0;
    qc.queue(SetForegroundColor(fg_curr))?;
    qc.queue(SetBackgroundColor(bg_curr))?;
    for Patch {
        cell: Cell { ch, fg, bg },
        x,
        y,
    } in patches
    {
        if !(y_prev == *y && x_prev + 1 == *x) {
            qc.queue(MoveTo(*x as u16, *y as u16))?;
        }
        x_prev = *x;
        y_prev = *y;
        if fg_curr != *fg {
            fg_curr = *fg;
            qc.queue(SetForegroundColor(fg_curr))?;
        }
        if bg_curr != *bg {
            bg_curr = *bg;
            qc.queue(SetBackgroundColor(bg_curr))?;
        }
        qc.queue(Print(ch))?;
    }
    Ok(())
}