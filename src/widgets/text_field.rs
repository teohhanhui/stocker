use tui::{
    buffer::Buffer,
    layout::{Margin, Rect},
    style::{Color, Style},
    widgets::{self, Block, Borders, Clear, Paragraph, Text},
};

pub struct TextField<'a, 't, T>
where
    T: Iterator<Item = &'t Text<'t>>,
{
    block: Block<'a>,
    paragraph: Paragraph<'a, 't, T>,
}

impl<'a, 't, T> TextField<'a, 't, T>
where
    T: Iterator<Item = &'t Text<'t>>,
{
    pub fn new(text: T) -> Self {
        let block = Block::default()
            .style(Style::default().fg(Color::White).bg(Color::DarkGray))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Gray));

        Self {
            block,
            paragraph: Paragraph::new(text).block(block),
        }
    }

    pub fn border_style(mut self, border_style: Style) -> Self {
        self.block = self.block.border_style(border_style);
        self.paragraph = self.paragraph.block(self.block);
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.block = self.block.style(style);
        self.paragraph = self.paragraph.block(self.block);
        self
    }
}

impl<'a, 't, T> widgets::StatefulWidget for TextField<'a, 't, T>
where
    T: Iterator<Item = &'t Text<'t>>,
{
    type State = TextFieldState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let paragraph = self.paragraph;

        widgets::Widget::render(Clear, area, buf);
        widgets::Widget::render(paragraph, area, buf);
    }
}

#[derive(Clone, Debug, Default)]
pub struct TextFieldState {
    pub active: bool,
    pub value: String,
}

impl TextFieldState {
    pub fn cursor_point(&self, text_field_area: Rect) -> Option<(u16, u16)> {
        if !self.active {
            return None;
        }

        let border_margin = Margin {
            horizontal: 1,
            vertical: 1,
        };
        let inner_area = text_field_area.inner(&border_margin);

        let cx = inner_area.left() + self.value.chars().count() as u16;
        let cy = inner_area.top();

        Some((cx, cy))
    }
}
