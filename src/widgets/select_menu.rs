use anyhow::{ensure, Context};
use std::marker::PhantomData;
use tui::{
    buffer::Buffer,
    layout::{Alignment, Margin, Rect},
    style::{Color, Style},
    widgets::{self, Block, Borders, Clear, List, ListState, Paragraph, Text},
};

pub struct SelectMenuBox<'a, 't, S: 'a, T>
where
    S: Clone + PartialEq + ToString,
    T: Iterator<Item = &'t Text<'t>>,
{
    active_border_style: Style,
    active_style: Style,
    paragraph: Paragraph<'a, 't, T>,
    phantom_s: PhantomData<&'a S>,
}

impl<'a, 't, S, T> SelectMenuBox<'a, 't, S, T>
where
    S: Clone + PartialEq + ToString,
    T: Iterator<Item = &'t Text<'t>>,
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

impl<'a, 't, S, T> widgets::StatefulWidget for SelectMenuBox<'a, 't, S, T>
where
    S: Clone + PartialEq + ToString,
    T: Iterator<Item = &'t Text<'t>>,
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

        widgets::Widget::render(Clear, area, buf);
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

#[derive(Clone, Debug, Default)]
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
        self.list_state
            .selected()
            .map(|n| {
                if self.allow_empty_selection {
                    if n == 0 {
                        None
                    } else {
                        Some(n - 1)
                    }
                } else {
                    Some(n)
                }
            })?
            .map(|n| self.items[n].clone())
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub fn select(&mut self, item: Option<T>) -> anyhow::Result<()> {
        let n = item.map_or_else(
            || {
                ensure!(self.allow_empty_selection, "empty selection not allowed");
                Ok(0)
            },
            |item| {
                self.items
                    .iter()
                    .cloned()
                    .position(|t| t == item)
                    .map(|n| if self.allow_empty_selection { n + 1 } else { n })
                    .with_context(|| "item not found")
            },
        )?;

        self.select_index(n)?;

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

        self.select_index(n)?;

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

        self.select_index(n)?;

        Ok(())
    }

    pub fn select_index(&mut self, n: usize) -> anyhow::Result<()> {
        self.list_state.select(Some(n));

        Ok(())
    }

    pub fn point_to_index(&self, menu_area: Rect, (x, y): (u16, u16)) -> Option<usize> {
        let border_margin = Margin {
            horizontal: 1,
            vertical: 1,
        };
        let inner_area = menu_area.inner(&border_margin);

        if inner_area.left() <= x
            && inner_area.right() >= x
            && inner_area.top() <= y
            && inner_area.bottom() >= y
        {
            if (inner_area.height as usize) < self.items.len() {
                todo!("not sure how to select an item from scrollable list");
            }
            let n: usize = (y - inner_area.top()) as usize;
            let l = self.items.len();
            let l = if self.allow_empty_selection { l + 1 } else { l };

            if n < l {
                return Some(n);
            }
        }

        None
    }
}
