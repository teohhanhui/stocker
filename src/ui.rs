use crate::app::{App, Indicator, TimeFrame, UiState, UiTarget};
use chrono::{Duration, TimeZone, Utc};
use itertools::Itertools;
use itertools::MinMaxResult::{MinMax, NoElements, OneElement};
use math::round;
use std::cmp;
use strum::IntoEnumIterator;
use ta::indicators;
use ta::{DataItem, Next};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, List, Paragraph, Text},
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
    if app.ui_state.debug_draw() {
        draw_debug(f, app)?;
    }

    Ok(())
}

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
            Constraint::Length(stock_name.chars().count() as u16),
            Constraint::Min(0),
        ])
        .split(area);
    let stock_symbol_area = chunks[0];
    let stock_name_area = chunks[1];

    let header_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let header_block = Block::default().style(header_base_style);
    f.render_widget(header_block, area);

    let stock_symbol_texts = vec![Text::raw(stock.symbol.as_str())];
    let stock_symbol_paragraph = Paragraph::new(stock_symbol_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style.clone().modifier(Modifier::BOLD));
    f.render_widget(stock_symbol_paragraph, stock_symbol_area);

    ui_state.set_target_area(UiTarget::StockSymbol, stock_symbol_area)?;

    let stock_name_texts = vec![Text::raw(stock_name)];
    let stock_name_paragraph = Paragraph::new(stock_name_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style);
    f.render_widget(stock_name_paragraph, stock_name_area);

    ui_state.set_target_area(UiTarget::StockName, stock_name_area)?;

    Ok(())
}

fn draw_body<B: Backend>(
    f: &mut Frame<B>,
    App { stock, ui_state }: &App,
    area: Rect,
) -> anyhow::Result<()> {
    const X_AXIS_LABEL_PADDING: u8 = 4;
    const X_AXIS_LABEL_WIDTH: u8 = 10;
    const Y_AXIS_LABEL_HEIGHT: u8 = 1;
    const Y_AXIS_LABEL_PADDING: u8 = 2;

    let historical_prices_data = stock.bars.iter().map(|bar| {
        (
            bar.timestamp_seconds() as f64,
            round::half_to_even(bar.close, 2),
        )
    });
    let (timestamps, prices): (Vec<_>, Vec<_>) = historical_prices_data.clone().unzip();

    let timestamp_steps: Vec<_> = match timestamps.clone().into_iter().minmax() {
        MinMax(min, max) => {
            let n = cmp::min(
                round::floor(
                    (area.width - 2) as f64 / (X_AXIS_LABEL_WIDTH + X_AXIS_LABEL_PADDING) as f64,
                    0,
                ) as usize,
                timestamps.len(),
            );

            itertools_num::linspace(min, max, n).collect()
        }
        OneElement(t) => vec![t, t],
        NoElements => vec![
            Utc.ymd(1, 1, 1).and_hms(0, 0, 0).timestamp() as f64,
            if let Some(end_date) = ui_state.end_date {
                end_date.timestamp()
            } else {
                Utc::now().timestamp()
            } as f64,
        ],
    };
    let x_axis_bounds = [
        *timestamp_steps.first().unwrap(),
        *timestamp_steps.last().unwrap(),
    ];
    let x_axis_labels: Vec<_> = timestamp_steps
        .iter()
        .map(|&t| Utc.timestamp(t as i64, 0).format("%Y-%m-%d").to_string())
        .collect();

    let price_steps: Vec<_> = match prices.clone().into_iter().minmax() {
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
    let y_axis_labels: Vec<_> = price_steps.iter().map(|&p| format!("{:.2}", p)).collect();

    let mut historical_prices_datasets = vec![];

    let bb_data: (Vec<_>, Vec<_>, Vec<_>);
    let ema_data: Vec<_>;
    let sma_data: Vec<_>;
    if let Some(indicator) = ui_state.indicator {
        let indicator_prices_data = stock.bars.iter().map(|bar| {
            let mut data_item = DataItem::builder()
                .open(bar.open)
                .high(bar.high)
                .low(bar.low)
                .close(bar.close);
            if let Some(volume) = bar.volume {
                data_item = data_item.volume(volume as f64);
            }
            let data_item = data_item.build().unwrap();
            (bar.timestamp_seconds() as f64, data_item)
        });

        match indicator {
            Indicator::BollingerBands => {
                let mut bb = indicators::BollingerBands::default();
                bb_data = indicator_prices_data.fold(
                    (vec![], vec![], vec![]),
                    |mut state, (timestamp, data_item)| {
                        let bb_output = bb.next(&data_item);
                        state.0.push((timestamp, bb_output.upper));
                        state.1.push((timestamp, bb_output.average));
                        state.2.push((timestamp, bb_output.lower));
                        state
                    },
                );

                historical_prices_datasets.push(
                    Dataset::default()
                        .marker(Marker::Braille)
                        .style(Style::default().fg(Color::DarkGray))
                        .graph_type(GraphType::Line)
                        .data(&bb_data.0),
                );
                historical_prices_datasets.push(
                    Dataset::default()
                        .marker(Marker::Braille)
                        .style(Style::default().fg(Color::DarkGray))
                        .graph_type(GraphType::Line)
                        .data(&bb_data.2),
                );
                historical_prices_datasets.push(
                    Dataset::default()
                        .marker(Marker::Braille)
                        .style(Style::default().fg(Color::Cyan))
                        .graph_type(GraphType::Line)
                        .data(&bb_data.1),
                );
            }
            Indicator::ExponentialMovingAverage(_) => {
                let mut ema = indicators::ExponentialMovingAverage::default();
                ema_data = indicator_prices_data
                    .map(|(timestamp, data_item)| (timestamp, ema.next(&data_item)))
                    .collect();

                historical_prices_datasets.push(
                    Dataset::default()
                        .marker(Marker::Braille)
                        .style(Style::default().fg(Color::Cyan))
                        .graph_type(GraphType::Line)
                        .data(&ema_data),
                );
            }
            Indicator::SimpleMovingAverage(period) => {
                let mut sma =
                    indicators::SimpleMovingAverage::new(u16::from(period) as u32).unwrap();
                sma_data = indicator_prices_data
                    .map(|(timestamp, data_item)| (timestamp, sma.next(&data_item)))
                    .collect();

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

    let historical_prices_data: Vec<_> = historical_prices_data.collect();
    let historical_prices_dataset = Dataset::default()
        .marker(Marker::Braille)
        .style(Style::default().fg({
            let first_price = prices.first().unwrap_or(&0f64);
            let last_price = prices.last().unwrap_or(&0f64);
            if last_price >= first_price {
                Color::Green
            } else {
                Color::Red
            }
        }))
        .graph_type(GraphType::Line)
        .data(&historical_prices_data);
    historical_prices_datasets.push(historical_prices_dataset);

    let historical_prices_chart = Chart::default()
        .block(
            Block::default()
                .title("Historical Prices")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Gray)),
        )
        .x_axis(Axis::default().bounds(x_axis_bounds).labels(&x_axis_labels))
        .y_axis(Axis::default().bounds(y_axis_bounds).labels(&y_axis_labels))
        .datasets(&historical_prices_datasets);
    f.render_widget(historical_prices_chart, area);

    Ok(())
}

fn draw_footer<B: Backend>(
    f: &mut Frame<B>,
    App { ui_state, .. }: &App,
    area: Rect,
) -> anyhow::Result<()> {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .horizontal_margin(if ui_state.time_frame_menu_state.active {
            0
        } else {
            1
        })
        .constraints(vec![
            Constraint::Min(0),
            Constraint::Length(30),
            Constraint::Length(20),
        ])
        .split(area);
    let indicator_area = chunks[1];
    let time_frame_area = chunks[2];

    let menu_active_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let indicators_texts = vec![
        Text::styled(
            "Indicator: ",
            if ui_state.indicator_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
        Text::styled(
            if let Some(indicator) = ui_state.indicator {
                indicator.to_string()
            } else {
                "None".to_string()
            },
            if ui_state.indicator_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
    ];
    let indicators_paragraph = Paragraph::new(indicators_texts.iter())
        .block(if ui_state.indicator_menu_state.active {
            Block::default()
                .style(if ui_state.indicator_menu_state.active {
                    menu_active_base_style
                } else {
                    Style::default()
                })
                .borders(Borders::ALL ^ Borders::TOP)
                .border_style(Style::default().fg(Color::Gray))
        } else {
            Block::default()
        })
        .style(if ui_state.indicator_menu_state.active {
            menu_active_base_style
        } else {
            Style::default()
        })
        .alignment(Alignment::Right);
    f.render_widget(indicators_paragraph, indicator_area);

    let time_frame_texts = vec![
        Text::styled(
            "Time frame: ",
            if ui_state.time_frame_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
        Text::styled(
            ui_state.time_frame.to_string(),
            if ui_state.time_frame_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
    ];
    let time_frame_paragraph = Paragraph::new(time_frame_texts.iter())
        .block(if ui_state.time_frame_menu_state.active {
            Block::default()
                .style(if ui_state.time_frame_menu_state.active {
                    menu_active_base_style
                } else {
                    Style::default()
                })
                .borders(Borders::ALL ^ Borders::TOP)
                .border_style(Style::default().fg(Color::Gray))
        } else {
            Block::default()
        })
        .style(if ui_state.time_frame_menu_state.active {
            menu_active_base_style
        } else {
            Style::default()
        })
        .alignment(Alignment::Right);
    f.render_widget(time_frame_paragraph, time_frame_area);

    ui_state.set_target_area(UiTarget::Indicator, indicator_area)?;
    ui_state.set_target_area(UiTarget::TimeFrame, time_frame_area)?;

    Ok(())
}

fn draw_overlay<B: Backend>(f: &mut Frame<B>, App { ui_state, .. }: &App) -> anyhow::Result<()> {
    let active_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);
    let highlight_base_style = Style::default().fg(Color::Black).bg(Color::White);

    if ui_state.stock_symbol_input_state.active {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Length(30), Constraint::Min(0)])
            .split(f.size());
        let stock_symbol_input_area = chunks[0];
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(stock_symbol_input_area);
        let stock_symbol_input_area = chunks[1];

        let stock_symbol_input_texts =
            vec![Text::raw(ui_state.stock_symbol_input_state.value.as_str())];
        let stock_symbol_input_paragraph = Paragraph::new(stock_symbol_input_texts.iter())
            .block(
                Block::default()
                    .style(active_base_style)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Gray)),
            )
            .style(active_base_style);
        f.render_widget(Clear, stock_symbol_input_area);
        f.render_widget(stock_symbol_input_paragraph, stock_symbol_input_area);

        ui_state.set_target_area(UiTarget::StockSymbolInput, stock_symbol_input_area)?;
    }

    if ui_state.indicator_menu_state.active {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Min(0), Constraint::Length(30)])
            .split(f.size());
        let indicator_menu_area = chunks[1];
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(0),
                Constraint::Length(cmp::min(
                    Indicator::iter().count() as u16 + 2,
                    indicator_menu_area.height - 2,
                )),
                Constraint::Length(2),
            ])
            .split(indicator_menu_area);
        let indicator_menu_area = chunks[1];

        let indicator_menu_items = Indicator::iter().map(|t| Text::raw(t.to_string()));
        let indicator_menu_list = List::new(indicator_menu_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Gray)),
            )
            .highlight_style(highlight_base_style);

        f.render_widget(Clear, indicator_menu_area);
        f.render_stateful_widget(
            indicator_menu_list,
            indicator_menu_area,
            &mut *ui_state.indicator_menu_state.list_state_write(),
        );

        ui_state.set_target_area(UiTarget::IndicatorMenu, indicator_menu_area)?;
    }

    if ui_state.time_frame_menu_state.active {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Min(0), Constraint::Length(20)])
            .split(f.size());
        let time_frame_menu_area = chunks[1];
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(0),
                Constraint::Length(cmp::min(
                    TimeFrame::iter().count() as u16 + 2,
                    time_frame_menu_area.height - 2,
                )),
                Constraint::Length(2),
            ])
            .split(time_frame_menu_area);
        let time_frame_menu_area = chunks[1];

        let time_frame_menu_items = TimeFrame::iter().map(|t| Text::raw(t.to_string()));
        let time_frame_menu_list = List::new(time_frame_menu_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Gray)),
            )
            .highlight_style(highlight_base_style);

        f.render_widget(Clear, time_frame_menu_area);
        f.render_stateful_widget(
            time_frame_menu_list,
            time_frame_menu_area,
            &mut *ui_state.time_frame_menu_state.list_state_write(),
        );

        ui_state.set_target_area(UiTarget::TimeFrameMenu, time_frame_menu_area)?;
    }

    Ok(())
}

fn draw_debug<B: Backend>(
    f: &mut Frame<B>,
    App {
        ui_state: UiState {
            frame_rate_counter, ..
        },
        ..
    }: &App,
) -> anyhow::Result<()> {
    let frame_time = if let Some(frame_time) = frame_rate_counter.incr() {
        Some(frame_time)
    } else {
        frame_rate_counter.frame_time()
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

    let timestamp_texts = vec![
        Text::styled("Frame time: ", Style::default()),
        Text::styled(
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
    ];
    let timestamp_paragraph = Paragraph::new(timestamp_texts.iter());

    f.render_widget(Clear, timestamp_area);
    f.render_widget(timestamp_paragraph, timestamp_area);

    Ok(())
}
