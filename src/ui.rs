use crate::{
    app::{App, Indicator, TimeFrame, UiState, UiTarget},
    widgets::{SelectMenuBox, SelectMenuList, TextField},
};
use chrono::{Duration, TimeZone, Utc};
use im::{hashmap, HashMap};
use itertools::Itertools;
use itertools::MinMaxResult::{MinMax, NoElements, OneElement};
use math::round;
use std::{cmp, iter, ops::Range};
use strum::IntoEnumIterator;
use ta::indicators;
use ta::{DataItem, Next};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Span, Spans},
    widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, ListItem, Paragraph},
    Frame,
};
use yahoo_finance::Timestamped;

pub fn draw<B: Backend>(f: &mut Frame<B>, app: &App) -> anyhow::Result<()> {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(f.size());
    let header_area = chunks[0];
    let body_area = chunks[1];
    let footer_area = chunks[2];

    draw_header(f, app, header_area)?;
    draw_body(f, app, body_area)?;
    draw_footer(f, app, footer_area)?;
    draw_overlay(f, app)?;
    if app.ui_state.debug_draw {
        draw_debug(f, app)?;
    }

    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn draw_header<B: Backend>(
    f: &mut Frame<B>,
    App {
        stock, ui_state, ..
    }: &App,
    area: Rect,
) -> anyhow::Result<()> {
    let stock_name = stock.name().unwrap_or("");

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .horizontal_margin(1)
        .constraints(vec![
            Constraint::Length(10),
            Constraint::Length(cmp::max(stock_name.chars().count() as u16, 20)),
            Constraint::Min(0),
        ])
        .split(area);
    let stock_symbol_area = chunks[0];
    let stock_name_area = chunks[1];

    let header_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let header_block = Block::default().style(header_base_style);
    f.render_widget(header_block, area);

    let stock_symbol_paragraph = Paragraph::new(Span::styled(
        stock.symbol.as_str(),
        header_base_style.add_modifier(Modifier::BOLD),
    ))
    .block(Block::default().style(header_base_style));
    f.render_widget(stock_symbol_paragraph, stock_symbol_area);

    ui_state
        .ui_target_areas
        .send((UiTarget::StockSymbolButton, Some(stock_symbol_area)));

    let stock_name_paragraph = Paragraph::new(Span::styled(stock_name, header_base_style))
        .block(Block::default().style(header_base_style));
    f.render_widget(stock_name_paragraph, stock_name_area);

    ui_state
        .ui_target_areas
        .send((UiTarget::StockNameButton, Some(stock_name_area)));

    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn draw_body<B: Backend>(
    f: &mut Frame<B>,
    App { stock, ui_state }: &App,
    area: Rect,
) -> anyhow::Result<()> {
    const X_AXIS_LABEL_PADDING: u8 = 4;
    const X_AXIS_LABEL_WIDTH: u8 = 10;
    const Y_AXIS_LABEL_HEIGHT: u8 = 1;
    const Y_AXIS_LABEL_PADDING: u8 = 2;

    let mut historical_prices_data: HashMap<String, Vec<_>> = hashmap! {};
    let stock_data = stock
        .bars
        .iter()
        .filter(|&bar| {
            ui_state
                .date_range
                .as_ref()
                .map_or(true, |date_range| date_range.contains(&bar.datetime()))
        })
        .map(|bar| {
            (
                bar.timestamp_seconds() as f64,
                round::half_to_even(bar.close, 2),
            )
        })
        .collect();
    historical_prices_data.insert(stock.symbol.clone(), stock_data);

    let mut historical_prices_datasets = vec![];

    if let Some(indicator) = ui_state.indicator {
        let indicator_prices_data = stock.bars.iter().map(|bar| {
            let data_item = DataItem::builder()
                .open(bar.open)
                .high(bar.high)
                .low(bar.low)
                .close(bar.close);
            let data_item = if let Some(volume) = bar.volume {
                data_item.volume(volume as f64)
            } else {
                data_item
            };
            let data_item = data_item.build().unwrap();
            (bar.timestamp_seconds() as f64, data_item)
        });

        match indicator {
            Indicator::BollingerBands(n, k) => {
                let indicator_prices_data = indicator_prices_data.filter(|(timestamp, _)| {
                    ui_state.date_range.as_ref().map_or(true, |date_range| {
                        let date_range =
                            (date_range.start - Duration::days(*n as i64 - 1))..date_range.end;
                        date_range.contains(&Utc.timestamp(*timestamp as i64, 0))
                    })
                });
                let mut bb = indicators::BollingerBands::new(*n as usize, *k as f64).unwrap();
                let (bb_upper_data, bb_middle_data, bb_lower_data) = indicator_prices_data.fold(
                    (vec![], vec![], vec![]),
                    |mut acc_data, (timestamp, data_item)| {
                        let bb_output = bb.next(&data_item);
                        acc_data.0.push((timestamp, bb_output.upper));
                        acc_data.1.push((timestamp, bb_output.average));
                        acc_data.2.push((timestamp, bb_output.lower));
                        acc_data
                    },
                );
                historical_prices_data = historical_prices_data
                    + hashmap! {
                        "BB Upper".to_owned() => bb_upper_data,
                        "BB Middle".to_owned() => bb_middle_data,
                        "BB Lower".to_owned() => bb_lower_data,
                    };
                let bb_upper_data = historical_prices_data.get("BB Upper").unwrap();
                let bb_middle_data = historical_prices_data.get("BB Middle").unwrap();
                let bb_lower_data = historical_prices_data.get("BB Lower").unwrap();

                historical_prices_datasets.push(
                    Dataset::default()
                        .marker(Marker::Braille)
                        .style(Style::default().fg(Color::DarkGray))
                        .graph_type(GraphType::Line)
                        .data(&bb_upper_data),
                );
                historical_prices_datasets.push(
                    Dataset::default()
                        .marker(Marker::Braille)
                        .style(Style::default().fg(Color::DarkGray))
                        .graph_type(GraphType::Line)
                        .data(&bb_lower_data),
                );
                historical_prices_datasets.push(
                    Dataset::default()
                        .marker(Marker::Braille)
                        .style(Style::default().fg(Color::Cyan))
                        .graph_type(GraphType::Line)
                        .data(&bb_middle_data),
                );
            }
            Indicator::ExponentialMovingAverage(n) => {
                let indicator_prices_data = indicator_prices_data.filter(|(timestamp, _)| {
                    ui_state.date_range.as_ref().map_or(true, |date_range| {
                        let date_range =
                            (date_range.start - Duration::days(*n as i64 - 1))..date_range.end;
                        date_range.contains(&Utc.timestamp(*timestamp as i64, 0))
                    })
                });
                let mut ema = indicators::ExponentialMovingAverage::new(*n as usize).unwrap();
                let ema_data = indicator_prices_data
                    .map(|(timestamp, data_item)| (timestamp, ema.next(&data_item)))
                    .collect();
                historical_prices_data.insert("EMA".to_owned(), ema_data);
                let ema_data = historical_prices_data.get("EMA").unwrap();

                historical_prices_datasets.push(
                    Dataset::default()
                        .marker(Marker::Braille)
                        .style(Style::default().fg(Color::Cyan))
                        .graph_type(GraphType::Line)
                        .data(&ema_data),
                );
            }
            Indicator::SimpleMovingAverage(n) => {
                let indicator_prices_data = indicator_prices_data.filter(|(timestamp, _)| {
                    ui_state.date_range.as_ref().map_or(true, |date_range| {
                        let date_range =
                            (date_range.start - Duration::days(*n as i64 - 1))..date_range.end;
                        date_range.contains(&Utc.timestamp(*timestamp as i64, 0))
                    })
                });
                let mut sma = indicators::SimpleMovingAverage::new(*n as usize).unwrap();
                let sma_data = indicator_prices_data
                    .map(|(timestamp, data_item)| (timestamp, sma.next(&data_item)))
                    .collect();
                historical_prices_data.insert("SMA".to_owned(), sma_data);
                let sma_data = historical_prices_data.get("SMA").unwrap();

                historical_prices_datasets.push(
                    Dataset::default()
                        .marker(Marker::Braille)
                        .style(Style::default().fg(Color::Cyan))
                        .graph_type(GraphType::Line)
                        .data(&sma_data),
                );
            }
        }
    }

    let stock_data = historical_prices_data.get(&stock.symbol).unwrap();
    let (stock_timestamps, stock_prices): (Vec<_>, Vec<_>) = stock_data.clone().into_iter().unzip();

    let historical_prices_dataset = Dataset::default()
        .marker(Marker::Braille)
        .style(Style::default().fg({
            let first_price = stock_prices.first().unwrap_or(&0f64);
            let last_price = stock_prices.last().unwrap_or(&0f64);
            if last_price >= first_price {
                Color::Green
            } else {
                Color::Red
            }
        }))
        .graph_type(GraphType::Line)
        .data(&stock_data);
    historical_prices_datasets.push(historical_prices_dataset);

    let timestamp_steps: Vec<_> = match stock_timestamps.clone().into_iter().minmax() {
        MinMax(min, max) => {
            let n = cmp::min(
                round::floor(
                    (area.width - 2) as f64 / (X_AXIS_LABEL_WIDTH + X_AXIS_LABEL_PADDING) as f64,
                    0,
                ) as usize,
                stock_timestamps.len(),
            );

            itertools_num::linspace(min, max, n).collect()
        }
        OneElement(t) => vec![t, t],
        NoElements => {
            if let Some(Range {
                start: start_date,
                end: end_date,
            }) = &ui_state.date_range
            {
                vec![
                    start_date.timestamp() as f64,
                    (*end_date - Duration::seconds(1)).timestamp() as f64,
                ]
            } else {
                vec![
                    Utc.ymd(1, 1, 1).and_hms(0, 0, 0).timestamp() as f64,
                    Utc::now().timestamp() as f64,
                ]
            }
        }
    };
    let x_axis_bounds = [
        *timestamp_steps.first().unwrap(),
        *timestamp_steps.last().unwrap(),
    ];
    let x_axis_labels: Vec<_> = timestamp_steps
        .iter()
        .map(|&t| Span::from(Utc.timestamp(t as i64, 0).format("%Y-%m-%d").to_string()))
        .collect();

    let (_, prices): (Vec<_>, Vec<_>) = historical_prices_data.values().flatten().copied().unzip();
    let price_steps: Vec<_> = match prices.into_iter().minmax() {
        MinMax(min, max) => {
            let n = round::floor(
                (area.height - 2) as f64 / (Y_AXIS_LABEL_HEIGHT + Y_AXIS_LABEL_PADDING) as f64,
                0,
            ) as usize;

            itertools_num::linspace(min, max, n).collect()
        }
        OneElement(p) => vec![p, p],
        NoElements => vec![0_f64, f64::INFINITY],
    };
    let y_axis_bounds = [*price_steps.first().unwrap(), *price_steps.last().unwrap()];
    let y_axis_labels: Vec<_> = price_steps
        .iter()
        .map(|&p| Span::from(format!("{:.2}", p)))
        .collect();

    let historical_prices_chart = Chart::new(historical_prices_datasets)
        .block(
            Block::default()
                .title("Historical Prices")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Gray)),
        )
        .x_axis(Axis::default().bounds(x_axis_bounds).labels(x_axis_labels))
        .y_axis(Axis::default().bounds(y_axis_bounds).labels(y_axis_labels));
    f.render_widget(historical_prices_chart, area);

    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn draw_footer<B: Backend>(
    f: &mut Frame<B>,
    App { ui_state, .. }: &App,
    area: Rect,
) -> anyhow::Result<()> {
    let (indicator_box_area, time_frame_box_area) = {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![
                Constraint::Min(0),
                Constraint::Length(30),
                Constraint::Length(20),
            ])
            .split(area);
        (chunks[1], chunks[2])
    };

    let menu_active_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let indicator_menu_state = ui_state.indicator_menu_state.borrow();

    let indicator_box_area = {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .horizontal_margin(if indicator_menu_state.active { 0 } else { 1 })
            .constraints(vec![Constraint::Min(0)])
            .split(indicator_box_area);
        chunks[0]
    };

    let indicator_box = SelectMenuBox::new(Spans::from(vec![
        Span::styled(
            "Indicator: ",
            if indicator_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
        Span::styled(
            if let Some(indicator) = ui_state.indicator {
                indicator.to_string()
            } else {
                "None".to_owned()
            },
            if indicator_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
    ]))
    .active_style(menu_active_base_style)
    .active_border_style(Style::default().fg(Color::Gray))
    .alignment(Alignment::Right);
    drop(indicator_menu_state);
    let mut indicator_menu_state = ui_state.indicator_menu_state.borrow_mut();
    f.render_stateful_widget(indicator_box, indicator_box_area, &mut indicator_menu_state);

    ui_state
        .ui_target_areas
        .send((UiTarget::IndicatorBox, Some(indicator_box_area)));

    let time_frame_menu_state = ui_state.time_frame_menu_state.borrow();

    let time_frame_area = {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .horizontal_margin(if time_frame_menu_state.active { 0 } else { 1 })
            .constraints(vec![Constraint::Min(0)])
            .split(time_frame_box_area);
        chunks[0]
    };

    let time_frame_box = SelectMenuBox::new(Spans::from(vec![
        Span::styled(
            "Time frame: ",
            if time_frame_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
        Span::styled(
            ui_state.time_frame.to_string(),
            if time_frame_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
    ]))
    .active_style(menu_active_base_style)
    .active_border_style(Style::default().fg(Color::Gray))
    .alignment(Alignment::Right);
    drop(time_frame_menu_state);
    let mut time_frame_menu_state = ui_state.time_frame_menu_state.borrow_mut();
    f.render_stateful_widget(time_frame_box, time_frame_area, &mut time_frame_menu_state);

    ui_state
        .ui_target_areas
        .send((UiTarget::TimeFrameBox, Some(time_frame_area)));

    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn draw_overlay<B: Backend>(f: &mut Frame<B>, App { ui_state, .. }: &App) -> anyhow::Result<()> {
    let active_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);
    let highlight_base_style = Style::default().fg(Color::Black).bg(Color::White);

    let stock_symbol_field_state = ui_state.stock_symbol_field_state.borrow();

    if stock_symbol_field_state.active {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Length(30), Constraint::Min(0)])
            .split(f.size());
        let stock_symbol_field_area = chunks[0];
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(stock_symbol_field_area);
        let stock_symbol_field_area = chunks[1];

        let stock_symbol_field = TextField::new(Span::styled(
            stock_symbol_field_state.value.clone(),
            active_base_style,
        ))
        .border_style(Style::default().fg(Color::Gray));
        drop(stock_symbol_field_state);
        let mut stock_symbol_field_state = ui_state.stock_symbol_field_state.borrow_mut();
        f.render_stateful_widget(
            stock_symbol_field,
            stock_symbol_field_area,
            &mut stock_symbol_field_state,
        );

        ui_state
            .ui_target_areas
            .send((UiTarget::StockSymbolField, Some(stock_symbol_field_area)));
    } else {
        ui_state
            .ui_target_areas
            .send((UiTarget::StockSymbolField, None));
    }

    let indicator_menu_state = ui_state.indicator_menu_state.borrow();

    if indicator_menu_state.active {
        let indicator_list_area = {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![
                    Constraint::Min(0),
                    Constraint::Length(30),
                    Constraint::Length(20),
                ])
                .split(f.size());
            let indicator_list_area = chunks[1];
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![
                    Constraint::Min(0),
                    Constraint::Length(cmp::min(
                        Indicator::iter().count() as u16 + 1 + 2,
                        indicator_list_area.height - 2,
                    )),
                    Constraint::Length(2),
                ])
                .split(indicator_list_area);
            chunks[1]
        };

        let indicator_menu_items: Vec<_> = iter::once("None".to_owned())
            .chain(Indicator::iter().map(|t| t.to_string()))
            .map(ListItem::new)
            .collect();
        let indicator_list = SelectMenuList::new(indicator_menu_items)
            .border_style(Style::default().fg(Color::Gray))
            .highlight_style(highlight_base_style);
        drop(indicator_menu_state);
        let mut indicator_menu_state = ui_state.indicator_menu_state.borrow_mut();
        f.render_stateful_widget(
            indicator_list,
            indicator_list_area,
            &mut indicator_menu_state,
        );

        ui_state
            .ui_target_areas
            .send((UiTarget::IndicatorMenu, Some(indicator_list_area)));
    } else {
        ui_state
            .ui_target_areas
            .send((UiTarget::IndicatorMenu, None));
    }

    let time_frame_menu_state = ui_state.time_frame_menu_state.borrow();

    if time_frame_menu_state.active {
        let time_frame_list_area = {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![Constraint::Min(0), Constraint::Length(20)])
                .split(f.size());
            let time_frame_list_area = chunks[1];
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![
                    Constraint::Min(0),
                    Constraint::Length(cmp::min(
                        TimeFrame::iter().count() as u16 + 2,
                        time_frame_list_area.height - 2,
                    )),
                    Constraint::Length(2),
                ])
                .split(time_frame_list_area);
            chunks[1]
        };

        let time_frame_menu_items: Vec<_> = TimeFrame::iter()
            .map(|t| ListItem::new(t.to_string()))
            .collect();
        let time_frame_list = SelectMenuList::new(time_frame_menu_items)
            .border_style(Style::default().fg(Color::Gray))
            .highlight_style(highlight_base_style);
        drop(time_frame_menu_state);
        let mut time_frame_menu_state = ui_state.time_frame_menu_state.borrow_mut();
        f.render_stateful_widget(
            time_frame_list,
            time_frame_list_area,
            &mut time_frame_menu_state,
        );

        ui_state
            .ui_target_areas
            .send((UiTarget::TimeFrameMenu, Some(time_frame_list_area)));
    } else {
        ui_state
            .ui_target_areas
            .send((UiTarget::TimeFrameMenu, None));
    }

    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn draw_debug<B: Backend>(
    f: &mut Frame<B>,
    App {
        ui_state: UiState {
            frame_rate_counter, ..
        },
        ..
    }: &App,
) -> anyhow::Result<()> {
    let frame_time = {
        let mut frame_rate_counter = frame_rate_counter.borrow_mut();
        if let Some(frame_time) = frame_rate_counter.incr() {
            Some(frame_time)
        } else {
            frame_rate_counter.frame_time()
        }
    };
    let frame_time_text = if let Some(frame_time) = frame_time {
        format!("{} ms", frame_time.num_milliseconds())
    } else {
        "...".to_owned()
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Length(20), Constraint::Min(0)])
        .split(f.size());
    let timestamp_area = chunks[0];
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Min(0), Constraint::Length(1)])
        .split(timestamp_area);
    let timestamp_area = chunks[1];

    let timestamp_paragraph = Paragraph::new(Spans::from(vec![
        Span::styled("Frame time: ", Style::default()),
        Span::styled(
            frame_time_text,
            if let Some(frame_time) = frame_time {
                if frame_time
                    >= Duration::milliseconds(round::ceil(crate::TICK_RATE as f64 * 1.1, 0) as i64)
                {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                }
            } else {
                Style::default()
            },
        ),
    ]));

    f.render_widget(Clear, timestamp_area);
    f.render_widget(timestamp_paragraph, timestamp_area);

    Ok(())
}
