use crate::{app::TimeFrame, reactive::StreamExt};
use chrono::{DateTime, Duration, TimeZone, Utc};
use futures::executor;
use gcollections::ops::{Bounded, Difference, Union};
use im::{hashmap, ordset, HashMap, OrdSet};
use interval::interval_set::{IntervalSet, ToIntervalSet};
use reactive_rs::Stream;
use std::{cell::RefCell, ops::Range, rc::Rc};
use yahoo_finance::{history, Bar, Profile, Quote, Timestamped};

#[derive(Clone, Debug, Default)]
pub struct Stock {
    pub bars: OrdSet<Bar>,
    pub profile: Option<Profile>,
    pub quote: Option<Quote>,
    pub symbol: String,
}

impl Stock {
    pub fn name(&self) -> Option<&str> {
        match &self.profile {
            Some(Profile::Company(company)) => Some(company.name.as_str()),
            Some(Profile::Fund(fund)) => Some(fund.name.as_str()),
            None => None,
        }
    }
}

pub fn to_stock_profiles<'a, S>(stock_symbols: S) -> ToStockProfiles<S>
where
    S: Stream<'a, Item = String>,
{
    ToStockProfiles {
        stock_profile_map: Rc::new(RefCell::new(hashmap! {})),
        stock_symbols,
    }
}

pub struct ToStockProfiles<S> {
    stock_profile_map: Rc<RefCell<HashMap<String, Profile>>>,
    stock_symbols: S,
}

impl<'a, S> Stream<'a> for ToStockProfiles<S>
where
    S: Stream<'a, Item = String>,
{
    type Context = S::Context;
    type Item = Profile;

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        let stock_profile_map = self.stock_profile_map.clone();
        self.stock_symbols
            .distinct_until_changed()
            .subscribe_ctx(move |ctx, stock_symbol| {
                let profile = {
                    let stock_profile_map = stock_profile_map.borrow();
                    stock_profile_map.get(stock_symbol).cloned()
                };
                let profile = profile.unwrap_or_else(|| {
                    let profile = executor::block_on(Profile::load(stock_symbol.as_str()))
                        .expect("profile load failed");
                    let mut stock_profile_map = stock_profile_map.borrow_mut();
                    stock_profile_map.insert(stock_symbol.clone(), profile.clone());
                    profile
                });

                observer(ctx, &profile);
            });
    }
}

pub fn to_stock_bar_sets<'a, S, U, R>(
    stock_symbols: S,
    time_frames: U,
    date_ranges: R,
) -> ToStockBarSets<S, U, R>
where
    S: Stream<'a, Item = String>,
    U: Stream<'a, Item = TimeFrame>,
    R: Stream<'a, Item = Option<Range<DateTime<Utc>>>>,
{
    ToStockBarSets {
        date_ranges,
        stock_bars_map: Rc::new(RefCell::new(hashmap! {})),
        stock_symbols,
        time_frames,
    }
}

type DateRangeIntervalSet = IntervalSet<i64>;
type BarCoverageHashMap = HashMap<String, (OrdSet<Bar>, DateRangeIntervalSet)>;

pub struct ToStockBarSets<S, U, R> {
    date_ranges: R,
    stock_bars_map: Rc<RefCell<BarCoverageHashMap>>,
    stock_symbols: S,
    time_frames: U,
}

impl<'a, S, U, R, C> Stream<'a> for ToStockBarSets<S, U, R>
where
    S: Stream<'a, Item = String, Context = C>,
    U: Stream<'a, Item = TimeFrame>,
    R: Stream<'a, Item = Option<Range<DateTime<Utc>>>>,
    C: 'a + Clone + Sized,
{
    type Context = C;
    type Item = OrdSet<Bar>;

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        let stock_bars_map = self.stock_bars_map.clone();
        self.stock_symbols
            .distinct_until_changed()
            .combine_latest(
                self.time_frames.distinct_until_changed(),
                |(stock_symbol, time_frame)| (stock_symbol.clone(), *time_frame),
            )
            .combine_latest(
                self.date_ranges.distinct_until_changed(),
                |((stock_symbol, time_frame), date_range)| {
                    (stock_symbol.clone(), *time_frame, date_range.clone())
                },
            )
            .subscribe_ctx(move |ctx, (stock_symbol, time_frame, date_range)| {
                let (stock_bar_set, covered_date_ranges) = {
                    let stock_bars_map = stock_bars_map.borrow();
                    stock_bars_map
                        .get(stock_symbol)
                        .cloned()
                        .unwrap_or((ordset![], vec![].to_interval_set()))
                };

                let (stock_bar_set, covered_date_ranges) = if let Some(date_range) = date_range {
                    let uncovered_date_ranges = (
                        date_range.start.timestamp(),
                        (date_range.end - Duration::seconds(1)).timestamp(),
                    )
                        .to_interval_set()
                        .difference(&covered_date_ranges);

                    let mut covered_date_ranges = covered_date_ranges;
                    let mut stock_bar_set = stock_bar_set;
                    for uncovered_date_range in uncovered_date_ranges {
                        let bars = executor::block_on(history::retrieve_range(
                            stock_symbol.as_str(),
                            Utc.timestamp(uncovered_date_range.lower(), 0),
                            Some(Utc.timestamp(uncovered_date_range.upper(), 0)),
                        ))
                        .expect("historical prices retrieval failed");
                        covered_date_ranges = covered_date_ranges.union(
                            &vec![(uncovered_date_range.lower(), uncovered_date_range.upper())]
                                .to_interval_set(),
                        );
                        stock_bar_set = stock_bar_set + OrdSet::from(bars);
                    }

                    (stock_bar_set, covered_date_ranges)
                } else {
                    let bars = executor::block_on(history::retrieve_interval(
                        stock_symbol.as_str(),
                        time_frame.interval(),
                    ))
                    .expect("historical prices retrieval failed");
                    let covered_date_ranges =
                        if let (Some(first_bar), Some(last_bar)) = (bars.first(), bars.last()) {
                            covered_date_ranges.union(
                                &vec![(
                                    first_bar.timestamp_seconds() as i64,
                                    last_bar.timestamp_seconds() as i64,
                                )]
                                .to_interval_set(),
                            )
                        } else {
                            covered_date_ranges
                        };
                    let stock_bar_set = stock_bar_set + OrdSet::from(bars);
                    (stock_bar_set, covered_date_ranges)
                };

                observer(ctx, &stock_bar_set);

                let mut stock_bars_map = stock_bars_map.borrow_mut();
                stock_bars_map.insert(stock_symbol.clone(), (stock_bar_set, covered_date_ranges));
            });
    }
}
