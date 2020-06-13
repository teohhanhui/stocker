use anyhow::Context;
use std::marker::PhantomData;
use tui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, List, ListState, StatefulWidget, Text},
};

pub struct SelectMenuList<'a, L, T: 'a>
where
    L: Iterator<Item = Text<'a>>,
    T: Clone + PartialEq + ToString,
{
    list: List<'a, L>,
    phantom: PhantomData<&'a T>,
}

impl<'a, L, T> SelectMenuList<'a, L, T>
where
    L: Iterator<Item = Text<'a>>,
    T: Clone + PartialEq + ToString,
{
    pub fn new(items: L) -> Self {
        Self {
            list: List::new(items).block(Block::default().borders(Borders::ALL)),
            phantom: PhantomData,
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

impl<'a, L, T> StatefulWidget for SelectMenuList<'a, L, T>
where
    L: Iterator<Item = Text<'a>>,
    T: Clone + PartialEq + ToString,
{
    type State = SelectMenuState<T>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        self.list.render(area, buf, &mut state.list_state);
    }
}

#[derive(Debug)]
pub struct SelectMenuState<T>
where
    T: Clone + PartialEq + ToString,
{
    pub active: bool,
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
            items: items.into_iter().collect(),
            list_state: ListState::default(),
        }
    }

    pub fn selected(&self) -> Option<T> {
        let selected = self.list_state.selected()?;

        Some(self.items[selected].clone())
    }

    pub fn select(&mut self, item: T) -> anyhow::Result<()> {
        let n = self
            .items
            .iter()
            .cloned()
            .position(|t| t == item)
            .with_context(|| "item not found")?;

        self.select_nth(n)?;

        Ok(())
    }

    pub fn select_prev(&mut self) -> anyhow::Result<()> {
        let selected = self
            .list_state
            .selected()
            .with_context(|| "cannot select previous item when nothing is selected")?;

        if selected > 0 {
            self.select_nth(selected - 1)?;
        }

        Ok(())
    }

    pub fn select_next(&mut self) -> anyhow::Result<()> {
        let selected = self
            .list_state
            .selected()
            .with_context(|| "cannot select next item when nothing is selected")?;

        if selected < self.items.len() - 1 {
            self.select_nth(selected + 1)?;
        }

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
