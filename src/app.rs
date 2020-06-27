use crate::{event::InputEvent, reactive::StreamExt, stock::Stock, widgets::SelectMenuState};
use chrono::{DateTime, Duration, Utc};
use crossterm::event::{KeyCode, KeyEvent};
use derive_more::{Display, From, Into};
use derive_new::new;
use math::round;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use reactive_rs::{Broadcast, Stream};
use regex::Regex;
use shrinkwraprs::Shrinkwrap;
use std::{
    cell::RefCell,
    fmt,
    marker::PhantomData,
    num::ParseIntError,
    ops::Range,
    rc::Rc,
    str::FromStr,
    sync::atomic::{self, AtomicU16},
};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;
use thiserror::Error;
use tui::layout::{Margin, Rect};
use typenum::{Unsigned, U2, U20, U50};
use yahoo_finance::Interval;

pub struct App<'r> {
    pub stock: Stock,
    pub ui_state: UiState<'r>,
}

type DateRange = Range<DateTime<Utc>>;

#[derive(Clone)]
pub struct UiState<'r> {
    pub date_range: Option<DateRange>,
    // debug_draw: bool,
    // pub frame_rate_counter: FrameRateCounter,
    pub indicator: Option<Indicator>,
    pub indicator_menu_state: Rc<RefCell<SelectMenuState<Indicator>>>,
    pub stock_symbol_input_state: InputState,
    pub time_frame: TimeFrame,
    pub time_frame_menu_state: Rc<RefCell<SelectMenuState<TimeFrame>>>,
    pub ui_target_areas: Broadcast<'r, (), (UiTarget, Option<Rect>)>,
}

impl<'r> UiState<'r> {
    // pub fn input_cursor(
    //     &self,
    //     input_state: &InputState,
    //     input_target: UiTarget,
    // ) -> Option<(u16, u16)> {
    //     let target_areas = self.target_areas.read();
    //     let input_area = target_areas.get(&input_target)?;
    //     let border_margin = Margin {
    //         horizontal: 1,
    //         vertical: 1,
    //     };
    //     let inner_area = input_area.inner(&border_margin);

    //     let cx = inner_area.left() + input_state.value.chars().count() as u16;
    //     let cy = inner_area.top();

    //     Some((cx, cy))
    // }

    // pub fn menu_index<T>(
    //     &self,
    //     menu_state: &SelectMenuState<T>,
    //     menu_area: Rect,
    //     x: u16,
    //     y: u16,
    // ) -> Option<usize>
    // where
    //     T: Clone + PartialEq + ToString,
    // {
    //     let border_margin = Margin {
    //         horizontal: 1,
    //         vertical: 1,
    //     };
    //     let inner_area = menu_area.inner(&border_margin);

    //     if inner_area.left() <= x
    //         && inner_area.right() >= x
    //         && inner_area.top() <= y
    //         && inner_area.bottom() >= y
    //     {
    //         if (inner_area.height as usize) < menu_state.items.len() {
    //             todo!("not sure how to select an item from scrollable list");
    //         }
    //         let n: usize = (y - inner_area.top()) as usize;
    //         let l = menu_state.items.len();
    //         let l = if menu_state.allow_empty_selection {
    //             l + 1
    //         } else {
    //             l
    //         };

    //         if n < l {
    //             return Some(n);
    //         }
    //     }

    //     None
    // }
}

impl<'r> Default for UiState<'r> {
    fn default() -> Self {
        Self {
            date_range: None,
            // debug_draw: false,
            // frame_rate_counter: FrameRateCounter::new(Duration::milliseconds(1_000)),
            indicator: None,
            indicator_menu_state: Rc::new(RefCell::new({
                let mut menu_state = SelectMenuState::new(Indicator::iter());
                menu_state.allow_empty_selection = true;
                menu_state.select_nth(0).unwrap();
                menu_state
            })),
            stock_symbol_input_state: InputState::default(),
            time_frame: TimeFrame::default(),
            time_frame_menu_state: Rc::new(RefCell::new({
                let mut menu_state = SelectMenuState::new(TimeFrame::iter());
                menu_state.select(TimeFrame::default()).unwrap();
                menu_state
            })),
            ui_target_areas: Broadcast::new(),
        }
    }
}

pub fn to_date_ranges<'a, V, S, R, U, C>(
    input_events: V,
    stock_symbols: S,
    time_frames: R,
    active_overlay_ui_targets: U,
) -> impl Stream<'a, Item = Option<DateRange>, Context = C>
where
    V: Stream<'a, Item = InputEvent, Context = C>,
    S: Stream<'a, Item = String>,
    R: Stream<'a, Item = TimeFrame>,
    U: Stream<'a, Item = Option<UiTarget>>,
    C: 'a + Clone,
{
    input_events
        .combine_latest(
            stock_symbols.distinct_until_changed(),
            |(ev, stock_symbol)| (*ev, stock_symbol.clone()),
        )
        .combine_latest(
            time_frames.distinct_until_changed(),
            |((ev, stock_symbol), time_frame)| (*ev, stock_symbol.clone(), *time_frame),
        )
        .with_latest_from(
            active_overlay_ui_targets,
            |((ev, stock_symbol, time_frame), active_overlay_ui_target)| {
                (
                    *ev,
                    stock_symbol.clone(),
                    *time_frame,
                    *active_overlay_ui_target,
                )
            },
        )
        .fold(
            (None, None, None),
            |(acc_stock_symbol, acc_time_frame, acc_date_range): &(
                Option<String>,
                Option<TimeFrame>,
                Option<DateRange>,
            ),
             (ev, stock_symbol, time_frame, active_overlay_ui_target)| {
                let stock_symbol_changed =
                    acc_stock_symbol.is_some() && acc_stock_symbol.as_ref() != Some(stock_symbol);
                let time_frame_changed =
                    acc_time_frame.is_some() && acc_time_frame.as_ref() != Some(time_frame);
                let acc_date_range =
                    if acc_time_frame.is_some() && !stock_symbol_changed && !time_frame_changed {
                        acc_date_range.clone()
                    } else {
                        time_frame.now_date_range()
                    };
                if stock_symbol_changed || time_frame_changed {
                    return (
                        Some(stock_symbol.clone()),
                        Some(*time_frame),
                        acc_date_range,
                    );
                }

                match ev {
                    InputEvent::Key(KeyEvent { code, .. }) => match code {
                        KeyCode::Left if active_overlay_ui_target.is_none() => {
                            let date_range = time_frame.duration().map(|duration| {
                                acc_date_range
                                    .as_ref()
                                    .map(|acc_date_range| {
                                        let end_date = acc_date_range.start;
                                        (end_date - duration)..end_date
                                    })
                                    .unwrap()
                            });
                            (Some(stock_symbol.clone()), Some(*time_frame), date_range)
                        }
                        KeyCode::Right if active_overlay_ui_target.is_none() => {
                            let date_range = time_frame.duration().map(|duration| {
                                acc_date_range
                                    .as_ref()
                                    .map(|acc_date_range| {
                                        let start_date = acc_date_range.end;
                                        start_date..(start_date + duration)
                                    })
                                    .map(|date_range| {
                                        let max_date_range = time_frame.now_date_range().unwrap();
                                        if date_range.end > max_date_range.end {
                                            max_date_range
                                        } else {
                                            date_range
                                        }
                                    })
                                    .unwrap()
                            });
                            (Some(stock_symbol.clone()), Some(*time_frame), date_range)
                        }
                        _ => (acc_stock_symbol.clone(), *acc_time_frame, acc_date_range),
                    },
                    _ => (acc_stock_symbol.clone(), *acc_time_frame, acc_date_range),
                }
            },
        )
        .map(|(_, _, date_range)| date_range.clone())
        .distinct_until_changed()
}

#[derive(Clone, Debug)]
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
    IndicatorBox,
    IndicatorList,
    StockName,
    StockSymbol,
    StockSymbolInput,
    TimeFrameBox,
    TimeFrameList,
}

#[derive(Debug)]
pub struct FrameRateCounter {
    frame_time: AtomicU16,
    frames: AtomicU16,
    last_interval: RwLock<DateTime<Utc>>,
    update_interval: Duration,
}

impl FrameRateCounter {
    pub fn new(update_interval: Duration) -> Self {
        Self {
            frame_time: AtomicU16::new(0),
            frames: AtomicU16::new(0),
            last_interval: RwLock::new(Utc::now()),
            update_interval,
        }
    }

    /// Increments the counter. Returns the frame time if the update interval has elapsed.
    pub fn incr(&self) -> Option<Duration> {
        self.frames.fetch_add(1, atomic::Ordering::Relaxed);

        let now = Utc::now();

        if now >= *self.last_interval.read() + self.update_interval {
            let frames = self.frames.load(atomic::Ordering::Relaxed);
            let frame_time =
                (now - *self.last_interval.read()).num_milliseconds() as f64 / frames as f64;
            let frame_time = round::floor(frame_time, 0) as u16;
            self.frame_time.store(frame_time, atomic::Ordering::Relaxed);

            self.frames.store(0, atomic::Ordering::Relaxed);

            let mut last_interval = self.last_interval.write();
            *last_interval = now;

            return Some(Duration::milliseconds(frame_time as i64));
        }

        None
    }

    pub fn frame_time(&self) -> Option<Duration> {
        match self.frame_time.load(atomic::Ordering::Relaxed) {
            0 => None,
            frame_time => Some(Duration::milliseconds(frame_time as i64)),
        }
    }
}

#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
pub enum Indicator {
    BollingerBands(Period<U20>, StdDevMultiplier<U2>),
    ExponentialMovingAverage(Period<U50>),
    // MovingAverageConvergenceDivergence,
    // RelativeStrengthIndex,
    SimpleMovingAverage(Period<U50>),
}

#[derive(
    Clone, Copy, Debug, Display, Eq, From, Into, new, Ord, PartialEq, PartialOrd, Shrinkwrap,
)]
#[display(fmt = "{}", _0)]
pub struct Period<D: Unsigned>(#[shrinkwrap(main_field)] u16, PhantomData<*const D>);

impl<D> Default for Period<D>
where
    D: Unsigned,
{
    fn default() -> Self {
        Self::new(D::to_u16())
    }
}

impl<D> FromStr for Period<D>
where
    D: Unsigned,
{
    type Err = <u16 as FromStr>::Err;

    fn from_str(src: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(u16::from_str(src)?))
    }
}

#[derive(
    Clone, Copy, Debug, Display, Eq, From, Into, new, Ord, PartialEq, PartialOrd, Shrinkwrap,
)]
#[display(fmt = "{}", _0)]
pub struct StdDevMultiplier<D: Unsigned>(#[shrinkwrap(main_field)] u8, PhantomData<*const D>);

impl<D> Default for StdDevMultiplier<D>
where
    D: Unsigned,
{
    fn default() -> Self {
        Self::new(D::to_u8())
    }
}

impl<D> FromStr for StdDevMultiplier<D>
where
    D: Unsigned,
{
    type Err = <u8 as FromStr>::Err;

    fn from_str(src: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(u8::from_str(src)?))
    }
}

impl FromStr for Indicator {
    type Err = ParseIndicatorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const BB_PATTERN: &str = r"BB\s*\(\s*(?P<n>\d+)\s*,\s*(?P<k>\d+)\s*\)";
        const EMA_PATTERN: &str = r"EMA\s*\(\s*(?P<n>\d+)\s*\)";
        const SMA_PATTERN: &str = r"SMA\s*\(\s*(?P<n>\d+)\s*\)";

        static BB_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(BB_PATTERN).unwrap());
        static EMA_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(EMA_PATTERN).unwrap());
        static SMA_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(SMA_PATTERN).unwrap());

        if let Some(caps) = BB_REGEX.captures(s) {
            let n = &caps["n"];
            let n = n.parse().map_err(|err| ParseIndicatorError::ParseInt {
                name: "n".to_owned(),
                source: err,
                value: n.to_owned(),
            })?;
            let k = &caps["k"];
            let k = k.parse().map_err(|err| ParseIndicatorError::ParseInt {
                name: "k".to_owned(),
                source: err,
                value: k.to_owned(),
            })?;
            Ok(Indicator::BollingerBands(n, k))
        } else if let Some(caps) = EMA_REGEX.captures(s) {
            let n = &caps["n"];
            let n = n.parse().map_err(|err| ParseIndicatorError::ParseInt {
                name: "n".to_owned(),
                source: err,
                value: n.to_owned(),
            })?;
            Ok(Indicator::ExponentialMovingAverage(n))
        } else if let Some(caps) = SMA_REGEX.captures(s) {
            let n = &caps["n"];
            let n = n.parse().map_err(|err| ParseIndicatorError::ParseInt {
                name: "n".to_owned(),
                source: err,
                value: n.to_owned(),
            })?;
            Ok(Indicator::SimpleMovingAverage(n))
        } else if s == "" {
            Err(ParseIndicatorError::Empty)
        } else {
            Err(ParseIndicatorError::Invalid)
        }
    }
}

#[derive(Debug, Error)]
pub enum ParseIndicatorError {
    #[error("cannot parse indicator from empty string")]
    Empty,
    #[error("invalid indicator literal")]
    Invalid,
    #[error("invalid indicator parameter {}: {}", .name, .value)]
    ParseInt {
        name: String,
        source: ParseIntError,
        value: String,
    },
}

impl fmt::Display for Indicator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BollingerBands(n, k) => write!(f, "BB({}, {})", n, k),
            Self::ExponentialMovingAverage(n) => write!(f, "EMA({})", n),
            // Self::MovingAverageConvergenceDivergence => write!(f, "MACD"),
            // Self::RelativeStrengthIndex => write!(f, "RSI"),
            Self::SimpleMovingAverage(n) => write!(f, "SMA({})", n),
        }
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

    pub fn now_date_range(self) -> Option<DateRange> {
        self.duration().map(|duration| {
            let end_date = Utc::now().date().and_hms(0, 0, 0) + Duration::days(1);
            (end_date - duration)..end_date
        })
    }
}

impl Default for TimeFrame {
    fn default() -> Self {
        Self::OneMonth
    }
}

impl FromStr for TimeFrame {
    type Err = ParseTimeFrameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "5D" | "5d" => Ok(Self::FiveDays),
            "1M" | "1mo" => Ok(Self::OneMonth),
            "3M" | "3mo" => Ok(Self::ThreeMonths),
            "6M" | "6mo" => Ok(Self::SixMonths),
            "YTD" | "ytd" => Ok(Self::YearToDate),
            "1Y" | "1y" => Ok(Self::OneYear),
            "2Y" | "2y" => Ok(Self::TwoYears),
            "5Y" | "5y" => Ok(Self::FiveYears),
            "10Y" | "10y" => Ok(Self::TenYears),
            "Max" | "max" => Ok(Self::Max),
            "" => Err(ParseTimeFrameError::Empty),
            _ => Err(ParseTimeFrameError::Invalid),
        }
    }
}

#[derive(Debug, Error)]
pub enum ParseTimeFrameError {
    #[error("cannot parse time frame from empty string")]
    Empty,
    #[error("invalid time frame literal")]
    Invalid,
}

impl fmt::Display for TimeFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FiveDays => write!(f, "5D"),
            Self::OneMonth => write!(f, "1M"),
            Self::ThreeMonths => write!(f, "3M"),
            Self::SixMonths => write!(f, "6M"),
            Self::YearToDate => write!(f, "YTD"),
            Self::OneYear => write!(f, "1Y"),
            Self::TwoYears => write!(f, "2Y"),
            Self::FiveYears => write!(f, "5Y"),
            Self::TenYears => write!(f, "10Y"),
            Self::Max => write!(f, "Max"),
        }
    }
}
