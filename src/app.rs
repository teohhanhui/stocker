use crate::stock::Stock;
use err_derive::Error;
use im::ordmap::OrdMap;
use parking_lot::{RwLock, RwLockWriteGuard};
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use strum_macros::EnumIter;
use tui::{layout::Rect, widgets::ListState};
use yahoo_finance::Interval;

pub struct App {
    pub stock: Stock,
    pub ui_state: UiState,
}

#[derive(Debug)]
pub struct UiState {
    pub stock_symbol_input_state: InputState,
    pub target_areas: RwLock<OrdMap<UiTarget, Rect>>,
    pub time_frame: TimeFrame,
    pub time_frame_menu_state: MenuState<TimeFrame>,
}

#[derive(Debug)]
pub struct MenuState<T>
where
    T: Clone + ToString,
{
    pub active: bool,
    pub items: Vec<T>,
    list_state: RwLock<ListState>,
}

impl<T> MenuState<T>
where
    T: Clone + PartialEq + ToString,
{
    pub fn new<I>(items: I) -> MenuState<T>
    where
        I: IntoIterator<Item = T>,
    {
        Self {
            active: false,
            items: items.into_iter().collect(),
            list_state: RwLock::new(ListState::default()),
        }
    }

    pub fn selected(&self) -> Option<T> {
        let selected = self.list_state.read().selected()?;

        Some(self.items[selected].clone())
    }

    pub fn select(&self, item: T) {
        let n = self
            .items
            .iter()
            .cloned()
            .position(|t| t == item)
            .expect("item not found");

        self.select_nth(n);
    }

    pub fn select_prev(&self) {
        let selected = self
            .list_state
            .read()
            .selected()
            .expect("cannot select previous item when nothing is selected");

        if selected > 0 {
            self.select_nth(selected - 1);
        }
    }

    pub fn select_next(&self) {
        let selected = self
            .list_state
            .read()
            .selected()
            .expect("cannot select next item when nothing is selected");

        if selected < self.items.len() - 1 {
            self.select_nth(selected + 1);
        }
    }

    pub fn select_nth(&self, n: usize) {
        self.list_state.write().select(Some(n));
    }

    pub fn clear_selection(&self) {
        self.list_state.write().select(None);
    }

    pub fn list_state_write(&self) -> RwLockWriteGuard<ListState> {
        self.list_state.write()
    }
}

#[derive(Debug)]
pub struct InputState {
    pub active: bool,
    pub value: String,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            active: false,
            value: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UiTarget {
    StockName,
    StockSymbol,
    StockSymbolInput,
    TimeFrame,
    TimeFrameMenu,
}

impl UiTarget {
    pub fn zindex(self) -> i8 {
        match self {
            Self::StockName => 0,
            Self::StockSymbol => 0,
            Self::StockSymbolInput => 1,
            Self::TimeFrame => 0,
            Self::TimeFrameMenu => 1,
        }
    }
}

impl Ord for UiTarget {
    fn cmp(&self, other: &Self) -> Ordering {
        let ordering = self.zindex().cmp(&other.zindex());
        if ordering == Ordering::Equal {
            (*self as isize).cmp(&(*other as isize))
        } else {
            ordering
        }
    }
}

impl PartialOrd for UiTarget {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
pub enum TimeFrame {
    _5d,
    _1mo,
    _3mo,
    _6mo,
    _1y,
    _2y,
    _5y,
    _10y,
    _ytd,
    _max,
}

impl TimeFrame {
    pub fn interval(self) -> Interval {
        match self {
            Self::_5d => Interval::_5d,
            Self::_1mo => Interval::_1mo,
            Self::_3mo => Interval::_3mo,
            Self::_6mo => Interval::_6mo,
            Self::_1y => Interval::_1y,
            Self::_2y => Interval::_2y,
            Self::_5y => Interval::_5y,
            Self::_10y => Interval::_10y,
            Self::_ytd => Interval::_ytd,
            Self::_max => Interval::_max,
        }
    }
}

impl FromStr for TimeFrame {
    type Err = ParseTimeFrameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "5d" => Ok(Self::_5d),
            "1mo" => Ok(Self::_1mo),
            "3mo" => Ok(Self::_3mo),
            "6mo" => Ok(Self::_6mo),
            "1y" => Ok(Self::_1y),
            "2y" => Ok(Self::_2y),
            "5y" => Ok(Self::_5y),
            "10y" => Ok(Self::_10y),
            "ytd" => Ok(Self::_ytd),
            "max" => Ok(Self::_max),
            "" => Err(ParseTimeFrameError::Empty),
            _ => Err(ParseTimeFrameError::Invalid),
        }
    }
}

#[derive(Debug, Error)]
pub enum ParseTimeFrameError {
    #[error(display = "cannot parse time frame from empty string")]
    Empty,
    #[error(display = "invalid time frame literal")]
    Invalid,
}

impl fmt::Display for TimeFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::_5d => write!(f, "5d"),
            Self::_1mo => write!(f, "1mo"),
            Self::_3mo => write!(f, "3mo"),
            Self::_6mo => write!(f, "6mo"),
            Self::_1y => write!(f, "1y"),
            Self::_2y => write!(f, "2y"),
            Self::_5y => write!(f, "5y"),
            Self::_10y => write!(f, "10y"),
            Self::_ytd => write!(f, "ytd"),
            Self::_max => write!(f, "max"),
        }
    }
}
