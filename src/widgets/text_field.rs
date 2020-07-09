use tui::{
    buffer::Buffer,
    layout::Rect,
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
