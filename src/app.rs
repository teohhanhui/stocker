use crate::stock::Stock;
use chrono::{DateTime, Duration, Utc};
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
    pub end_date: Option<DateTime<Utc>>,
    pub start_date: Option<DateTime<Utc>>,
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

#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
pub enum TimeFrame {
    FiveDays,
    OneMonth,
    ThreeMonths,
    SixMonths,
    YearToDate,
    OneYear,
    TwoYears,
    FiveYears,
    TenYears,
    Max,
}

impl TimeFrame {
    pub fn duration(self) -> Option<Duration> {
        match self {
            Self::FiveDays => Some(Duration::days(5)),
            Self::OneMonth => Some(Duration::days(30)),
            Self::ThreeMonths => Some(Duration::days(30 * 3)),
            Self::SixMonths => Some(Duration::days(30 * 6)),
            Self::OneYear => Some(Duration::days(30 * 12)),
            Self::TwoYears => Some(Duration::days(30 * 12 * 2)),
            Self::FiveYears => Some(Duration::days(30 * 12 * 5)),
            Self::TenYears => Some(Duration::days(30 * 12 * 10)),
            _ => None,
        }
    }

    pub fn interval(self) -> Interval {
        match self {
            Self::FiveDays => Interval::_5d,
            Self::OneMonth => Interval::_1mo,
            Self::ThreeMonths => Interval::_3mo,
            Self::SixMonths => Interval::_6mo,
            Self::YearToDate => Interval::_ytd,
            Self::OneYear => Interval::_1y,
            Self::TwoYears => Interval::_2y,
            Self::FiveYears => Interval::_5y,
            Self::TenYears => Interval::_10y,
            Self::Max => Interval::_max,
        }
    }
}

impl FromStr for TimeFrame {
    type Err = ParseTimeFrameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "5d" => Ok(Self::FiveDays),
            "1mo" => Ok(Self::OneMonth),
            "3mo" => Ok(Self::ThreeMonths),
            "6mo" => Ok(Self::SixMonths),
            "ytd" => Ok(Self::YearToDate),
            "1y" => Ok(Self::OneYear),
            "2y" => Ok(Self::TwoYears),
            "5y" => Ok(Self::FiveYears),
            "10y" => Ok(Self::TenYears),
            "max" => Ok(Self::Max),
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
            Self::FiveDays => write!(f, "5d"),
            Self::OneMonth => write!(f, "1mo"),
            Self::ThreeMonths => write!(f, "3mo"),
            Self::SixMonths => write!(f, "6mo"),
            Self::YearToDate => write!(f, "ytd"),
            Self::OneYear => write!(f, "1y"),
            Self::TwoYears => write!(f, "2y"),
            Self::FiveYears => write!(f, "5y"),
            Self::TenYears => write!(f, "10y"),
            Self::Max => write!(f, "max"),
        }
    }
}
