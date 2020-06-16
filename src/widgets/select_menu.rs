use anyhow::Context;
use std::marker::PhantomData;
use tui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    widgets::{self, Block, Borders, Clear, List, ListState, Paragraph, Text},
};

pub struct SelectMenuBox<'a, S: 'a, T>
where
    S: Clone + PartialEq + ToString,
    T: Iterator<Item = &'a Text<'a>>,
{
    active_border_style: Style,
    active_style: Style,
    paragraph: Paragraph<'a, 'a, T>,
    phantom_s: PhantomData<&'a S>,
}

impl<'a, S, T> SelectMenuBox<'a, S, T>
where
    S: Clone + PartialEq + ToString,
    T: Iterator<Item = &'a Text<'a>>,
{
    pub fn new(text: T) -> Self {
        Self {
            active_border_style: Style::default().fg(Color::Gray),
            active_style: Style::default().fg(Color::White).bg(Color::DarkGray),
            paragraph: Paragraph::new(text),
            phantom_s: PhantomData,
        }
    }

    pub fn active_border_style(mut self, active_border_style: Style) -> Self {
        self.active_border_style = active_border_style;
        self
    }

    pub fn active_style(mut self, active_style: Style) -> Self {
        self.active_style = active_style;
        self
    }

    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.paragraph = self.paragraph.alignment(alignment);
        self
    }
}

impl<'a, S, T> widgets::StatefulWidget for SelectMenuBox<'a, S, T>
where
    S: Clone + PartialEq + ToString,
    T: Iterator<Item = &'a Text<'a>>,
{
    type State = SelectMenuState<S>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let paragraph = self
            .paragraph
            .block(if state.active {
                Block::default()
                    .style(self.active_style)
                    .borders(Borders::ALL ^ Borders::TOP)
                    .border_style(self.active_border_style)
            } else {
                Block::default()
            })
            .style(if state.active {
                self.active_style
            } else {
                Style::default()
            });

        widgets::Widget::render(paragraph, area, buf);
    }
}

pub struct SelectMenuList<'a, L, S: 'a>
where
    L: Iterator<Item = Text<'a>>,
    S: Clone + PartialEq + ToString,
{
    list: List<'a, L>,
    phantom_s: PhantomData<&'a S>,
}

impl<'a, L, S> SelectMenuList<'a, L, S>
where
    L: Iterator<Item = Text<'a>>,
    S: Clone + PartialEq + ToString,
{
    pub fn new(items: L) -> Self {
        Self {
            list: List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Gray)),
                )
                .highlight_style(Style::default().fg(Color::Black).bg(Color::White)),
            phantom_s: PhantomData,
        }
    }

    pub fn border_style(mut self, border_style: Style) -> Self {
        self.list = self.list.block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style),
        );
        self
    }

    pub fn highlight_style(mut self, highlight_style: Style) -> Self {
        self.list = self.list.highlight_style(highlight_style);
        self
    }
}

impl<'a, L, S> widgets::StatefulWidget for SelectMenuList<'a, L, S>
where
    L: Iterator<Item = Text<'a>>,
    S: Clone + PartialEq + ToString,
{
    type State = SelectMenuState<S>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        widgets::Widget::render(Clear, area, buf);
        widgets::StatefulWidget::render(self.list, area, buf, &mut state.list_state);
    }
}

#[derive(Debug)]
pub struct SelectMenuState<T>
where
    T: Clone + PartialEq + ToString,
{
    pub active: bool,
    pub allow_empty_selection: bool,
    pub items: Vec<T>,
    list_state: ListState,
}

impl<T> SelectMenuState<T>
where
    T: Clone + PartialEq + ToString,
{
    pub fn new<I>(items: I) -> SelectMenuState<T>
    where
        I: IntoIterator<Item = T>,
    {
        Self {
            active: false,
            allow_empty_selection: false,
            items: items.into_iter().collect(),
            list_state: ListState::default(),
        }
    }

    pub fn selected(&self) -> Option<T> {
        let n = self.list_state.selected()?;
        let n = if self.allow_empty_selection {
            if n == 0 {
                return None;
            }
            n - 1
        } else {
            n
        };

        Some(self.items[n].clone())
    }

    pub fn select(&mut self, item: T) -> anyhow::Result<()> {
        let n = self
            .items
            .iter()
            .cloned()
            .position(|t| t == item)
            .map(|n| if self.allow_empty_selection { n + 1 } else { n })
            .with_context(|| "item not found")?;

        self.select_nth(n)?;

        Ok(())
    }

    pub fn select_prev(&mut self) -> anyhow::Result<()> {
        let n = self.list_state.selected().map_or_else(
            || {
                let l = self.items.len();
                if self.allow_empty_selection {
                    l
                } else {
                    l - 1
                }
            },
            |n| if n > 0 { n - 1 } else { 0 },
        );

        self.select_nth(n)?;

        Ok(())
    }

    pub fn select_next(&mut self) -> anyhow::Result<()> {
        let n = self.list_state.selected().map_or(0, |n| {
            let l = self.items.len();
            let l = if self.allow_empty_selection { l } else { l - 1 };
            if n < l {
                n + 1
            } else {
                n
            }
        });

        self.select_nth(n)?;

        Ok(())
    }

    pub fn select_nth(&mut self, n: usize) -> anyhow::Result<()> {
        self.list_state.select(Some(n));

        Ok(())
    }

    pub fn clear_selection(&mut self) -> anyhow::Result<()> {
        self.list_state.select(None);

        Ok(())
    }
}
