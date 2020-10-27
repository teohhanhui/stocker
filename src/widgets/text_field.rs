use tui::{
    buffer::Buffer,
    layout::{Margin, Rect},
    style::{Color, Style},
    text::Text,
    widgets::{self, Block, Borders, Clear, Paragraph},
};

pub struct TextField<'a> {
    block: Block<'a>,
    paragraph: Paragraph<'a>,
}

impl<'a> TextField<'a> {
    pub fn new<T>(text: T) -> Self
    where
        T: Into<Text<'a>>,
    {
        let block = Block::default()
            .style(Style::default().fg(Color::White).bg(Color::DarkGray))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Gray));
        let paragraph = Paragraph::new(text).block(block.clone());

        Self { block, paragraph }
    }

    pub fn border_style(mut self, border_style: Style) -> Self {
        self.block = self.block.border_style(border_style);
        self.paragraph = self.paragraph.block(self.block.clone());
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.block = self.block.style(style);
        self.paragraph = self.paragraph.block(self.block.clone());
        self
    }
}

impl<'a> widgets::StatefulWidget for TextField<'a> {
    type State = TextFieldState;

    fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
        widgets::Widget::render(Clear, area, buf);
        widgets::Widget::render(self.paragraph, area, buf);
    }
}

#[derive(Clone, Debug, Default)]
pub struct TextFieldState {
    pub active: bool,
    pub cursor_offset: usize,
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

        let cx = inner_area.left() + self.cursor_offset as u16;
        let cy = inner_area.top();

        Some((cx, cy))
    }
}
